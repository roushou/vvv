use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Move one declaration — with what belongs to it in the text, and for Rust
/// its `impl` blocks — from the file declaring it to another file of the
/// same language, and make every reference follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveSymbolIntent {
    pub name: String,
    /// The file declaring it.
    pub from: PathBuf,
    /// The file to declare it in; it must exist.
    pub to: PathBuf,
}

impl MoveSymbolIntent {
    pub fn new(name: impl Into<String>, from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            from: from.into(),
            to: to.into(),
        }
    }
}
