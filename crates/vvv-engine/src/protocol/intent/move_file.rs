use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Move a file and make every reference to it follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveIntent {
    pub from: PathBuf,
    pub to: PathBuf,
}

impl MoveIntent {
    pub fn new(from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}
