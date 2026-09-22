use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use vvv_core::{Address, Position, ReachKind};

/// Something a plan could not do and a human should look at. Never fatal:
/// the rest of the plan is still valid. Data only; the display layer words it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub path: RelPath,
    pub start: Position,
    #[serde(flatten)]
    pub kind: NoticeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoticeKind {
    /// An import the layout understood but the surgery could not rewrite in place;
    /// `replacement` is what it should become.
    UnrewritableImport { import: String, replacement: String },
    /// After the move, `from` references `item` but may not see it, and the
    /// widening it would take is not one vvv makes on its own (making a
    /// declaration public across packages is the author's call).
    Unreachable {
        item: String,
        from: Address,
        needs: ReachKind,
    },
    /// An import that now names a declaration in its own file, inside a
    /// grouped statement vvv does not split for this; remove the entry.
    RedundantImport { import: String },
}
