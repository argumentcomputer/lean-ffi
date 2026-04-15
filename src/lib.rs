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

/// Generate a `#[repr(transparent)]` newtype over a `LeanRef` type parameter
/// for a specific Lean type, with Clone, conditional Copy, `as_ctor`, `from_ctor`,
/// `new`, `into_raw`, and `From<Self<LeanOwned>> for LeanOwned` impls.
///
/// This is the low-level building block for bare domain types (external
/// objects, types without a ctor layout, or types whose layout is attached
/// separately). For ctor-backed structures and inductives, prefer
/// [`lean_inductive!`] — it calls this macro internally and also attaches
/// the layout + accessor methods in one declaration.
///
/// # Naming convention
///
/// Domain types should be prefixed with `Lean` to distinguish them from Lean-side
/// types and to match the built-in types (`LeanArray`, `LeanString`, `LeanNat`, etc.).
///
/// ```ignore
/// lean_domain_type! {
///     /// Lean `RustData` — opaque external object
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

/// Attach a single `lean_ctor_object` layout to a [`lean_domain_type!`] wrapper.
///
/// Implements [`LeanCtorLayout<1>`](object::LeanCtorLayout) and generates
/// inherent `alloc()`, `ctor_tag()`, `get_obj` / `set_obj`, `get_usize` /
/// `set_usize`, and `get_num_{64,32,16,8}` / `set_num_{64,32,16,8}` methods.
/// Indices are bounds-checked against the declared counts and all byte offsets
/// are const-computed from the layout.
///
/// Within each scalar size (8B / 4B / 2B / 1B), fields follow Lean's
/// declaration order. Tag defaults to 0; pass `tag: N` for non-zero variants.
///
/// Most callers should use [`lean_inductive!`] instead — it composes
/// [`lean_domain_type!`] + `lean_ctor!` in one declaration. Reach for
/// `lean_ctor!` directly only if the domain type is declared separately (e.g.
/// in another module) or if you want to attach the layout after the fact.
///
/// ```ignore
/// lean_domain_type! { LeanFoo; }
/// lean_ctor!(LeanFoo { num_obj: 1, num_64: 2 });
///
/// let foo = LeanFoo::alloc();
/// foo.set_obj(0, some_val);
/// foo.set_num_64(0, 42);
/// ```
#[macro_export]
macro_rules! lean_ctor {
    ($ty:ident { $($key:ident : $val:expr),* $(,)? }) => {
        impl<R: $crate::object::LeanRef> $crate::object::LeanCtorLayout<1> for $ty<R> {
            const LAYOUTS: [$crate::object::SingleCtorLayout; 1] = [
                $crate::object::SingleCtorLayout {
                    $($key: $val,)*
                    ..$crate::object::SingleCtorLayout::ZERO
                }
            ];
        }

        impl<R: $crate::object::LeanRef> $ty<R> {
            #[doc(hidden)]
            const __LAYOUT: $crate::object::SingleCtorLayout =
                <Self as $crate::object::LeanCtorLayout<1>>::LAYOUTS[0];

            /// Constructor tag this wrapper represents.
            #[inline]
            pub const fn ctor_tag() -> u8 {
                Self::__LAYOUT.tag
            }

            /// Get a borrowed reference to the `i`-th object field.
            pub fn get_obj(&self, i: usize) -> $crate::object::LeanBorrowed<'_> {
                assert!(
                    i < Self::__LAYOUT.num_obj,
                    "object field index {i} out of bounds (num_obj = {})",
                    Self::__LAYOUT.num_obj,
                );
                let raw = unsafe {
                    $crate::include::lean_ctor_get(
                        self.as_raw(),
                        $crate::object::to_u32(i),
                    )
                };
                unsafe { $crate::object::LeanBorrowed::from_raw(raw) }
            }

            /// Set the `i`-th object field. Takes ownership of `val`.
            pub fn set_obj(&self, i: usize, val: impl Into<$crate::object::LeanOwned>) {
                assert!(
                    i < Self::__LAYOUT.num_obj,
                    "object field index {i} out of bounds (num_obj = {})",
                    Self::__LAYOUT.num_obj,
                );
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
                assert!(
                    i < Self::__LAYOUT.num_usize,
                    "USize field index {i} out of bounds (num_usize = {})",
                    Self::__LAYOUT.num_usize,
                );
                self.as_ctor().get_usize(i)
            }
            pub fn set_usize(&self, i: usize, val: usize) {
                assert!(
                    i < Self::__LAYOUT.num_usize,
                    "USize field index {i} out of bounds (num_usize = {})",
                    Self::__LAYOUT.num_usize,
                );
                self.as_ctor().set_usize(i, val)
            }

            pub fn get_num_64(&self, i: usize) -> u64 {
                assert!(
                    i < Self::__LAYOUT.num_64,
                    "64-bit field index {i} out of bounds (num_64 = {})",
                    Self::__LAYOUT.num_64,
                );
                self.as_ctor().get_u64(Self::__LAYOUT.offset_64(i))
            }
            pub fn set_num_64(&self, i: usize, val: u64) {
                assert!(
                    i < Self::__LAYOUT.num_64,
                    "64-bit field index {i} out of bounds (num_64 = {})",
                    Self::__LAYOUT.num_64,
                );
                self.as_ctor().set_u64(Self::__LAYOUT.offset_64(i), val)
            }

            pub fn get_num_32(&self, i: usize) -> u32 {
                assert!(
                    i < Self::__LAYOUT.num_32,
                    "32-bit field index {i} out of bounds (num_32 = {})",
                    Self::__LAYOUT.num_32,
                );
                self.as_ctor().get_u32(Self::__LAYOUT.offset_32(i))
            }
            pub fn set_num_32(&self, i: usize, val: u32) {
                assert!(
                    i < Self::__LAYOUT.num_32,
                    "32-bit field index {i} out of bounds (num_32 = {})",
                    Self::__LAYOUT.num_32,
                );
                self.as_ctor().set_u32(Self::__LAYOUT.offset_32(i), val)
            }

            pub fn get_num_16(&self, i: usize) -> u16 {
                assert!(
                    i < Self::__LAYOUT.num_16,
                    "16-bit field index {i} out of bounds (num_16 = {})",
                    Self::__LAYOUT.num_16,
                );
                self.as_ctor().get_u16(Self::__LAYOUT.offset_16(i))
            }
            pub fn set_num_16(&self, i: usize, val: u16) {
                assert!(
                    i < Self::__LAYOUT.num_16,
                    "16-bit field index {i} out of bounds (num_16 = {})",
                    Self::__LAYOUT.num_16,
                );
                self.as_ctor().set_u16(Self::__LAYOUT.offset_16(i), val)
            }

            pub fn get_num_8(&self, i: usize) -> u8 {
                assert!(
                    i < Self::__LAYOUT.num_8,
                    "8-bit field index {i} out of bounds (num_8 = {})",
                    Self::__LAYOUT.num_8,
                );
                self.as_ctor().get_u8(Self::__LAYOUT.offset_8(i))
            }
            pub fn set_num_8(&self, i: usize, val: u8) {
                assert!(
                    i < Self::__LAYOUT.num_8,
                    "8-bit field index {i} out of bounds (num_8 = {})",
                    Self::__LAYOUT.num_8,
                );
                self.as_ctor().set_u8(Self::__LAYOUT.offset_8(i), val)
            }
        }

        impl $ty<$crate::object::LeanOwned> {
            /// Allocate a new constructor with this type's layout.
            pub fn alloc() -> Self {
                const L: $crate::object::SingleCtorLayout =
                    <$ty<$crate::object::LeanOwned> as $crate::object::LeanCtorLayout<1>>::LAYOUTS[0];
                Self::new($crate::object::LeanCtor::alloc(L.tag, L.num_obj, L.scalar_size()).into())
            }
        }
    };
}

/// Declare a Lean structure or multi-variant inductive and all its field
/// layouts in one shot.
///
/// **Structure form** (one layout, tag 0):
/// ```ignore
/// lean_inductive! {
///     LeanPoint { num_obj: 2 }
/// }
/// ```
///
/// **Multi-variant form** (one wrapper per variant + top-level dispatch type):
/// ```ignore
/// lean_inductive! {
///     LeanCompareResult {
///         LeanCompareMatched   { tag: 0 },
///         LeanCompareMismatch  { tag: 1, num_64: 3 },
///         LeanCompareNotFound  { tag: 2 },
///     }
/// }
///
/// // Read side:
/// match result.as_ctor().tag() {
///     1 => {
///         let m = LeanCompareMismatch::from_ctor(result.as_ctor());
///         let first_diff = m.get_num_64(2);
///     }
///     _ => { /* matched or not found */ }
/// }
///
/// // Write side:
/// let m = LeanCompareMismatch::alloc();
/// m.set_num_64(0, lean_size);
/// let result: LeanCompareResult<LeanOwned> = m.into();
/// ```
///
/// The form is disambiguated by the token after the first inner ident: `:`
/// (key-value pair) means structure, `{` (brace group) means variant. Variants
/// must be listed in tag order (tag 0 first, dense) and their names must differ
/// from the top-level name.
///
/// This macro composes [`lean_domain_type!`] + [`lean_ctor!`]. Use those
/// directly if you need to split the domain-type declaration from the layout
/// (e.g. to declare wrappers that aren't ctors, or to attach the layout from a
/// different module).
#[macro_export]
macro_rules! lean_inductive {
    // --- Structure form: LeanFoo { num_obj: 1, num_64: 2 } ---
    //
    // Distinguished from the multi-variant arm by the trailing `:` after the
    // first inner ident (vs. `{` for the multi-variant case).
    (
        $(#[$top_meta:meta])*
        $top:ident { $($key:ident : $val:expr),* $(,)? }
    ) => {
        $crate::lean_domain_type! { $(#[$top_meta])* $top; }
        $crate::lean_ctor!($top { $($key : $val),* });
    };

    // --- Multi-variant form: LeanFoo { Variant1 { ... }, Variant2 { ... } } ---
    (
        $(#[$top_meta:meta])*
        $top:ident {
            $(
                $(#[$var_meta:meta])*
                $variant:ident { $($key:ident : $val:expr),* $(,)? }
            ),+ $(,)?
        }
    ) => {
        $crate::lean_domain_type! {
            $(#[$top_meta])* $top;
            $( $(#[$var_meta])* $variant; )+
        }
        $( $crate::lean_ctor!($variant { $($key : $val),* }); )+

        impl<R: $crate::object::LeanRef>
            $crate::object::LeanCtorLayout<{ [ $( stringify!($variant) ),+ ].len() }>
            for $top<R>
        {
            const LAYOUTS: [
                $crate::object::SingleCtorLayout;
                [ $( stringify!($variant) ),+ ].len()
            ] = [
                $(
                    <$variant<$crate::object::LeanOwned>
                        as $crate::object::LeanCtorLayout<1>>::LAYOUTS[0],
                )+
            ];
        }

        $(
            impl From<$variant<$crate::object::LeanOwned>>
                for $top<$crate::object::LeanOwned>
            {
                #[inline]
                fn from(v: $variant<$crate::object::LeanOwned>) -> Self {
                    Self::new(v.into())
                }
            }
        )+
    };
}
