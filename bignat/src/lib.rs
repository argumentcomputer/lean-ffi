//! Arbitrary-precision natural number, wrapping `BigUint`.
//!
//! This crate holds the generic `Nat` type used across projects that need a
//! `BigUint`-shaped natural number without taking a dependency on Lean. The
//! `lean-ffi` crate re-exports this `Nat`; the corresponding Lean-side
//! decode/encode lives there as inherent methods on `LeanNat<LeanOwned>`
//! (`from_nat` / `to_nat`).

use std::fmt;

use num_bigint::BigUint;

#[derive(Hash, PartialEq, Eq, Debug, Clone, PartialOrd, Ord)]
pub struct Nat(pub BigUint);

impl fmt::Display for Nat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Nat {
    fn from(x: u64) -> Self {
        Nat(BigUint::from(x))
    }
}

impl Nat {
    pub const ZERO: Self = Self(BigUint::ZERO);

    /// Try to convert to u64, returning None if the value is too large.
    #[inline]
    pub fn to_u64(&self) -> Option<u64> {
        u64::try_from(&self.0).ok()
    }

    #[inline]
    pub fn from_le_bytes(bytes: &[u8]) -> Nat {
        Nat(BigUint::from_bytes_le(bytes))
    }

    #[inline]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        self.0.to_bytes_le()
    }
}
