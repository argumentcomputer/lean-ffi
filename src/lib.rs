//! Low-level Lean FFI bindings and ownership-aware type-safe wrappers.
//!
//! The `include` submodule contains auto-generated bindings from `lean.h` via
//! bindgen. Higher-level helpers are in `object` and `nat`.

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unsafe_op_in_unsafe_fn,
    unused_qualifications,
    clippy::all,
    clippy::ptr_as_ptr,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::derive_partial_eq_without_eq
)]
pub mod include {
    include!(concat!(env!("OUT_DIR"), "/lean.rs"));
}

pub mod nat;
pub mod object;

pub use object::LeanShared;

#[cfg(feature = "test-ffi")]
mod test_ffi;

use std::ffi::{CString, c_void};

/// Create a CString from a str, stripping any interior null bytes.
/// Lean strings are length-prefixed and can contain null bytes, but the
/// `lean_mk_string` FFI requires a null-terminated C string. This function
/// ensures conversion always succeeds by filtering out interior nulls.
pub fn safe_cstring(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| {
        let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
        CString::new(bytes).expect("filtered string should have no nulls")
    })
}

/// Signal activity to Lean's task system. Call periodically in long-running
/// FFI functions to prevent Lean from treating them as stuck.
#[inline]
pub fn inc_heartbeat() {
    unsafe { include::lean_inc_heartbeat() }
}

/// No-op foreach callback for external classes that hold no Lean references.
///
/// # Safety
/// Must only be used as a `lean_external_foreach_fn` callback.
pub unsafe extern "C" fn noop_foreach(_: *mut c_void, _: *mut include::lean_object) {}

/// Declare a `#[repr(transparent)]` wrapper `Ty<R: LeanRef>` for a Lean domain
/// type, with `inner`, `as_raw`, `as_ctor`, `from_ctor`, `new`, `into_raw`,
/// `Clone`, conditional `Copy`, and `From<Ty<LeanOwned>> for LeanOwned`.
///
/// This is the low-level primitive for wrappers that are **not** ctor-backed
/// — opaque externals, types represented by a tagged scalar (`lean_box(n)`),
/// or wrappers whose layout is attached from a different module. For Lean
/// `structure` / `inductive` types, use [`lean_inductive!`] instead; it calls
/// this macro and also attaches the layout and typed accessors.
///
/// Wrapper names are prefixed with `Lean` to match the built-ins (`LeanArray`,
/// `LeanString`, `LeanNat`, …).
///
/// ```ignore
/// lean_domain_type! {
///     /// Rust handle wrapped as an opaque Lean `@[extern_type]`.
///     LeanRustData;
/// }
/// ```
#[macro_export]
macro_rules! lean_domain_type {
  ($($(#[$meta:meta])* $name:ident;)*) => {$(
    $(#[$meta])*
    #[repr(transparent)]
    pub struct $name<R: $crate::object::LeanRef>(pub R);

    impl<R: $crate::object::LeanRef> Clone for $name<R> {
      #[inline]
      fn clone(&self) -> Self { Self(self.0.clone()) }
    }

    impl<R: $crate::object::LeanRef + Copy> Copy for $name<R> {}

    impl<R: $crate::object::LeanRef> $name<R> {
      /// Get the inner reference.
      #[inline]
      pub fn inner(&self) -> &R { &self.0 }

      /// Get the raw lean_object pointer.
      #[inline]
      pub fn as_raw(&self) -> *mut $crate::include::lean_object { self.0.as_raw() }

      /// View this object as a `LeanCtor` for field access.
      #[inline]
      pub fn as_ctor(&self) -> $crate::object::LeanCtor<$crate::object::LeanBorrowed<'_>> {
          unsafe { $crate::object::LeanBorrowed::from_raw(self.0.as_raw()) }.as_ctor()
      }
    }

    impl<'a> $name<$crate::object::LeanBorrowed<'a>> {
      /// Wrap a borrowed `LeanCtor` as this domain type.
      #[inline]
      pub fn from_ctor(ctor: $crate::object::LeanCtor<$crate::object::LeanBorrowed<'a>>) -> Self {
          Self(unsafe { $crate::object::LeanBorrowed::from_raw(ctor.as_raw()) })
      }
    }

    impl $name<$crate::object::LeanOwned> {
      /// Wrap an owned `LeanOwned` value.
      #[inline]
      pub fn new(obj: $crate::object::LeanOwned) -> Self { Self(obj) }

      /// Consume without calling `lean_dec`.
      #[inline]
      pub fn into_raw(self) -> *mut $crate::include::lean_object {
        let ptr = self.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the caller
        std::mem::forget(self);
        ptr
      }
    }

    impl From<$name<$crate::object::LeanOwned>> for $crate::object::LeanOwned {
      #[inline]
      fn from(x: $name<$crate::object::LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        // Suppress Drop (lean_dec) — ownership transfers to the returned LeanOwned
        std::mem::forget(x);
        unsafe { $crate::object::LeanOwned::from_raw(ptr) }
      }
    }
  )*};
}

/// Declare a wrapper for a Lean `structure` or `inductive` with its full
/// per-constructor layout.
///
/// Emits a [`lean_domain_type!`] wrapper, an [`LeanCtorLayout`](object::LeanCtorLayout)
/// impl whose `LAYOUTS` slice has one entry per Lean constructor (indexed by
/// tag), plus these inherent methods:
///
/// - `alloc(tag: u8)` — `lean_alloc_ctor` for the given variant.
/// - `get_obj(i)` / `set_obj(i, val)`
/// - `get_usize(i)` / `set_usize(i, val)`
/// - `get_num_{64,32,16,8}(i)` / `set_num_{64,32,16,8}(i, val)`
///
/// Accessors read the object's actual tag from its header, look up
/// `LAYOUTS[tag]`, bounds-check the field index against that variant's
/// counts, and compute byte offsets. Within each scalar size (8B / 4B / 2B /
/// 1B), field indices follow Lean's declaration order.
///
/// A `structure` is the one-variant case. Typical call, using the `Point`
/// from `structure Point where x : Nat; y : Nat`:
///
/// ```ignore
/// lean_inductive! {
///     /// Lean `Point` — see `Tests/Gen.lean`.
///     LeanPoint [ { num_obj: 2 } ]
/// }
///
/// let p = LeanPoint::alloc(0);
/// p.set_obj(0, x_nat);
/// p.set_obj(1, y_nat);
/// ```
///
/// Multi-variant — wrapping this Lean inductive:
///
/// ```ignore
/// // Lean side:
/// // inductive BlockCompareResult
/// //   | matched
/// //   | mismatch (leanSize rustSize firstDiff : UInt64)
/// //   | notFound
///
/// lean_inductive! {
///     LeanBlockCompareResult [
///         { },                // matched
///         { num_64: 3 },      // mismatch
///         { },                // notFound
///     ]
/// }
///
/// // Build:
/// let m = LeanBlockCompareResult::alloc(1);
/// m.set_num_64(0, lean_size);
/// m.set_num_64(1, rust_size);
/// m.set_num_64(2, first_diff);
///
/// // Read:
/// match result.as_ctor().tag() {
///     1 => {
///         let (l, r, d) = (result.get_num_64(0), result.get_num_64(1), result.get_num_64(2));
///     }
///     _ => { /* matched | notFound */ }
/// }
/// ```
///
/// `alloc(tag)` and every accessor panic if the index is out of range for
/// the current variant.
#[macro_export]
macro_rules! lean_inductive {
    (
        $(#[$top_meta:meta])*
        $top:ident [
            $( { $($key:ident : $val:expr),* $(,)? } ),+ $(,)?
        ]
    ) => {
        $crate::lean_domain_type! { $(#[$top_meta])* $top; }

        impl<R: $crate::object::LeanRef> $crate::object::LeanCtorLayout for $top<R> {
            const LAYOUTS: &'static [$crate::object::SingleCtorLayout] = &[
                $(
                    $crate::object::SingleCtorLayout {
                        $($key: $val,)*
                        ..$crate::object::SingleCtorLayout::ZERO
                    },
                )+
            ];
        }

        impl<R: $crate::object::LeanRef> $top<R> {
            /// Layout of the variant this object currently holds (read from the
            /// ctor's tag in its object header).
            #[doc(hidden)]
            #[inline]
            fn __variant_layout(&self) -> $crate::object::SingleCtorLayout {
                let tag = self.as_ctor().tag() as usize;
                <Self as $crate::object::LeanCtorLayout>::LAYOUTS[tag]
            }

            /// Get a borrowed reference to the `i`-th object field.
            pub fn get_obj(&self, i: usize) -> $crate::object::LeanBorrowed<'_> {
                let l = self.__variant_layout();
                assert!(i < l.num_obj, "object field {i} out of bounds (num_obj = {})", l.num_obj);
                let raw = unsafe {
                    $crate::include::lean_ctor_get(self.as_raw(), $crate::object::to_u32(i))
                };
                unsafe { $crate::object::LeanBorrowed::from_raw(raw) }
            }

            /// Set the `i`-th object field. Takes ownership of `val`.
            pub fn set_obj(&self, i: usize, val: impl Into<$crate::object::LeanOwned>) {
                let l = self.__variant_layout();
                assert!(i < l.num_obj, "object field {i} out of bounds (num_obj = {})", l.num_obj);
                let val: $crate::object::LeanOwned = val.into();
                unsafe {
                    $crate::include::lean_ctor_set(
                        self.as_raw(),
                        $crate::object::to_u32(i),
                        val.into_raw(),
                    );
                }
            }

            pub fn get_usize(&self, i: usize) -> usize {
                let l = self.__variant_layout();
                assert!(i < l.num_usize, "USize field {i} out of bounds (num_usize = {})", l.num_usize);
                self.as_ctor().get_usize(i)
            }
            pub fn set_usize(&self, i: usize, val: usize) {
                let l = self.__variant_layout();
                assert!(i < l.num_usize, "USize field {i} out of bounds (num_usize = {})", l.num_usize);
                self.as_ctor().set_usize(i, val)
            }

            pub fn get_num_64(&self, i: usize) -> u64 {
                let l = self.__variant_layout();
                assert!(i < l.num_64, "64-bit field {i} out of bounds (num_64 = {})", l.num_64);
                self.as_ctor().get_u64(l.offset_64(i))
            }
            pub fn set_num_64(&self, i: usize, val: u64) {
                let l = self.__variant_layout();
                assert!(i < l.num_64, "64-bit field {i} out of bounds (num_64 = {})", l.num_64);
                self.as_ctor().set_u64(l.offset_64(i), val)
            }

            pub fn get_num_32(&self, i: usize) -> u32 {
                let l = self.__variant_layout();
                assert!(i < l.num_32, "32-bit field {i} out of bounds (num_32 = {})", l.num_32);
                self.as_ctor().get_u32(l.offset_32(i))
            }
            pub fn set_num_32(&self, i: usize, val: u32) {
                let l = self.__variant_layout();
                assert!(i < l.num_32, "32-bit field {i} out of bounds (num_32 = {})", l.num_32);
                self.as_ctor().set_u32(l.offset_32(i), val)
            }

            pub fn get_num_16(&self, i: usize) -> u16 {
                let l = self.__variant_layout();
                assert!(i < l.num_16, "16-bit field {i} out of bounds (num_16 = {})", l.num_16);
                self.as_ctor().get_u16(l.offset_16(i))
            }
            pub fn set_num_16(&self, i: usize, val: u16) {
                let l = self.__variant_layout();
                assert!(i < l.num_16, "16-bit field {i} out of bounds (num_16 = {})", l.num_16);
                self.as_ctor().set_u16(l.offset_16(i), val)
            }

            pub fn get_num_8(&self, i: usize) -> u8 {
                let l = self.__variant_layout();
                assert!(i < l.num_8, "8-bit field {i} out of bounds (num_8 = {})", l.num_8);
                self.as_ctor().get_u8(l.offset_8(i))
            }
            pub fn set_num_8(&self, i: usize, val: u8) {
                let l = self.__variant_layout();
                assert!(i < l.num_8, "8-bit field {i} out of bounds (num_8 = {})", l.num_8);
                self.as_ctor().set_u8(l.offset_8(i), val)
            }
        }

        impl $top<$crate::object::LeanOwned> {
            /// Allocate a new constructor object for the given tag (variant).
            ///
            /// Panics if `tag` is out of range for this type's `LAYOUTS`.
            pub fn alloc(tag: u8) -> Self {
                let layouts = <Self as $crate::object::LeanCtorLayout>::LAYOUTS;
                let layout = layouts[tag as usize];
                Self::new(
                    $crate::object::LeanCtor::alloc(tag, layout.num_obj, layout.scalar_size())
                        .into(),
                )
            }
        }
    };
}
