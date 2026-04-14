//! Ownership-aware wrappers for Lean FFI references.
//!
//! The two core reference types are:
//! - [`LeanOwned`]: Owned reference. `Drop` calls `lean_dec`, `Clone` calls `lean_inc`. Not `Copy`.
//! - [`LeanBorrowed`]: Borrowed reference. `Copy`, no `Drop`, lifetime-bounded.
//!
//! Domain types like [`LeanArray`], [`LeanCtor`], etc. are generic over `R: LeanRef`,
//! inheriting ownership semantics from the inner reference type.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use crate::include;

/// Assert that runs only when the `test-ffi` feature is enabled (i.e. during `lake test`).
macro_rules! test_assert {
    ($($arg:tt)*) => {
        #[cfg(feature = "test-ffi")]
        assert!($($arg)*);
    };
}
use crate::safe_cstring;

// Tag constants from lean.h (only used by test_assert! when test-ffi is enabled)
#[cfg(feature = "test-ffi")]
mod tags {
    pub(super) const LEAN_MAX_CTOR_TAG: u8 = 243;
    pub(super) const LEAN_TAG_ARRAY: u8 = 246;
    pub(super) const LEAN_TAG_SCALAR_ARRAY: u8 = 248;
    pub(super) const LEAN_TAG_STRING: u8 = 249;
    pub(super) const LEAN_TAG_EXTERNAL: u8 = 254;
}
#[cfg(feature = "test-ffi")]
use tags::*;

/// Constructor tag for `IO.Error.userError`.
const IO_ERROR_USER_ERROR_TAG: u8 = 7;

/// Convert a `usize` to `u32` for the Lean C API, panicking on overflow.
#[inline]
fn to_u32(val: usize) -> u32 {
    u32::try_from(val).expect("value exceeds u32::MAX")
}

// =============================================================================
// LeanRef trait — shared interface for owned and borrowed references
// =============================================================================
//
// lean.h base object header (8 bytes):
//   typedef struct {
//       int      m_rc;       // >0 single-threaded, <0 multi-threaded, 0 persistent
//       unsigned m_cs_sz:16;
//       unsigned m_other:8;  // num_objs (ctors) or element size (scalar arrays)
//       unsigned m_tag:8;    // object type tag (0–243 ctor, 246 array, 248 sarray, ...)
//   } lean_object;

/// Trait for types that hold a reference to a Lean object (owned or borrowed).
///
/// Provides shared read-only operations. Implemented by [`LeanOwned`] and [`LeanBorrowed`].
pub trait LeanRef: Clone {
    /// Get the raw `*mut lean_object` pointer.
    fn as_raw(&self) -> *mut include::lean_object;

    /// True if this is a tagged scalar (bit 0 set), not a heap pointer.
    #[inline]
    fn is_scalar(&self) -> bool {
        self.as_raw() as usize & 1 == 1
    }

    /// Return the object tag. Panics if the object is a scalar.
    #[inline]
    fn tag(&self) -> u8 {
        assert!(!self.is_scalar(), "tag() called on scalar");
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            include::lean_obj_tag(self.as_raw()) as u8
        }
    }

    /// True if this object has exactly one reference and is in single-threaded mode.
    /// When exclusive, the object can be safely mutated in place without copying.
    #[inline]
    fn is_exclusive(&self) -> bool {
        !self.is_scalar() && unsafe { include::lean_is_exclusive(self.as_raw()) }
    }

    /// True if this is a persistent object (m_rc == 0). Persistent objects live
    /// for the program's lifetime and must not have their reference count modified.
    /// Objects in compact regions and values created at initialization time are persistent.
    #[inline]
    fn is_persistent(&self) -> bool {
        !self.is_scalar() && unsafe { include::lean_is_persistent(self.as_raw()) }
    }

    /// Unbox a tagged scalar pointer into a `usize`.
    #[inline]
    fn unbox_usize(&self) -> usize {
        self.as_raw() as usize >> 1
    }

    /// Extract the raw tag value from a zero-field enum constructor.
    #[inline]
    fn as_enum_tag(&self) -> usize {
        self.as_raw() as usize
    }

    /// Unbox a Lean `UInt64` object.
    #[inline]
    fn unbox_u64(&self) -> u64 {
        unsafe { include::lean_unbox_uint64(self.as_raw()) }
    }

    /// Unbox a Lean `UInt32` object.
    #[inline]
    fn unbox_u32(&self) -> u32 {
        unsafe { include::lean_unbox_uint32(self.as_raw()) }
    }

    /// Unbox a Lean `Float` (f64) object.
    #[inline]
    fn unbox_f64(&self) -> f64 {
        unsafe { include::lean_unbox_float(self.as_raw()) }
    }

    /// Unbox a Lean `Float32` (f32) object.
    #[inline]
    fn unbox_f32(&self) -> f32 {
        unsafe { include::lean_unbox_float32(self.as_raw()) }
    }

    /// Unbox a Lean `USize` object (heap-allocated, not tagged scalar).
    #[inline]
    fn unbox_usize_obj(&self) -> usize {
        unsafe { include::lean_unbox_usize(self.as_raw()) }
    }
}

// =============================================================================
// LeanOwned — Owned reference to a Lean object (RAII)
// =============================================================================

/// An owned reference to a Lean object, in the sense of
/// [Counting Immutable Beans](https://arxiv.org/abs/1908.05647): the holder
/// of an owned reference must call `lean_dec` exactly once.
///
/// In the Lean C API, owned and borrowed references are both raw `lean_object*`
/// pointers — the distinction is purely a calling convention:
/// an owned reference (`lean_obj_arg`) means the recipient must call `lean_dec`
/// exactly once, while a borrowed reference (`b_lean_obj_arg`) means they must
/// not. Calling `lean_inc` does not create a new pointer; it increments `m_rc`
/// on the existing one. The reference count tracks how many `lean_dec` calls
/// remain before the object is freed.
///
/// `LeanOwned` wraps a raw `lean_object*` with RAII semantics. **Every
/// `LeanOwned` value will call `lean_dec` exactly once when dropped.** `Clone`
/// calls `lean_inc` to balance the additional `lean_dec` from the new value's
/// `Drop`, and returns a second `LeanOwned` to the same object.
///
/// Not `Copy` — passing or assigning a `LeanOwned` moves it (transferring the
/// `lean_dec`); use `.clone()` to create a second owned reference via `lean_inc`.
///
/// Corresponds to `lean_obj_arg` (received) and `lean_obj_res` (returned via
/// repr(transparent)).
#[repr(transparent)]
pub struct LeanOwned(*mut include::lean_object);

impl Drop for LeanOwned {
    #[inline]
    fn drop(&mut self) {
        if self.0 as usize & 1 != 1 {
            unsafe { include::lean_dec_ref(self.0) };
        }
    }
}

impl Clone for LeanOwned {
    /// Clone by incrementing the reference count.
    /// Safe for persistent objects (m_rc == 0) — `lean_inc_ref` is a no-op when `m_rc == 0`.
    #[inline]
    fn clone(&self) -> Self {
        if self.0 as usize & 1 != 1 {
            unsafe { include::lean_inc_ref(self.0) };
        }
        LeanOwned(self.0)
    }
}

impl LeanRef for LeanOwned {
    #[inline]
    fn as_raw(&self) -> *mut include::lean_object {
        self.0
    }
}

impl LeanOwned {
    /// Borrow this owned reference. The returned `LeanBorrowed` is
    /// lifetime-bounded to `&self`. No refcount change.
    #[inline]
    pub fn borrow(&self) -> LeanBorrowed<'_> {
        unsafe { LeanBorrowed::from_raw(self.0) }
    }

    /// Wrap a raw pointer, taking ownership of the reference count.
    ///
    /// # Safety
    /// The pointer must be a valid Lean object (or tagged scalar), and the
    /// caller must be transferring one reference count to this wrapper.
    #[inline]
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        Self(ptr)
    }

    /// Consume this wrapper without calling `lean_dec`.
    ///
    /// Use when passing ownership to a Lean C API function that takes
    /// `lean_obj_arg` (which will `lean_dec` internally). Without this,
    /// both the C function and Rust's `Drop` would `lean_dec`, causing a
    /// double-free.
    ///
    /// Not needed for returning values from `extern "C"` FFI functions —
    /// returning a `LeanOwned` directly works because Rust does not call
    /// `Drop` on return values.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0;
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }

    /// Box a `usize` into a tagged scalar pointer.
    #[inline]
    pub fn box_usize(n: usize) -> Self {
        Self(((n << 1) | 1) as *mut _)
    }

    /// Create a `LeanOwned` from a raw tag value for zero-field enum constructors.
    #[inline]
    pub fn from_enum_tag(tag: usize) -> Self {
        Self(tag as *mut _)
    }

    /// Create a Lean `Nat` from a `u64` value.
    #[inline]
    pub fn from_nat_u64(n: u64) -> Self {
        unsafe { Self(include::lean_uint64_to_nat(n)) }
    }

    /// Box a `u32` into a Lean `UInt32` object.
    #[inline]
    pub fn box_u32(n: u32) -> Self {
        Self(unsafe { include::lean_box_uint32(n) })
    }

    /// Box a `u64` into a Lean `UInt64` object.
    #[inline]
    pub fn box_u64(n: u64) -> Self {
        Self(unsafe { include::lean_box_uint64(n) })
    }

    /// Box a `f64` into a Lean `Float` object.
    #[inline]
    pub fn box_f64(v: f64) -> Self {
        Self(unsafe { include::lean_box_float(v) })
    }

    /// Box a `f32` into a Lean `Float32` object.
    #[inline]
    pub fn box_f32(v: f32) -> Self {
        Self(unsafe { include::lean_box_float32(v) })
    }

    /// Box a `usize` into a Lean object via `lean_box_usize` (heap-allocated).
    #[inline]
    pub fn box_usize_obj(v: usize) -> Self {
        Self(unsafe { include::lean_box_usize(v) })
    }
}

// =============================================================================
// LeanBorrowed — Borrowed reference to a Lean object
// =============================================================================

/// Borrowed reference to a Lean object.
///
/// - `Copy + Clone` (trivial bitwise copy, no reference counting).
/// - **No `Drop`** — does not call `lean_dec`.
/// - Lifetime `'a` prevents the reference from outliving its source.
///
/// Corresponds to `b_lean_obj_arg` (borrowed input) and `b_lean_obj_res` (borrowed output).
#[repr(transparent)]
pub struct LeanBorrowed<'a>(*mut include::lean_object, PhantomData<&'a ()>);

impl<'a> Clone for LeanBorrowed<'a> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a> Copy for LeanBorrowed<'a> {}

impl<'a> LeanRef for LeanBorrowed<'a> {
    #[inline]
    fn as_raw(&self) -> *mut include::lean_object {
        self.0
    }
}

impl<'a> LeanBorrowed<'a> {
    /// Wrap a raw pointer as a borrowed reference.
    ///
    /// # Safety
    /// The pointed-to object must remain alive for lifetime `'a`.
    /// The caller must not call `lean_dec` on this pointer.
    #[inline]
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        Self(ptr, PhantomData)
    }

    /// Promote this borrowed reference to an owned reference.
    ///
    /// Calls `lean_inc_ref` to account for the `lean_dec` that the returned
    /// [`LeanOwned`]'s `Drop` will perform.
    /// No-op for tagged scalars (bit 0 set) and persistent objects (`m_rc == 0`).
    #[inline]
    pub fn to_owned_ref(&self) -> LeanOwned {
        let ptr = self.as_raw();
        if ptr as usize & 1 != 1 {
            unsafe { include::lean_inc_ref(ptr) };
        }
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanNat — Nat (scalar or heap mpz)
// =============================================================================
//
// Small Nat: tagged scalar via `lean_box(n)` for n ≤ LEAN_MAX_SMALL_NAT (2^63-1 on 64-bit).
// Big Nat:   heap object with m_tag == LeanMPZ (250), containing a GMP mpz_t.

/// Typed wrapper for a Lean `Nat` (small = tagged scalar, big = heap `mpz_object`).
#[repr(transparent)]
pub struct LeanNat<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanNat<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanNat<R> {}

impl<R: LeanRef> LeanNat<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }
}

impl LeanNat<LeanOwned> {
    /// Wrap an owned `LeanOwned` as a `LeanNat`.
    #[inline]
    pub fn new(obj: LeanOwned) -> Self {
        Self(obj)
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl<'a> LeanNat<LeanBorrowed<'a>> {
    /// Wrap a borrowed reference as a `LeanNat`.
    #[inline]
    pub fn new_borrowed(obj: LeanBorrowed<'a>) -> Self {
        Self(obj)
    }
}

impl From<LeanNat<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanNat<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanBool — Bool (unboxed scalar: false = 0, true = 1)
// =============================================================================
//
// lean.h: Bool.false = lean_box(0), Bool.true = lean_box(1).
// Always a tagged scalar — never heap-allocated.

/// Typed wrapper for a Lean `Bool` (always an unboxed scalar: false = 0, true = 1).
#[repr(transparent)]
pub struct LeanBool<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanBool<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanBool<R> {}

impl<R: LeanRef> LeanBool<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    /// Decode to a Rust `bool`.
    #[inline]
    pub fn to_bool(&self) -> bool {
        self.0.as_enum_tag() != 0
    }
}

impl LeanBool<LeanOwned> {
    /// Wrap an owned `LeanOwned` as a `LeanBool`.
    #[inline]
    pub fn new(obj: LeanOwned) -> Self {
        Self(obj)
    }
}

impl From<LeanBool<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanBool<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanArray — Array α (tag LEAN_TAG_ARRAY)
// =============================================================================
//
// lean.h:
//   typedef struct {
//       lean_object   m_header;
//       size_t        m_size;
//       size_t        m_capacity;
//       lean_object * m_data[];
//   } lean_array_object;

/// Typed wrapper for a Lean `Array α` object (tag `LEAN_TAG_ARRAY`).
#[repr(transparent)]
pub struct LeanArray<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanArray<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanArray<R> {}

impl<R: LeanRef> LeanArray<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    pub fn len(&self) -> usize {
        unsafe { include::lean_array_size(self.0.as_raw()) }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get a borrowed reference to the `i`-th element.
    pub fn get(&self, i: usize) -> LeanBorrowed<'_> {
        LeanBorrowed(
            unsafe { include::lean_array_get_core(self.0.as_raw(), i) },
            PhantomData,
        )
    }

    /// Return a slice over the array elements as borrowed references.
    pub fn data(&self) -> &[LeanBorrowed<'_>] {
        unsafe {
            let cptr = include::lean_array_cptr(self.0.as_raw());
            // Safety: LeanBorrowed is repr(transparent) over *mut lean_object,
            // same layout as the array's element pointers.
            std::slice::from_raw_parts(cptr.cast(), self.len())
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = LeanBorrowed<'_>> + '_ {
        self.data().iter().copied()
    }

    pub fn map<T>(&self, f: impl Fn(LeanBorrowed<'_>) -> T) -> Vec<T> {
        self.iter().map(f).collect()
    }
}

impl LeanArray<LeanOwned> {
    /// Wrap a raw pointer, asserting it is an `Array`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `Array` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } == u32::from(LEAN_TAG_ARRAY));
        Self(LeanOwned(ptr))
    }

    /// Allocate a new array with `size` elements (capacity = size).
    pub fn alloc(size: usize) -> Self {
        let obj = unsafe { include::lean_alloc_array(size, size) };
        Self(LeanOwned(obj))
    }

    /// Build an array from a Lean `List`. Consumes the list. Wraps `lean_array_mk`.
    pub fn from_list(list: LeanList<LeanOwned>) -> Self {
        let result = unsafe { include::lean_array_mk(list.into_raw()) };
        LeanArray(LeanOwned(result))
    }

    /// Convert this array to a Lean `List`. Consumes self. Wraps `lean_array_to_list`.
    pub fn to_list(self) -> LeanList<LeanOwned> {
        let result = unsafe { include::lean_array_to_list(self.into_raw()) };
        LeanList(LeanOwned(result))
    }

    /// Set the `i`-th element of an exclusively owned array. Takes ownership of `val`.
    /// Wraps `lean_array_set_core`, which asserts `lean_is_exclusive`.
    ///
    /// Use this for populating freshly allocated arrays (where `rc == 1` is guaranteed).
    /// For arrays that may be shared, use [`uset`](Self::uset) instead.
    pub fn set(&self, i: usize, val: impl Into<LeanOwned>) {
        let val: LeanOwned = val.into();
        unsafe {
            include::lean_array_set_core(self.0.as_raw(), i, val.into_raw());
        }
    }

    /// Append `val` to the array, returning the (possibly reallocated) array.
    ///
    /// Consumes both `self` and `val` (matching `lean_array_push` semantics).
    pub fn push(self, val: impl Into<LeanOwned>) -> LeanArray<LeanOwned> {
        let val: LeanOwned = val.into();
        let self_ptr = ManuallyDrop::new(self).0.as_raw();
        let val_ptr = val.into_raw();
        let result = unsafe { include::lean_array_push(self_ptr, val_ptr) };
        LeanArray(LeanOwned(result))
    }

    /// Set element `i` to `val`, ensuring exclusive ownership first.
    /// If the array is shared, it is copied before mutation.
    /// Consumes `self` and returns the (possibly new) array. Wraps `lean_array_uset`.
    ///
    /// Use this for modifying arrays that may be shared (e.g. received from Lean).
    /// For populating freshly allocated arrays, [`set`](Self::set) is simpler.
    pub fn uset(self, i: usize, val: impl Into<LeanOwned>) -> LeanArray<LeanOwned> {
        let val: LeanOwned = val.into();
        let result = unsafe { include::lean_array_uset(self.into_raw(), i, val.into_raw()) };
        LeanArray(LeanOwned(result))
    }

    /// Remove the last element, copying the array first if it is shared.
    /// Returns the array unchanged if it is empty.
    pub fn pop(self) -> LeanArray<LeanOwned> {
        let result = unsafe { include::lean_array_pop(self.into_raw()) };
        LeanArray(LeanOwned(result))
    }

    /// Swap elements at indices `i` and `j`, copying the array first if it is shared.
    /// Wraps `lean_array_uswap`.
    pub fn uswap(self, i: usize, j: usize) -> LeanArray<LeanOwned> {
        let result = unsafe { include::lean_array_uswap(self.into_raw(), i, j) };
        LeanArray(LeanOwned(result))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanArray<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanArray<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanByteArray — ByteArray (tag LEAN_TAG_SCALAR_ARRAY)
// =============================================================================
//
// lean.h:
//   typedef struct {
//       lean_object m_header;
//       size_t      m_size;
//       size_t      m_capacity;
//       uint8_t     m_data[];
//   } lean_sarray_object;

/// Typed wrapper for a Lean `ByteArray` object (tag `LEAN_TAG_SCALAR_ARRAY`).
#[repr(transparent)]
pub struct LeanByteArray<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanByteArray<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanByteArray<R> {}

impl<R: LeanRef> LeanByteArray<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    pub fn len(&self) -> usize {
        unsafe { include::lean_sarray_size(self.0.as_raw()) }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the byte contents as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            let cptr = include::lean_sarray_cptr(self.0.as_raw());
            std::slice::from_raw_parts(cptr, self.len())
        }
    }
}

impl LeanByteArray<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a `ByteArray`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `ByteArray` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } == u32::from(LEAN_TAG_SCALAR_ARRAY));
        Self(LeanOwned(ptr))
    }

    /// Allocate a new byte array with `size` bytes (capacity = size).
    pub fn alloc(size: usize) -> Self {
        let obj = unsafe { include::lean_alloc_sarray(1, size, size) };
        Self(LeanOwned(obj))
    }

    /// Allocate a new byte array from a Rust byte slice.
    /// Use this when constructing a Lean `ByteArray` from Rust data.
    /// To duplicate an existing Lean `ByteArray`, use [`copy`](Self::copy).
    pub fn from_bytes(data: &[u8]) -> Self {
        let arr = Self::alloc(data.len());
        unsafe {
            let cptr = include::lean_sarray_cptr(arr.0.as_raw());
            std::ptr::copy_nonoverlapping(data.as_ptr(), cptr, data.len());
        }
        arr
    }

    /// Copy `data` into an exclusively owned byte array and update its size.
    /// Wraps direct pointer writes, which assume `lean_is_exclusive`.
    ///
    /// Use this for populating freshly allocated byte arrays (where `rc == 1` is guaranteed).
    /// For byte arrays that may be shared, use [`uset`](Self::uset) instead.
    ///
    /// # Safety
    /// The caller must ensure the array has sufficient capacity for `data`.
    pub unsafe fn set_data(&self, data: &[u8]) {
        unsafe {
            let obj = self.0.as_raw();
            let cptr = include::lean_sarray_cptr(obj);
            std::ptr::copy_nonoverlapping(data.as_ptr(), cptr, data.len());
            // Update m_size: at offset 8 (after lean_object header)
            *obj.cast::<u8>().add(8).cast::<usize>() = data.len();
        }
    }

    /// Set byte `i` to `val`, ensuring exclusive ownership first.
    /// If the array is shared, it is copied before mutation.
    /// Consumes `self` and returns the (possibly new) array. Wraps `lean_byte_array_uset`.
    ///
    /// For populating freshly allocated byte arrays, [`set_data`](Self::set_data) is simpler.
    pub fn uset(self, i: usize, val: u8) -> LeanByteArray<LeanOwned> {
        let result = unsafe { include::lean_byte_array_uset(self.into_raw(), i, val) };
        LeanByteArray(LeanOwned(result))
    }

    /// Append a byte, reallocating if needed. Copies the array first if it is shared.
    pub fn push(self, val: u8) -> LeanByteArray<LeanOwned> {
        let result = unsafe { include::lean_byte_array_push(self.into_raw(), val) };
        LeanByteArray(LeanOwned(result))
    }

    /// Duplicate this byte array into a new exclusively owned copy.
    /// Consumes self, decrementing the original's refcount.
    /// Use this to get an exclusive copy before mutation. Wraps `lean_copy_byte_array`.
    /// To construct a `ByteArray` from a Rust `&[u8]`, use [`from_bytes`](Self::from_bytes).
    pub fn copy(self) -> Self {
        let result = unsafe { include::lean_copy_byte_array(self.into_raw()) };
        LeanByteArray(LeanOwned(result))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanByteArray<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanByteArray<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanString — String (tag LEAN_TAG_STRING)
// =============================================================================
//
// lean.h:
//   typedef struct {
//       lean_object m_header;
//       size_t      m_size;      // byte length including NUL terminator
//       size_t      m_capacity;
//       size_t      m_length;    // UTF-8 character count
//       char        m_data[];
//   } lean_string_object;

/// Typed wrapper for a Lean `String` object (tag `LEAN_TAG_STRING`).
#[repr(transparent)]
pub struct LeanString<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanString<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanString<R> {}

impl<R: LeanRef> LeanString<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    /// Number of data bytes (excluding the trailing NUL).
    pub fn byte_len(&self) -> usize {
        unsafe { include::lean_string_size(self.0.as_raw()) - 1 }
    }

    /// The length of the string (number of UTF-8 characters, not bytes).
    /// Wraps `lean_string_len`.
    pub fn length(&self) -> usize {
        unsafe { include::lean_string_len(self.0.as_raw()) }
    }

    /// View the string data as a `&str`.
    pub fn as_str(&self) -> &str {
        unsafe {
            let data = include::lean_string_cstr(self.0.as_raw());
            let bytes = std::slice::from_raw_parts(data.cast::<u8>(), self.byte_len());
            std::str::from_utf8_unchecked(bytes)
        }
    }
}

impl<R: LeanRef> std::fmt::Display for LeanString<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl LeanString<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a `String`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `String` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } == u32::from(LEAN_TAG_STRING));
        Self(LeanOwned(ptr))
    }

    /// Create a Lean string from a Rust `&str`.
    pub fn new(s: &str) -> Self {
        let c = safe_cstring(s);
        let obj = unsafe { include::lean_mk_string(c.as_ptr()) };
        Self(LeanOwned(obj))
    }

    /// Create a Lean string from raw bytes via `lean_mk_string_from_bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let obj = unsafe { include::lean_mk_string_from_bytes(bytes.as_ptr().cast(), bytes.len()) };
        Self(LeanOwned(obj))
    }

    /// Push a UTF-32 character, ensuring exclusive ownership first.
    /// Consumes `self` and returns the (possibly new) string. Wraps `lean_string_push`.
    pub fn push(self, c: u32) -> LeanString<LeanOwned> {
        let result = unsafe { include::lean_string_push(self.into_raw(), c) };
        LeanString(LeanOwned(result))
    }

    /// Append another string, ensuring exclusive ownership of `self` first.
    /// Borrows `other`. Consumes `self` and returns the (possibly new) string.
    /// Wraps `lean_string_append`.
    pub fn append(self, other: &LeanString<impl LeanRef>) -> LeanString<LeanOwned> {
        let result = unsafe { include::lean_string_append(self.into_raw(), other.0.as_raw()) };
        LeanString(LeanOwned(result))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanString<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanString<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanCtor — Constructor objects (tag 0–LEAN_MAX_CTOR_TAG)
// =============================================================================
//
// lean.h:
//   typedef struct {
//       lean_object   m_header;   // m_tag = ctor index, m_other = num_objs
//       lean_object * m_objs[];   // object fields, then scalar fields in memory
//   } lean_ctor_object;
//
// Memory layout after m_header:
//   [obj_0, obj_1, ..., obj_{n-1}] [usize_0, ...] [scalar bytes (descending size)]

/// Typed wrapper for a Lean constructor object (tag 0–`LEAN_MAX_CTOR_TAG`).
///
/// # Overall structure
///
/// In C, a constructor is:
/// ```c
/// typedef struct {
///     lean_object   m_header;  // 8 bytes: tag in m_tag, obj count in m_other
///     lean_object * m_objs[];  // flexible array: data area starts here
/// } lean_ctor_object;
/// ```
///
/// `m_objs` is a C flexible array member — it has no compile-time size.
/// `lean_alloc_ctor(tag, num_objs, scalar_sz)` over-allocates so that the
/// region starting at `m_objs` is large enough for `num_objs` pointer-width
/// entries **plus** `scalar_sz` additional bytes for scalar fields. Despite
/// the `lean_object *` element type, the trailing bytes are not pointers —
/// the C API accesses them by casting to `uint8_t*` and indexing by byte
/// offset. `lean_ctor_obj_cptr` returns `&m_objs[0]`; all offsets in the
/// accessor API are relative to this address.
///
/// # Constructor field kinds
///
/// Lean constructor fields are either:
///
/// - **Object fields**: `lean_object*` values — either a pointer to a heap
///   object (lowest bit 0) or a boxed immediate value (lowest bit 1).
///
/// - **Scalar fields**: values stored inline as their unboxed C types
///   (`uint8_t`, `uint32_t`, `double`, etc.) rather than as `lean_object*`.
///   For example, `Bool` is stored as an unboxed `uint8_t` when it appears
///   directly in a `lean_ctor_object`, but
///   as a boxed `lean_object*` in polymorphic contexts like `Array Bool`.
///
/// # Data area layout
///
/// **Important:** Lean reorders fields by kind and size, so the memory layout
/// may differ from the declaration order in Lean source. The data area
/// (accessed via `lean_ctor_obj_cptr`) is laid out as three regions, following
/// the terminology from the
/// [Lean reference manual](https://lean-lang.org/doc/reference/latest/):
///
/// 1. **Object fields** — pointer-width `lean_object*` entries, one per field
///    ("fields of the first kind")
/// 2. **`USize` fields** — pointer-width `usize` entries, one per field
///    ("fields of the second kind"); together with object fields these form
///    a contiguous array of pointer-width entries
/// 3. **Fixed-size scalar fields** — laid out in descending size order
///    (u64/f64, u32/f32, u16, u8/bool)
///
/// For example, a Lean structure declared as:
/// ```text
/// structure Foo where
///   name : String     -- object field
///   flag : Bool       -- scalar (u8)
///   count : UInt64    -- scalar (u64)
///   size : USize      -- usize field
/// ```
/// is reordered and laid out as (assuming 64-bit pointers):
/// ```text
/// lean_ctor_obj_cptr ──>
///   byte 0..7:   name  (lean_object*)   ← m_objs[0], object field
///   byte 8..15:  size  (usize)          ← m_objs[1], USize field
///   byte 16..23: count (u64)            ← scalar field
///   byte 24:     flag  (u8)             ← scalar field
/// ```
/// `lean_alloc_ctor(tag, num_objs=1, scalar_sz=9)` allocates enough space
/// for the header, 1 object pointer, and 9 scalar bytes (8 for the USize
/// entry + 8 for the u64 + 1 for the u8). The scalar fields live past the
/// end of the pointer-width entries; the C API accesses them by casting
/// `lean_ctor_obj_cptr` to `uint8_t*` and indexing by byte offset.
///
/// # Offset conventions
///
/// For fixed-size scalar types (`u8`–`u64`, `f32`, `f64`, `bool`), the
/// `LeanCtor` methods take `offset` as an **absolute byte offset** from
/// `lean_ctor_obj_cptr` — the same convention as the Lean C API functions
/// `lean_ctor_get_uint8`, `lean_ctor_set_uint32`, etc.
///
/// For `usize`, the accessor indexes into the pointer-width entry array
/// described above. The index is relative to the first `USize` entry;
/// the object field count is read from the header and added internally, so
/// `get_usize(0)` reads the first `USize` field.
///
/// The [`LeanCtorScalar`] trait computes these byte offsets automatically
/// from declared field counts, so users don't need to calculate them
/// manually. Reading all fields of `Foo` in C and with the trait:
///
/// ```c
/// // C — all offsets are from lean_ctor_obj_cptr
/// lean_object* name = lean_ctor_get(o, 0);        // array index 0
/// size_t size        = lean_ctor_get_usize(o, 1);  // array index 1 (num_objs + 0)
/// uint64_t count     = lean_ctor_get_uint64(o, 16); // byte offset (1 obj + 1 usize) * 8
/// uint8_t flag       = lean_ctor_get_uint8(o, 24);  // byte offset 16 + sizeof(uint64_t)
/// ```
///
/// ```ignore
/// // Rust — using LeanCtorScalar trait (no manual offsets)
/// let name  = foo.as_ctor().get(0);  // object field
/// let size  = foo.get_usize(0);      // first USize field
/// let count = foo.get_u64(0);        // first u64 scalar
/// let flag  = foo.get_bool(0);       // first bool scalar
/// ```
#[repr(transparent)]
pub struct LeanCtor<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanCtor<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanCtor<R> {}

impl<R: LeanRef> LeanCtor<R> {
    /// Get the raw `*mut lean_object` pointer.
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    pub fn tag(&self) -> u8 {
        self.0.tag()
    }

    /// Get a borrowed reference to the `i`-th object field.
    pub fn get(&self, i: usize) -> LeanBorrowed<'_> {
        LeanBorrowed(
            unsafe { include::lean_ctor_get(self.0.as_raw(), to_u32(i)) },
            PhantomData,
        )
    }

    /// Read `N` object-field pointers using raw pointer math.
    pub fn objs<const N: usize>(&self) -> [LeanBorrowed<'_>; N] {
        let base = unsafe { self.0.as_raw().cast::<*mut include::lean_object>().add(1) };
        std::array::from_fn(|i| LeanBorrowed(unsafe { *base.add(i) }, PhantomData))
    }

    // -------------------------------------------------------------------------
    // Scalar field readers
    // -------------------------------------------------------------------------

    /// Number of object fields, read from the constructor header.
    #[inline]
    pub fn num_objs(&self) -> usize {
        unsafe { include::lean_ctor_num_objs(self.0.as_raw()) as usize }
    }

    /// All scalar accessors below take `offset` as an absolute byte offset
    /// from `lean_ctor_obj_cptr`, matching the Lean C API convention for
    /// `lean_ctor_get_uint8`, `lean_ctor_get_uint32`, etc.
    pub fn get_u8(&self, offset: usize) -> u8 {
        unsafe { include::lean_ctor_get_uint8(self.0.as_raw(), to_u32(offset)) }
    }
    pub fn get_u16(&self, offset: usize) -> u16 {
        unsafe { include::lean_ctor_get_uint16(self.0.as_raw(), to_u32(offset)) }
    }
    pub fn get_u32(&self, offset: usize) -> u32 {
        unsafe { include::lean_ctor_get_uint32(self.0.as_raw(), to_u32(offset)) }
    }
    pub fn get_u64(&self, offset: usize) -> u64 {
        unsafe { include::lean_ctor_get_uint64(self.0.as_raw(), to_u32(offset)) }
    }
    pub fn get_f64(&self, offset: usize) -> f64 {
        unsafe { include::lean_ctor_get_float(self.0.as_raw(), to_u32(offset)) }
    }
    pub fn get_f32(&self, offset: usize) -> f32 {
        unsafe { include::lean_ctor_get_float32(self.0.as_raw(), to_u32(offset)) }
    }
    /// Read a `USize` field. `USize` fields occupy pointer-width entries in
    /// the data area right after object fields. `index` is relative to
    /// the first `USize` entry; the object field count is read from the header
    /// and added internally.
    pub fn get_usize(&self, index: usize) -> usize {
        unsafe { include::lean_ctor_get_usize(self.0.as_raw(), to_u32(self.num_objs() + index)) }
    }
    /// Read a single `Bool` scalar field (`uint8_t`).
    /// Returns `true` if the byte is non-zero.
    pub fn get_bool(&self, offset: usize) -> bool {
        self.get_u8(offset) != 0
    }

    // -------------------------------------------------------------------------
    // Scalar field setters
    // -------------------------------------------------------------------------
    //
    // All setters take `offset` as an absolute byte offset from
    // `lean_ctor_obj_cptr`, matching the Lean C API convention.
    // Available on all R: LeanRef (not restricted to LeanOwned).

    pub fn set_u8(&self, offset: usize, val: u8) {
        unsafe {
            include::lean_ctor_set_uint8(self.0.as_raw(), to_u32(offset), val);
        }
    }
    pub fn set_u16(&self, offset: usize, val: u16) {
        unsafe {
            include::lean_ctor_set_uint16(self.0.as_raw(), to_u32(offset), val);
        }
    }
    pub fn set_u32(&self, offset: usize, val: u32) {
        unsafe {
            include::lean_ctor_set_uint32(self.0.as_raw(), to_u32(offset), val);
        }
    }
    pub fn set_u64(&self, offset: usize, val: u64) {
        unsafe {
            include::lean_ctor_set_uint64(self.0.as_raw(), to_u32(offset), val);
        }
    }
    pub fn set_f64(&self, offset: usize, val: f64) {
        unsafe {
            include::lean_ctor_set_float(self.0.as_raw(), to_u32(offset), val);
        }
    }
    pub fn set_f32(&self, offset: usize, val: f32) {
        unsafe {
            include::lean_ctor_set_float32(self.0.as_raw(), to_u32(offset), val);
        }
    }
    /// Set a `USize` field. `USize` fields occupy pointer-width entries in
    /// the data area right after object fields. `index` is relative to
    /// the first `USize` entry; the object field count is read from the header
    /// and added internally.
    pub fn set_usize(&self, index: usize, val: usize) {
        unsafe {
            include::lean_ctor_set_usize(self.0.as_raw(), to_u32(self.num_objs() + index), val);
        }
    }
    /// Write a single `Bool` scalar field (`uint8_t`, 0 or 1).
    pub fn set_bool(&self, offset: usize, val: bool) {
        self.set_u8(offset, val as u8);
    }
}

impl LeanCtor<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a constructor.
    ///
    /// # Safety
    /// The pointer must be a valid Lean constructor object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } <= u32::from(LEAN_MAX_CTOR_TAG));
        Self(LeanOwned(ptr))
    }

    /// Allocate a new constructor object.
    pub fn alloc(tag: u8, num_objs: usize, scalar_size: usize) -> Self {
        let obj = unsafe {
            include::lean_alloc_ctor(u32::from(tag), to_u32(num_objs), to_u32(scalar_size))
        };
        Self(LeanOwned(obj))
    }

    /// Set the `i`-th object field. Takes ownership of `val`.
    pub fn set(&self, i: usize, val: impl Into<LeanOwned>) {
        let val: LeanOwned = val.into();
        unsafe {
            include::lean_ctor_set(self.0.as_raw(), to_u32(i), val.into_raw());
        }
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanCtor<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanCtor<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanCtorScalar — trait for type-indexed scalar field access
// =============================================================================

/// Trait for type-indexed scalar field access on domain types.
///
/// Implement this on a `lean_domain_type!` wrapper to get `get_*`/`set_*`
/// methods that index by type rather than byte offset. Set the associated
/// constants to match your Lean structure's field counts:
///
/// ```ignore
/// lean_domain_type! { LeanFoo; }
///
/// impl<R: LeanRef> LeanCtorScalar for LeanFoo<R> {
///     const NUM_USIZE: usize = 1;
///     const NUM_U64: usize = 1;
///     fn as_ctor(&self) -> LeanCtor<LeanBorrowed<'_>> { self.as_ctor() }
/// }
///
/// let count = foo.get_u64(0);
/// let flag = foo.get_bool(0);
/// ```
///
/// Within each size tier, Lean preserves **declaration order** — it does not
/// sub-sort by type. Each tier has one getter/setter pair that returns the
/// natural integer type for that width. Use `f64::from_bits()` /
/// `f64::to_bits()` for float fields, and `val != 0` for bool fields.
pub trait LeanCtorScalar {
    /// Number of `USize` fields (pointer-width, after object fields).
    const NUM_USIZE: usize = 0;
    /// Number of 8-byte scalar fields (`UInt64` + `Float`).
    const NUM_8B: usize = 0;
    /// Number of 4-byte scalar fields (`UInt32` + `Float32`).
    const NUM_4B: usize = 0;
    /// Number of 2-byte scalar fields (`UInt16`).
    const NUM_2B: usize = 0;
    // 1-byte count not needed — it's the last tier.

    /// Access the underlying constructor. Already generated by `lean_domain_type!`.
    fn as_ctor(&self) -> LeanCtor<LeanBorrowed<'_>>;

    /// Byte offset from `lean_ctor_obj_cptr` where fixed-size scalar fields begin.
    /// Reads the object field count from the header and adds `NUM_USIZE`.
    fn scalar_base(&self) -> usize {
        (self.as_ctor().num_objs() + Self::NUM_USIZE) * size_of::<usize>()
    }

    // -- USize fields --

    fn get_usize(&self, i: usize) -> usize {
        self.as_ctor().get_usize(i)
    }
    fn set_usize(&self, i: usize, val: usize) {
        self.as_ctor().set_usize(i, val)
    }

    // -- 8-byte tier (UInt64 / Float) --

    fn get_64(&self, i: usize) -> u64 {
        self.as_ctor().get_u64(self.scalar_base() + i * 8)
    }
    fn set_64(&self, i: usize, val: u64) {
        self.as_ctor().set_u64(self.scalar_base() + i * 8, val)
    }

    // -- 4-byte tier (UInt32 / Float32) --

    fn get_32(&self, i: usize) -> u32 {
        self.as_ctor()
            .get_u32(self.scalar_base() + Self::NUM_8B * 8 + i * 4)
    }
    fn set_32(&self, i: usize, val: u32) {
        self.as_ctor()
            .set_u32(self.scalar_base() + Self::NUM_8B * 8 + i * 4, val)
    }

    // -- 2-byte tier (UInt16) --

    fn get_16(&self, i: usize) -> u16 {
        self.as_ctor()
            .get_u16(self.scalar_base() + Self::NUM_8B * 8 + Self::NUM_4B * 4 + i * 2)
    }
    fn set_16(&self, i: usize, val: u16) {
        self.as_ctor().set_u16(
            self.scalar_base() + Self::NUM_8B * 8 + Self::NUM_4B * 4 + i * 2,
            val,
        )
    }

    // -- 1-byte tier (UInt8 / Bool) --

    fn get_8(&self, i: usize) -> u8 {
        self.as_ctor()
            .get_u8(self.scalar_base() + Self::NUM_8B * 8 + Self::NUM_4B * 4 + Self::NUM_2B * 2 + i)
    }
    fn set_8(&self, i: usize, val: u8) {
        self.as_ctor().set_u8(
            self.scalar_base() + Self::NUM_8B * 8 + Self::NUM_4B * 4 + Self::NUM_2B * 2 + i,
            val,
        )
    }
}

// =============================================================================
// LeanExternal<T> — External objects (tag LEAN_TAG_EXTERNAL)
// =============================================================================
//
// lean.h:
//   typedef struct {
//       lean_external_finalize_proc m_finalize;
//       lean_external_foreach_proc  m_foreach;
//   } lean_external_class;
//
//   typedef struct {
//       lean_object           m_header;
//       lean_external_class * m_class;
//       void *                m_data;
//   } lean_external_object;

/// Typed wrapper for a Lean external object (tag `LEAN_TAG_EXTERNAL`) holding a `T`.
#[repr(transparent)]
pub struct LeanExternal<T, R: LeanRef>(R, PhantomData<T>);

impl<T, R: LeanRef> Clone for LeanExternal<T, R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<T, R: LeanRef + Copy> Copy for LeanExternal<T, R> {}

impl<T, R: LeanRef> LeanExternal<T, R> {
    /// Get a reference to the wrapped data.
    pub fn get(&self) -> &T {
        unsafe { &*include::lean_get_external_data(self.0.as_raw()).cast::<T>() }
    }
}

impl<T> LeanExternal<T, LeanOwned> {
    /// Wrap a raw pointer, asserting it is an external object.
    ///
    /// # Safety
    /// The pointer must be a valid Lean external object whose data pointer
    /// points to a valid `T`.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } == u32::from(LEAN_TAG_EXTERNAL));
        Self(LeanOwned(ptr), PhantomData)
    }

    /// Allocate a new external object holding `data`.
    pub fn alloc(class: &ExternalClass, data: T) -> Self {
        let data_ptr = Box::into_raw(Box::new(data));
        let obj = unsafe { include::lean_alloc_external(class.0, data_ptr.cast()) };
        Self(LeanOwned(obj), PhantomData)
    }

    /// Get a mutable reference to the wrapped data if the external object is
    /// exclusively owned (`m_rc == 1`, single-threaded mode).
    ///
    /// Returns `None` if the object is shared or multi-threaded. The `&mut self`
    /// requirement ensures unique Rust access, while the `is_exclusive` check
    /// ensures unique Lean access.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if unsafe { include::lean_is_exclusive(self.0.as_raw()) } {
            Some(unsafe { &mut *include::lean_get_external_data(self.0.as_raw()).cast::<T>() })
        } else {
            None
        }
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl<'a, T> LeanExternal<T, LeanBorrowed<'a>> {
    /// Wrap a raw pointer as a borrowed reference to an external object.
    ///
    /// # Safety
    /// The pointer must be a valid Lean external object whose data pointer
    /// points to a valid `T`, and the object must outlive `'a`.
    pub unsafe fn from_raw_borrowed(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(unsafe { include::lean_obj_tag(ptr) } == u32::from(LEAN_TAG_EXTERNAL));
        Self(unsafe { LeanBorrowed::from_raw(ptr) }, PhantomData)
    }
}

impl<T> From<LeanExternal<T, LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanExternal<T, LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// ExternalClass — Registered external class
// =============================================================================

/// A registered Lean external class (wraps `lean_external_class*`).
///
/// A "class" is a pair of function pointers (finalizer + foreach) shared by
/// all external objects of the same Rust type. Created once via
/// [`register`](Self::register) or [`register_with_drop`](Self::register_with_drop)
/// and stored in a `static`.
pub struct ExternalClass(*mut include::lean_external_class);

// Safety: the class pointer is initialized once and read-only thereafter.
unsafe impl Send for ExternalClass {}
unsafe impl Sync for ExternalClass {}

impl ExternalClass {
    /// Register a new external class with explicit finalizer and foreach callbacks.
    ///
    /// # Safety
    /// The `finalizer` callback must correctly free the external data, and
    /// `foreach` must visit any `lean_object*` pointers held by the data so that
    /// `lean_mark_persistent` and `lean_mark_mt` can traverse the full object
    /// graph. Only called during persistent/MT marking, not during normal
    /// deallocation.
    pub unsafe fn register(
        finalizer: include::lean_external_finalize_proc,
        foreach: include::lean_external_foreach_proc,
    ) -> Self {
        Self(unsafe { include::lean_register_external_class(finalizer, foreach) })
    }

    /// Register a new external class that uses `Drop` to finalize `T`
    /// and provides a no-op foreach (suitable when `T` holds no `lean_object*`
    /// pointers).
    pub fn register_with_drop<T>() -> Self {
        unsafe extern "C" fn drop_finalizer<T>(ptr: *mut std::ffi::c_void) {
            if !ptr.is_null() {
                drop(unsafe { Box::from_raw(ptr.cast::<T>()) });
            }
        }
        unsafe { Self::register(Some(drop_finalizer::<T>), Some(crate::noop_foreach)) }
    }
}

// =============================================================================
// LeanList — List α
// =============================================================================
//
// Constructor-based inductive (no special lean.h struct):
//   List.nil  = lean_box(0)                          (tagged scalar)
//   List.cons = lean_ctor_object, tag 1, 2 obj fields (head, tail)

/// Typed wrapper for a Lean `List α` (nil = scalar `lean_box(0)`, cons = ctor tag 1).
#[repr(transparent)]
pub struct LeanList<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanList<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanList<R> {}

impl<R: LeanRef> LeanList<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    pub fn is_nil(&self) -> bool {
        self.0.is_scalar()
    }

    pub fn iter(&self) -> LeanListIter<'_> {
        LeanListIter(LeanBorrowed(self.0.as_raw(), PhantomData))
    }

    pub fn collect<T>(&self, f: impl Fn(LeanBorrowed<'_>) -> T) -> Vec<T> {
        self.iter().map(f).collect()
    }
}

impl LeanList<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a valid `List`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `List` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 == 1 || unsafe { include::lean_obj_tag(ptr) } == 1);
        Self(LeanOwned(ptr))
    }

    /// The empty list.
    pub fn nil() -> Self {
        Self(LeanOwned::box_usize(0))
    }

    /// Prepend `head` to `tail`.
    pub fn cons(head: impl Into<LeanOwned>, tail: LeanList<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(1, 2, 0);
        ctor.set(0, head);
        ctor.set(1, tail);
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl<T: Into<LeanOwned>> FromIterator<T> for LeanList<LeanOwned> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let items: Vec<LeanOwned> = iter.into_iter().map(Into::into).collect();
        let mut list = Self::nil();
        for item in items.into_iter().rev() {
            list = Self::cons(item, list);
        }
        list
    }
}

impl<'a> IntoIterator for LeanList<LeanBorrowed<'a>> {
    type Item = LeanBorrowed<'a>;
    type IntoIter = LeanListIter<'a>;

    /// Iterate elements with the original borrow lifetime `'a`.
    ///
    /// Unlike [`iter()`](LeanList::iter) (which ties the output lifetime to
    /// `&self`), this preserves the lifetime of the underlying Lean objects.
    /// Use this when the list is a local `Copy` value and the elements need to
    /// outlive the list binding.
    #[inline]
    fn into_iter(self) -> LeanListIter<'a> {
        LeanListIter(self.0)
    }
}

impl<'a> LeanList<LeanBorrowed<'a>> {
    /// Collect elements into a `Vec` with the original borrow lifetime.
    pub fn to_vec(self) -> Vec<LeanBorrowed<'a>> {
        self.into_iter().collect()
    }
}

/// Iterator over the elements of a `LeanList`.
pub struct LeanListIter<'a>(LeanBorrowed<'a>);

impl<'a> Iterator for LeanListIter<'a> {
    type Item = LeanBorrowed<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_scalar() {
            return None;
        }
        let ptr = self.0.as_raw();
        let head = unsafe { include::lean_ctor_get(ptr, 0) };
        let tail = unsafe { include::lean_ctor_get(ptr, 1) };
        self.0 = LeanBorrowed(tail, PhantomData);
        Some(LeanBorrowed(head, PhantomData))
    }
}

impl From<LeanList<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanList<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanOption — Option α
// =============================================================================
//
// Constructor-based inductive (no special lean.h struct):
//   Option.none = lean_box(0)                              (tagged scalar)
//   Option.some = lean_ctor_object, tag 1, 1 obj field (value)

/// Typed wrapper for a Lean `Option α` (none = scalar, some = ctor tag 1).
#[repr(transparent)]
pub struct LeanOption<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanOption<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanOption<R> {}

impl<R: LeanRef> LeanOption<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }
    #[inline]
    pub fn as_ctor(&self) -> LeanCtor<LeanBorrowed<'_>> {
        unsafe { LeanBorrowed::from_raw(self.0.as_raw()) }.as_ctor()
    }

    pub fn is_none(&self) -> bool {
        self.0.is_scalar()
    }

    pub fn is_some(&self) -> bool {
        !self.is_none()
    }

    pub fn to_option(&self) -> Option<LeanBorrowed<'_>> {
        if self.is_none() {
            None
        } else {
            let val = unsafe { include::lean_ctor_get(self.0.as_raw(), 0) };
            Some(LeanBorrowed(val, PhantomData))
        }
    }
}

impl LeanOption<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a valid `Option`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `Option` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 == 1 || unsafe { include::lean_obj_tag(ptr) } == 1);
        Self(LeanOwned(ptr))
    }

    pub fn none() -> Self {
        Self(LeanOwned::box_usize(0))
    }

    pub fn some(val: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(1, 1, 0);
        ctor.set(0, val);
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanOption<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanOption<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanExcept — Except ε α
// =============================================================================
//
// Constructor-based inductive (no special lean.h struct):
//   Except.error = lean_ctor_object, tag 0, 1 obj field (error value)
//   Except.ok    = lean_ctor_object, tag 1, 1 obj field (ok value)

/// Typed wrapper for a Lean `Except ε α` (error = ctor tag 0, ok = ctor tag 1).
#[repr(transparent)]
pub struct LeanExcept<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanExcept<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanExcept<R> {}

impl<R: LeanRef> LeanExcept<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }
    #[inline]
    pub fn as_ctor(&self) -> LeanCtor<LeanBorrowed<'_>> {
        unsafe { LeanBorrowed::from_raw(self.0.as_raw()) }.as_ctor()
    }

    pub fn is_ok(&self) -> bool {
        self.0.tag() == 1
    }

    pub fn is_error(&self) -> bool {
        self.0.tag() == 0
    }

    pub fn into_result(&self) -> Result<LeanBorrowed<'_>, LeanBorrowed<'_>> {
        let val = unsafe { include::lean_ctor_get(self.0.as_raw(), 0) };
        if self.is_ok() {
            Ok(LeanBorrowed(val, PhantomData))
        } else {
            Err(LeanBorrowed(val, PhantomData))
        }
    }
}

impl LeanExcept<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a valid `Except`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `Except` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        test_assert!(ptr as usize & 1 != 1);
        test_assert!(
            unsafe { include::lean_obj_tag(ptr) } == 0
                || unsafe { include::lean_obj_tag(ptr) } == 1
        );
        Self(LeanOwned(ptr))
    }

    /// Build `Except.ok val`.
    pub fn ok(val: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(1, 1, 0);
        ctor.set(0, val);
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Build `Except.error msg`.
    pub fn error(msg: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(0, 1, 0);
        ctor.set(0, msg);
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Build `Except.error (String.mk msg)` from a Rust string.
    pub fn error_string(msg: &str) -> Self {
        Self::error(LeanString::new(msg))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanExcept<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanExcept<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanIOResult — EStateM.Result (BaseIO.Result)
// =============================================================================
//
// Constructor-based inductive (no special lean.h struct):
//   EStateM.Result.ok    = lean_ctor_object, tag 0, 2 obj fields (value, state)
//   EStateM.Result.error = lean_ctor_object, tag 1, 2 obj fields (error, state)
//
// lean.h accessors:
//   lean_io_result_is_ok(r)        → lean_ptr_tag(r) == 0
//   lean_io_result_get_value(r)    → lean_ctor_get(r, 0)
//   lean_io_result_get_error(r)    → lean_ctor_get(r, 0)

/// Typed wrapper for a Lean `BaseIO.Result α` (`EStateM.Result`).
/// ok = ctor tag 0 (value, world), error = ctor tag 1 (error, world).
#[repr(transparent)]
pub struct LeanIOResult<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanIOResult<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanIOResult<R> {}

impl<R: LeanRef> LeanIOResult<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }
    #[inline]
    pub fn as_ctor(&self) -> LeanCtor<LeanBorrowed<'_>> {
        unsafe { LeanBorrowed::from_raw(self.0.as_raw()) }.as_ctor()
    }
}

impl LeanIOResult<LeanOwned> {
    /// Build a successful IO result (tag 0, fields: [val, box(0)]).
    pub fn ok(val: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(0, 2, 0);
        ctor.set(0, val);
        ctor.set(1, LeanOwned::box_usize(0)); // world token
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Build an IO error result (tag 1, fields: [err, box(0)]).
    pub fn error(err: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(1, 2, 0);
        ctor.set(0, err);
        ctor.set(1, LeanOwned::box_usize(0)); // world token
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Build an IO error from a Rust string via `IO.Error.userError` (tag 7, 1 field).
    pub fn error_string(msg: &str) -> Self {
        let user_error = LeanCtor::alloc(IO_ERROR_USER_ERROR_TAG, 1, 0);
        user_error.set(0, LeanString::new(msg));
        Self::error(user_error)
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanIOResult<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanIOResult<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanProd — Prod α β (pair)
// =============================================================================
//
// Constructor-based inductive (no special lean.h struct):
//   Prod.mk = lean_ctor_object, tag 0, 2 obj fields (fst, snd)

/// Typed wrapper for a Lean `Prod α β` (ctor tag 0, 2 object fields).
#[repr(transparent)]
pub struct LeanProd<R: LeanRef>(R);

impl<R: LeanRef> Clone for LeanProd<R> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: LeanRef + Copy> Copy for LeanProd<R> {}

impl<R: LeanRef> LeanProd<R> {
    #[inline]
    pub fn inner(&self) -> &R {
        &self.0
    }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    /// Get a borrowed reference to the first element.
    pub fn fst(&self) -> LeanBorrowed<'_> {
        LeanBorrowed(
            unsafe { include::lean_ctor_get(self.0.as_raw(), 0) },
            PhantomData,
        )
    }

    /// Get a borrowed reference to the second element.
    pub fn snd(&self) -> LeanBorrowed<'_> {
        LeanBorrowed(
            unsafe { include::lean_ctor_get(self.0.as_raw(), 1) },
            PhantomData,
        )
    }
}

impl LeanProd<LeanOwned> {
    /// Build a pair `(fst, snd)`.
    pub fn new(fst: impl Into<LeanOwned>, snd: impl Into<LeanOwned>) -> Self {
        let ctor = LeanCtor::alloc(0, 2, 0);
        ctor.set(0, fst);
        ctor.set(1, snd);
        Self(LeanOwned(ctor.into_raw()))
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanProd<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanProd<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// From<primitive> for LeanOwned
// =============================================================================

impl From<u32> for LeanOwned {
    #[inline]
    fn from(x: u32) -> Self {
        Self::box_u32(x)
    }
}

impl From<f64> for LeanOwned {
    #[inline]
    fn from(x: f64) -> Self {
        Self::box_f64(x)
    }
}

impl From<f32> for LeanOwned {
    #[inline]
    fn from(x: f32) -> Self {
        Self::box_f32(x)
    }
}

// =============================================================================
// Convenience: as_ctor / as_string / as_array / as_list / as_byte_array
// =============================================================================

/// Helper methods for interpreting a borrowed reference as a specific domain type.
impl<'a> LeanBorrowed<'a> {
    /// Interpret as a constructor object.
    #[inline]
    pub fn as_ctor(self) -> LeanCtor<LeanBorrowed<'a>> {
        test_assert!(!self.is_scalar() && self.tag() <= LEAN_MAX_CTOR_TAG);
        LeanCtor(self)
    }

    /// Interpret as a `String` object.
    #[inline]
    pub fn as_string(self) -> LeanString<LeanBorrowed<'a>> {
        test_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_STRING);
        LeanString(self)
    }

    /// Interpret as an `Array` object.
    #[inline]
    pub fn as_array(self) -> LeanArray<LeanBorrowed<'a>> {
        test_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_ARRAY);
        LeanArray(self)
    }

    /// Interpret as a `List`.
    #[inline]
    pub fn as_list(self) -> LeanList<LeanBorrowed<'a>> {
        test_assert!(self.is_scalar() || self.tag() == 1);
        LeanList(self)
    }

    /// Interpret as a `ByteArray` object.
    #[inline]
    pub fn as_byte_array(self) -> LeanByteArray<LeanBorrowed<'a>> {
        test_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_SCALAR_ARRAY);
        LeanByteArray(self)
    }
}

// =============================================================================
// LeanShared — Thread-safe owned reference to a Lean object
// =============================================================================

/// Thread-safe owned reference to a Lean object, with atomic refcounting.
///
/// Lean objects track refcounts in `m_rc`:
/// - `m_rc > 0` → single-threaded (ST): non-atomic inc/dec
/// - `m_rc < 0` → multi-threaded (MT): atomic inc/dec (negative magnitude is the count)
/// - `m_rc == 0` → persistent: inc/dec are no-ops
///
/// [`LeanShared::new`] calls `lean_mark_mt` which recursively transitions
/// the entire reachable object graph from ST to MT by negating `m_rc`.
/// After marking, `lean_inc_ref` uses `atomic_fetch_sub` (subtracting makes
/// the count more negative) and `lean_dec_ref_cold` uses `atomic_fetch_add`
/// (adding towards zero; freeing when previous value was -1).
///
/// This means [`LeanOwned`]'s existing `Clone` (`lean_inc_ref`) and `Drop`
/// (`lean_dec_ref`) are automatically thread-safe on MT-marked objects —
/// no custom refcount logic is needed in `LeanShared`.
///
/// Calling `lean_mark_mt` on an already-MT object is a single branch
/// (`lean_is_st` check) with no traversal, so it's safe to mark
/// sub-objects of an already-marked parent.
#[repr(transparent)]
pub struct LeanShared(LeanOwned);

// SAFETY: lean_mark_mt transitions the entire reachable object graph to
// multi-threaded mode (m_rc negated). After marking:
// - lean_inc_ref: atomic_fetch_sub(m_rc, 1) — makes count more negative
// - lean_dec_ref_cold: atomic_fetch_add(m_rc, 1) — frees when previous == -1
// This makes Clone (inc_ref) and Drop (dec_ref) thread-safe.
unsafe impl Send for LeanShared {}
unsafe impl Sync for LeanShared {}

impl LeanShared {
    /// Mark the object's entire reachable graph as MT and wrap as a shared reference.
    ///
    /// Persistent objects (`m_rc == 0`) and scalars are unaffected.
    /// After this call, all refcount operations on the object graph use
    /// atomic instructions.
    #[inline]
    pub fn new(owned: LeanOwned) -> Self {
        if !owned.is_scalar() && !owned.is_persistent() {
            unsafe {
                include::lean_mark_mt(owned.as_raw());
            }
        }
        Self(owned)
    }

    /// Borrow this object. The returned reference is lifetime-bounded
    /// to `&self` and is **not** `Send`.
    #[inline]
    pub fn borrow(&self) -> LeanBorrowed<'_> {
        unsafe { LeanBorrowed::from_raw(self.0.as_raw()) }
    }

    /// Get the raw pointer, e.g. for pointer-identity caching across threads.
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }

    /// Consume, returning the inner [`LeanOwned`] (still MT-marked).
    #[inline]
    pub fn into_owned(self) -> LeanOwned {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        unsafe { LeanOwned::from_raw(ptr) }
    }
}

impl Clone for LeanShared {
    #[inline]
    fn clone(&self) -> Self {
        // lean_inc_ref uses atomic ops for MT objects (m_rc < 0).
        Self(self.0.clone())
    }
}

// No custom Drop needed: LeanOwned's Drop calls lean_dec_ref, which handles
// MT objects via lean_dec_ref_cold (atomic decrement + deallocation).

impl LeanRef for LeanShared {
    #[inline]
    fn as_raw(&self) -> *mut include::lean_object {
        self.0.as_raw()
    }
}
