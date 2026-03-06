use serde::{Deserialize, Serialize};
use std::fmt;

/// A content-addressed reference to a stored expression.
/// 32-byte blake3 hash that acts as a pointer into the expression store.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TermRef(pub [u8; 32]);

impl TermRef {
    /// Compute the hash of raw bytes.
    pub fn from_bytes(data: &[u8]) -> Self {
        TermRef(*blake3::hash(data).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
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
