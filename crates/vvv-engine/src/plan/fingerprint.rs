use std::fmt;

use serde::{Deserialize, Serialize};

/// Content hash used to detect that a file changed between planning and applying.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn of(text: &str) -> Self {
        Self(blake3::hash(text.as_bytes()).to_hex().to_string())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({}…)", &self.0[..8])
    }
}
