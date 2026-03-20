//! Ownership-aware wrappers for Lean FFI object pointers.
//!
//! The two core pointer types are:
//! - [`LeanOwned`]: Owned reference. `Drop` calls `lean_dec`, `Clone` calls `lean_inc`. Not `Copy`.
//! - [`LeanBorrowed`]: Borrowed reference. `Copy`, no `Drop`, lifetime-bounded.
//!
//! Domain types like [`LeanArray`], [`LeanCtor`], etc. are generic over `R: LeanRef`,
//! inheriting ownership semantics from the inner pointer type.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use crate::include;
use crate::safe_cstring;

// Tag constants from lean.h
const LEAN_MAX_CTOR_TAG: u8 = 243;
const LEAN_TAG_ARRAY: u8 = 246;
const LEAN_TAG_SCALAR_ARRAY: u8 = 248;
const LEAN_TAG_STRING: u8 = 249;
const LEAN_TAG_EXTERNAL: u8 = 254;

/// Constructor tag for `IO.Error.userError`.
const IO_ERROR_USER_ERROR_TAG: u8 = 7;

// =============================================================================
// LeanRef trait — shared interface for owned and borrowed pointers
// =============================================================================

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

    /// True if this is a persistent object (m_rc == 0). Persistent objects live
    /// for the program's lifetime and must not have their reference count modified.
    /// Objects in compact regions and values created at initialization time are persistent.
    #[inline]
    fn is_persistent(&self) -> bool {
        !self.is_scalar() && unsafe { include::lean_is_persistent(self.as_raw()) }
    }

    /// Produce an owned copy by incrementing the reference count.
    /// Safe for persistent objects (m_rc == 0) — `lean_inc_ref` is a no-op when `m_rc == 0`.
    #[inline]
    fn to_owned_ref(&self) -> LeanOwned {
        let ptr = self.as_raw();
        if ptr as usize & 1 != 1 {
            unsafe { include::lean_inc_ref(ptr) };
        }
        LeanOwned(ptr)
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
// LeanOwned — Owned Lean object pointer (RAII)
// =============================================================================

/// Owned reference to a Lean object.
///
/// - `Drop` calls `lean_dec` (with scalar check).
/// - `Clone` calls `lean_inc`.
/// - **Not `Copy`** — ownership is linear.
///
/// Corresponds to `lean_obj_arg` (received) and `lean_obj_res` (returned via repr(transparent)).
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
    /// Use when transferring ownership back to Lean (returning `lean_obj_res`).
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0;
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
// LeanBorrowed — Borrowed Lean object pointer
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
}

// =============================================================================
// LeanNat — Nat (scalar or heap mpz)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }
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
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanBool — Bool (unboxed scalar: false = 0, true = 1)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

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
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanArray — Array α (tag LEAN_TAG_ARRAY)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

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
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 == LEAN_TAG_ARRAY);
            Self(LeanOwned(ptr))
        }
    }

    /// Allocate a new array with `size` elements (capacity = size).
    pub fn alloc(size: usize) -> Self {
        let obj = unsafe { include::lean_alloc_array(size, size) };
        Self(LeanOwned(obj))
    }

    /// Set the `i`-th element. Takes ownership of `val`.
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

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanArray<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanArray<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanByteArray — ByteArray (tag LEAN_TAG_SCALAR_ARRAY)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

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
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 == LEAN_TAG_SCALAR_ARRAY);
            Self(LeanOwned(ptr))
        }
    }

    /// Allocate a new byte array with `size` bytes (capacity = size).
    pub fn alloc(size: usize) -> Self {
        let obj = unsafe { include::lean_alloc_sarray(1, size, size) };
        Self(LeanOwned(obj))
    }

    /// Allocate a new byte array and copy `data` into it.
    pub fn from_bytes(data: &[u8]) -> Self {
        let arr = Self::alloc(data.len());
        unsafe {
            let cptr = include::lean_sarray_cptr(arr.0.as_raw());
            std::ptr::copy_nonoverlapping(data.as_ptr(), cptr, data.len());
        }
        arr
    }

    /// Copy `data` into the byte array and update its size.
    ///
    /// # Safety
    /// The caller must ensure the array has sufficient capacity for `data`.
    pub unsafe fn set_data(&self, data: &[u8]) {
        unsafe {
            let obj = self.0.as_raw();
            let cptr = include::lean_sarray_cptr(obj);
            std::ptr::copy_nonoverlapping(data.as_ptr(), cptr, data.len());
            // Update m_size: at offset 8 (after lean_object header)
            *(obj as *mut u8).add(8).cast::<usize>() = data.len();
        }
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanByteArray<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanByteArray<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanString — String (tag LEAN_TAG_STRING)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

    /// Number of data bytes (excluding the trailing NUL).
    pub fn byte_len(&self) -> usize {
        unsafe { include::lean_string_size(self.0.as_raw()) - 1 }
    }
}

impl<R: LeanRef> std::fmt::Display for LeanString<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            let obj = self.0.as_raw();
            let len = include::lean_string_size(obj) - 1; // m_size includes NUL
            let data = include::lean_string_cstr(obj);
            let bytes = std::slice::from_raw_parts(data.cast::<u8>(), len);
            let s = std::str::from_utf8_unchecked(bytes);
            f.write_str(s)
        }
    }
}

impl LeanString<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a `String`.
    ///
    /// # Safety
    /// The pointer must be a valid Lean `String` object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 == LEAN_TAG_STRING);
            Self(LeanOwned(ptr))
        }
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

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanString<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanString<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanCtor — Constructor objects (tag 0–LEAN_MAX_CTOR_TAG)
// =============================================================================

/// Typed wrapper for a Lean constructor object (tag 0–`LEAN_MAX_CTOR_TAG`).
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
        #[allow(clippy::cast_possible_truncation)]
        LeanBorrowed(
            unsafe { include::lean_ctor_get(self.0.as_raw(), i as u32) },
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
    //
    // `num_objs` is the number of non-scalar fields (object + usize) preceding
    // the scalar area. `offset` is a byte offset within the scalar area.
    // For `get_usize`, `slot` is a slot index (not byte offset).

    /// Compute the absolute byte offset for a scalar field.
    #[allow(clippy::cast_possible_truncation)]
    #[inline]
    fn scalar_offset(num_objs: usize, offset: usize) -> u32 {
        (num_objs * 8 + offset) as u32
    }

    pub fn get_u8(&self, num_objs: usize, offset: usize) -> u8 {
        unsafe { include::lean_ctor_get_uint8(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    pub fn get_u16(&self, num_objs: usize, offset: usize) -> u16 {
        unsafe { include::lean_ctor_get_uint16(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    pub fn get_u32(&self, num_objs: usize, offset: usize) -> u32 {
        unsafe { include::lean_ctor_get_uint32(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    pub fn get_u64(&self, num_objs: usize, offset: usize) -> u64 {
        unsafe { include::lean_ctor_get_uint64(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    pub fn get_f64(&self, num_objs: usize, offset: usize) -> f64 {
        unsafe { include::lean_ctor_get_float(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    pub fn get_f32(&self, num_objs: usize, offset: usize) -> f32 {
        unsafe { include::lean_ctor_get_float32(self.0.as_raw(), Self::scalar_offset(num_objs, offset)) }
    }
    /// Read a `usize` at slot `slot` past `num_objs` object fields.
    /// Uses a **slot index** (not byte offset).
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_usize(&self, num_objs: usize, slot: usize) -> usize {
        unsafe { include::lean_ctor_get_usize(self.0.as_raw(), (num_objs + slot) as u32) }
    }
    pub fn get_bool(&self, num_objs: usize, offset: usize) -> bool {
        self.get_u8(num_objs, offset) != 0
    }
}

impl LeanCtor<LeanOwned> {
    /// Wrap a raw pointer, asserting it is a constructor.
    ///
    /// # Safety
    /// The pointer must be a valid Lean constructor object.
    pub unsafe fn from_raw(ptr: *mut include::lean_object) -> Self {
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 <= LEAN_MAX_CTOR_TAG);
            Self(LeanOwned(ptr))
        }
    }

    /// Allocate a new constructor object.
    pub fn alloc(tag: u8, num_objs: usize, scalar_size: usize) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let obj =
            unsafe { include::lean_alloc_ctor(tag as u32, num_objs as u32, scalar_size as u32) };
        Self(LeanOwned(obj))
    }

    /// Set the `i`-th object field. Takes ownership of `val`.
    pub fn set(&self, i: usize, val: impl Into<LeanOwned>) {
        let val: LeanOwned = val.into();
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            include::lean_ctor_set(self.0.as_raw(), i as u32, val.into_raw());
        }
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
    }

    // -------------------------------------------------------------------------
    // Scalar field setters (owned only — mutation)
    // -------------------------------------------------------------------------

    pub fn set_u8(&self, num_objs: usize, offset: usize, val: u8) {
        unsafe { include::lean_ctor_set_uint8(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    pub fn set_u16(&self, num_objs: usize, offset: usize, val: u16) {
        unsafe { include::lean_ctor_set_uint16(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    pub fn set_u32(&self, num_objs: usize, offset: usize, val: u32) {
        unsafe { include::lean_ctor_set_uint32(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    pub fn set_u64(&self, num_objs: usize, offset: usize, val: u64) {
        unsafe { include::lean_ctor_set_uint64(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    pub fn set_f64(&self, num_objs: usize, offset: usize, val: f64) {
        unsafe { include::lean_ctor_set_float(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    pub fn set_f32(&self, num_objs: usize, offset: usize, val: f32) {
        unsafe { include::lean_ctor_set_float32(self.0.as_raw(), Self::scalar_offset(num_objs, offset), val); }
    }
    /// Set a `usize` at slot `slot` past `num_objs` object fields.
    /// Uses a **slot index** (not byte offset).
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_usize(&self, num_objs: usize, slot: usize, val: usize) {
        unsafe { include::lean_ctor_set_usize(self.0.as_raw(), (num_objs + slot) as u32, val); }
    }
    pub fn set_bool(&self, num_objs: usize, offset: usize, val: bool) {
        self.set_u8(num_objs, offset, val as u8);
    }
}

impl From<LeanCtor<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanCtor<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanExternal<T> — External objects (tag LEAN_TAG_EXTERNAL)
// =============================================================================

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
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 == LEAN_TAG_EXTERNAL);
            Self(LeanOwned(ptr), PhantomData)
        }
    }

    /// Allocate a new external object holding `data`.
    pub fn alloc(class: &ExternalClass, data: T) -> Self {
        let data_ptr = Box::into_raw(Box::new(data));
        let obj = unsafe { include::lean_alloc_external(class.0, data_ptr.cast()) };
        Self(LeanOwned(obj), PhantomData)
    }

    /// Consume without calling `lean_dec`.
    #[inline]
    pub fn into_raw(self) -> *mut include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
    }
}

impl<'a, T> LeanExternal<T, LeanBorrowed<'a>> {
    /// Wrap a raw pointer as a borrowed external object.
    ///
    /// # Safety
    /// The pointer must be a valid Lean external object whose data pointer
    /// points to a valid `T`, and the object must outlive `'a`.
    pub unsafe fn from_raw_borrowed(ptr: *mut include::lean_object) -> Self {
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(include::lean_obj_tag(ptr) as u8 == LEAN_TAG_EXTERNAL);
            Self(LeanBorrowed::from_raw(ptr), PhantomData)
        }
    }
}

impl<T> From<LeanExternal<T, LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanExternal<T, LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// ExternalClass — Registered external class
// =============================================================================

/// A registered Lean external class (wraps `lean_external_class*`).
pub struct ExternalClass(*mut include::lean_external_class);

// Safety: the class pointer is initialized once and read-only thereafter.
unsafe impl Send for ExternalClass {}
unsafe impl Sync for ExternalClass {}

impl ExternalClass {
    /// Register a new external class with explicit finalizer and foreach callbacks.
    ///
    /// # Safety
    /// The `finalizer` callback must correctly free the external data, and
    /// `foreach` must correctly visit any Lean object references held by the data.
    pub unsafe fn register(
        finalizer: include::lean_external_finalize_proc,
        foreach: include::lean_external_foreach_proc,
    ) -> Self {
        Self(unsafe { include::lean_register_external_class(finalizer, foreach) })
    }

    /// Register a new external class that uses `Drop` to finalize `T`
    /// and has no Lean object references to visit.
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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

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
        unsafe {
            debug_assert!(ptr as usize & 1 == 1 || include::lean_obj_tag(ptr) as u8 == 1);
            Self(LeanOwned(ptr))
        }
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
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanOption — Option α
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }
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
            #[allow(clippy::cast_possible_truncation)]
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
        unsafe {
            debug_assert!(ptr as usize & 1 == 1 || include::lean_obj_tag(ptr) as u8 == 1);
            Self(LeanOwned(ptr))
        }
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
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanOption<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanOption<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanExcept — Except ε α
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }
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
        unsafe {
            debug_assert!(ptr as usize & 1 != 1);
            debug_assert!(
                include::lean_obj_tag(ptr) as u8 == 0 || include::lean_obj_tag(ptr) as u8 == 1
            );
            Self(LeanOwned(ptr))
        }
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
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanExcept<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanExcept<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanIOResult — EStateM.Result (BaseIO.Result)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }
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
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanIOResult<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanIOResult<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        LeanOwned(ptr)
    }
}

// =============================================================================
// LeanProd — Prod α β (pair)
// =============================================================================

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
    pub fn inner(&self) -> &R { &self.0 }
    #[inline]
    pub fn as_raw(&self) -> *mut include::lean_object { self.0.as_raw() }

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
        std::mem::forget(self);
        ptr
    }
}

impl From<LeanProd<LeanOwned>> for LeanOwned {
    #[inline]
    fn from(x: LeanProd<LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
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

/// Helper methods for interpreting a reference as a specific domain type (borrowed view).
impl<'a> LeanBorrowed<'a> {
    /// Interpret as a constructor object.
    #[inline]
    pub fn as_ctor(self) -> LeanCtor<LeanBorrowed<'a>> {
        debug_assert!(!self.is_scalar() && self.tag() <= LEAN_MAX_CTOR_TAG);
        LeanCtor(self)
    }

    /// Interpret as a `String` object.
    #[inline]
    pub fn as_string(self) -> LeanString<LeanBorrowed<'a>> {
        debug_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_STRING);
        LeanString(self)
    }

    /// Interpret as an `Array` object.
    #[inline]
    pub fn as_array(self) -> LeanArray<LeanBorrowed<'a>> {
        debug_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_ARRAY);
        LeanArray(self)
    }

    /// Interpret as a `List`.
    #[inline]
    pub fn as_list(self) -> LeanList<LeanBorrowed<'a>> {
        debug_assert!(self.is_scalar() || self.tag() == 1);
        LeanList(self)
    }

    /// Interpret as a `ByteArray` object.
    #[inline]
    pub fn as_byte_array(self) -> LeanByteArray<LeanBorrowed<'a>> {
        debug_assert!(!self.is_scalar() && self.tag() == LEAN_TAG_SCALAR_ARRAY);
        LeanByteArray(self)
    }
}

// =============================================================================
// LeanShared — Thread-safe owned Lean object
// =============================================================================

/// Thread-safe owned Lean object with atomic refcounting.
///
/// Created by calling [`lean_mark_mt`] on the object graph, which transitions
/// all reachable objects from single-threaded to multi-threaded mode.
/// After marking, [`lean_inc_ref`] / [`lean_dec_ref`] use atomic operations,
/// so [`LeanOwned`]'s existing `Clone` and `Drop` are thread-safe.
///
/// Scalars (tagged pointers with bit 0 set) and persistent objects
/// (`m_rc == 0`) are unaffected by MT marking.
#[repr(transparent)]
pub struct LeanShared(LeanOwned);

// SAFETY: lean_mark_mt transitions the entire reachable object graph to
// multi-threaded mode. After marking, lean_inc_ref uses atomic operations
// for refcount increments, and lean_dec_ref delegates to lean_dec_ref_cold
// which also handles MT objects atomically. This makes Clone (inc_ref) and
// Drop (dec_ref) thread-safe.
unsafe impl Send for LeanShared {}
unsafe impl Sync for LeanShared {}

impl LeanShared {
    /// Mark an owned object's entire reachable graph as MT and take ownership.
    ///
    /// Persistent objects (`m_rc == 0`) and scalars are unaffected.
    /// After this call, all refcount operations on the object graph use
    /// atomic instructions.
    #[inline]
    pub fn new(owned: LeanOwned) -> Self {
        if !owned.is_scalar() && !owned.is_persistent() {
            unsafe { include::lean_mark_mt(owned.as_raw()); }
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
