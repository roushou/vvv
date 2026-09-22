//! What the user wants done, as data.
//!
//! An intent is built by the CLI from arguments, by a TUI from a form, or by
//! a client from JSON, and handed to the engine. Nothing here knows how the
//! intent is fulfilled.

mod batch;
mod move_file;
mod move_symbol;
mod rename;
mod rewrite;

use serde::{Deserialize, Serialize};

pub use batch::BatchIntent;
pub use move_file::MoveIntent;
pub use move_symbol::MoveSymbolIntent;
pub use rename::RenameIntent;
pub use rewrite::{RewriteIntent, RewriteOf};

/// Any mutating request, as one value: what history records, what a summary
/// renders, what a remote client sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Intent {
    Rewrite(RewriteIntent),
    Rename(RenameIntent),
    Move(MoveIntent),
    MoveSymbol(MoveSymbolIntent),
    /// Several intents planned in sequence, each against the state the
    /// previous one leaves, and applied as one.
    Batch(BatchIntent),
}

impl From<BatchIntent> for Intent {
    fn from(i: BatchIntent) -> Self {
        Self::Batch(i)
    }
}

impl From<RewriteIntent> for Intent {
    fn from(i: RewriteIntent) -> Self {
        Self::Rewrite(i)
    }
}

impl From<RenameIntent> for Intent {
    fn from(i: RenameIntent) -> Self {
        Self::Rename(i)
    }
}

impl From<MoveIntent> for Intent {
    fn from(i: MoveIntent) -> Self {
        Self::Move(i)
    }
}

impl From<MoveSymbolIntent> for Intent {
    fn from(i: MoveSymbolIntent) -> Self {
        Self::MoveSymbol(i)
    }
}
