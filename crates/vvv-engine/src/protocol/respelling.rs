use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use vvv_core::{Position, Span};

/// A reference a move re-spelled in place: where it is, what it said and
/// what it says now. A preview lists these as one row each instead of a
/// diff hunk; an edit no respelling accounts for is structural and shows as
/// a hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Respelling {
    pub path: RelPath,
    /// The reference's span before the move — the edit that rewrites it.
    pub span: Span,
    pub start: Position,
    pub from: String,
    pub to: String,
}
