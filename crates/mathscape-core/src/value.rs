use serde::{Deserialize, Serialize};
use std::fmt;

/// A numeric value in the expression system.
/// Starts with naturals (Peano-style), extensible to rationals/reals later.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Value {
    /// Natural number (Peano encoding for small values, direct for large)
    Nat(u64),
}

impl Value {
    pub fn zero() -> Self {
        Value::Nat(0)
    }

    pub fn succ(&self) -> Self {
        match self {
            Value::Nat(n) => Value::Nat(n + 1),
        }
    }

    pub fn as_nat(&self) -> Option<u64> {
        match self {
            Value::Nat(n) => Some(*n),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nat(n) => write!(f, "{n}"),
        }
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Nat(n)
    }
}
