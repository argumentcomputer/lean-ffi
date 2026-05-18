//! Lean `Nat` (arbitrary-precision natural number) FFI surface.
//!
//! The generic `Nat = BigUint` newtype lives in the `bignat` crate and is
//! re-exported here. This module adds the Lean-side encode/decode operations
//! as inherent methods on [`LeanNat<LeanOwned>`], plus the GMP-backed limb
//! constructor used to build big Nats.
//!
//! Lean stores small naturals as tagged scalars and large ones as GMP
//! `mpz_object`s on the heap; both representations are handled here.

use std::ffi::c_int;
use std::mem::MaybeUninit;

use num_bigint::BigUint;

pub use bignat::Nat;

use crate::include::lean_uint64_to_nat;
use crate::object::{LeanNat, LeanOwned, LeanRef};

impl LeanNat<LeanOwned> {
    /// Decode a [`Nat`] from any Lean reference. Handles both scalar (unboxed)
    /// and heap-allocated (GMP `mpz_object`) representations.
    pub fn to_nat(obj: &impl LeanRef) -> Nat {
        if obj.is_scalar() {
            Nat(BigUint::from(obj.unbox_usize() as u64))
        } else {
            let mpz: &MpzObject = unsafe { &*obj.as_raw().cast() };
            Nat(mpz.m_value.to_biguint())
        }
    }

    /// Convert a [`Nat`] into a Lean `Nat` (owned reference).
    pub fn from_nat(n: &Nat) -> Self {
        let raw = match n.to_u64() {
            Some(val) if val <= (usize::MAX >> 1) as u64 => {
                #[allow(clippy::cast_possible_truncation)]
                let scalar = val as usize;
                LeanOwned::box_usize(scalar)
            }
            Some(val) => LeanOwned::from_nat_u64(val),
            None => {
                let limbs = n.0.to_u64_digits();
                unsafe { lean_nat_from_limbs(limbs.len(), limbs.as_ptr()) }
            }
        };
        LeanNat::new(raw)
    }
}

/// From https://github.com/leanprover/lean4/blob/master/src/runtime/object.h:
/// ```cpp
/// struct mpz_object {
///     lean_object m_header;
///     mpz         m_value;
///     mpz_object() {}
///     explicit mpz_object(mpz const & m):m_value(m) {}
/// };
/// ```
#[repr(C)]
struct MpzObject {
    _header: [u8; 8],
    m_value: Mpz,
}

#[repr(C)]
struct Mpz {
    alloc: i32,
    size: i32,
    d: *const u64,
}

impl Mpz {
    fn to_biguint(&self) -> BigUint {
        let nlimbs = self.size.unsigned_abs() as usize;
        let limbs = unsafe { std::slice::from_raw_parts(self.d, nlimbs) };

        // Convert limbs (little-endian by limb)
        let bytes: Vec<_> = limbs.iter().flat_map(|&limb| limb.to_le_bytes()).collect();

        BigUint::from_bytes_le(&bytes)
    }
}

// =============================================================================
// GMP interop for building Lean Nat objects from limbs
// =============================================================================

/// LEAN_MAX_SMALL_NAT = SIZE_MAX >> 1
const LEAN_MAX_SMALL_NAT: u64 = (usize::MAX >> 1) as u64;

unsafe extern "C" {
    #[link_name = "__gmpz_init"]
    fn mpz_init(x: *mut Mpz);

    #[link_name = "__gmpz_import"]
    fn mpz_import(
        rop: *mut Mpz,
        count: usize,
        order: c_int,
        size: usize,
        endian: c_int,
        nails: usize,
        op: *const u64,
    );

    #[link_name = "__gmpz_clear"]
    fn mpz_clear(x: *mut Mpz);

    /// Lean's internal mpz allocation — deep-copies the mpz value.
    /// Caller must still call mpz_clear on the original.
    fn lean_alloc_mpz(v: *mut Mpz) -> *mut std::ffi::c_void;
}

/// Create a Lean `Nat` from a little-endian array of u64 limbs.
/// # Safety
/// `limbs` must be valid for reading `num_limbs` elements.
pub unsafe fn lean_nat_from_limbs(num_limbs: usize, limbs: *const u64) -> LeanOwned {
    match num_limbs {
        0 => LeanOwned::box_usize(0),
        1 => {
            let first = unsafe { *limbs };
            if first <= LEAN_MAX_SMALL_NAT {
                #[allow(clippy::cast_possible_truncation)] // only targets 64-bit
                let scalar = first as usize;
                LeanOwned::box_usize(scalar)
            } else {
                unsafe { LeanOwned::from_raw(lean_uint64_to_nat(first)) }
            }
        }
        // Multi-limb: use GMP
        _ => unsafe {
            let mut value = MaybeUninit::<Mpz>::uninit();
            mpz_init(value.as_mut_ptr());
            // order = -1 (least significant limb first)
            // size = 8 bytes per limb, endian = 0 (native), nails = 0
            mpz_import(value.as_mut_ptr(), num_limbs, -1, 8, 0, 0, limbs);
            // lean_alloc_mpz deep-copies; we must free the original
            let result = lean_alloc_mpz(value.as_mut_ptr());
            mpz_clear(value.as_mut_ptr());
            LeanOwned::from_raw(result.cast())
        },
    }
}
