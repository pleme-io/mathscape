use serde::{Deserialize, Serialize};
use std::fmt;

/// A content-addressed reference to a stored expression.
/// 32-byte blake3 hash that acts as a pointer into the expression store.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TermRef(pub [u8; 32]);

impl TermRef {
    /// Compute the hash of raw bytes.
    pub fn from_bytes(data: &[u8]) -> Self {
        TermRef(*blake3::hash(data).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 64 || !s.is_ascii() {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(TermRef(out))
    }
}

impl fmt::Debug for TermRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TermRef({})", &hex(&self.0[..4]))
    }
}

impl fmt::Display for TermRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex(&self.0[..8]))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hex_round_trips_and_malformed_hex_is_refused() {
        let r = TermRef::from_bytes(b"mathscape");
        assert_eq!(TermRef::from_hex(&r.to_hex()), Some(r));
        assert_eq!(r.to_hex().len(), 64);
        for bad in ["", "zz", &"g".repeat(64), &r.to_hex()[..63]] {
            assert_eq!(TermRef::from_hex(bad), None);
        }
    }
}
