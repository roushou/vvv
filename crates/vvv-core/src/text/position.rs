use serde::{Deserialize, Serialize};

/// Zero-based line and character column. Columns count `char`s, not bytes,
/// so they are safe to show to humans and editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

impl Position {
    pub fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }

    /// One-based rendering for terminals (`12:4`).
    pub fn display(&self) -> String {
        format!("{}:{}", self.line + 1, self.column + 1)
    }
}
