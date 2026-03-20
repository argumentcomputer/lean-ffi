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

/// No-op foreach callback for external classes that hold no Lean references.
///
/// # Safety
/// Must only be used as a `lean_external_foreach_fn` callback.
pub unsafe extern "C" fn noop_foreach(_: *mut c_void, _: *mut include::lean_object) {}

/// Generate a `#[repr(transparent)]` newtype over a `LeanRef` type parameter
/// for a specific Lean type, with Clone, conditional Copy, from_raw, into_raw, and From impls.
///
/// # Naming convention
///
/// Domain types should be prefixed with `Lean` to distinguish them from Lean-side types
/// and to match the built-in types (`LeanArray`, `LeanString`, `LeanNat`, etc.).
/// For example, a Lean `Point` structure becomes `LeanPoint` in Rust:
///
/// ```ignore
/// lean_domain_type! {
///     /// Lean `Point` — structure Point where x : Nat; y : Nat
///     LeanPoint;
/// }
/// ```
#[macro_export]
macro_rules! lean_domain_type {
  ($($(#[$meta:meta])* $name:ident;)*) => {$(
    $(#[$meta])*
    #[repr(transparent)]
    pub struct $name<R: $crate::object::LeanRef>(R);

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

    impl $name<$crate::object::LeanOwned> {
      /// Wrap an owned `LeanOwned` value.
      #[inline]
      pub fn new(obj: $crate::object::LeanOwned) -> Self { Self(obj) }

      /// Consume without calling `lean_dec`.
      #[inline]
      pub fn into_raw(self) -> *mut $crate::include::lean_object {
        let ptr = self.0.as_raw();
        std::mem::forget(self);
        ptr
      }
    }

    impl From<$name<$crate::object::LeanOwned>> for $crate::object::LeanOwned {
      #[inline]
      fn from(x: $name<$crate::object::LeanOwned>) -> Self {
        let ptr = x.0.as_raw();
        std::mem::forget(x);
        unsafe { $crate::object::LeanOwned::from_raw(ptr) }
      }
    }
  )*};
}

