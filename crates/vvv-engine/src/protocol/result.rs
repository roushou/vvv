//! What each command answers with. A mutating command answers the same shape
//! whether it previewed or wrote: `applied` and `history_id` say which.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};
use vvv_core::{Address, Edit, Query};

use super::diff::Diff;
use super::{
    BatchIntent, Intent, Match, MoveIntent, MoveSymbolIntent, Notice, Occurrence, RenameIntent,
    Respelling, RewriteIntent, Skipped,
};
use crate::plan::{FilePreview, Plan};

/// `vvv search`: what was found, and which languages could not be asked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Search {
    pub query: Query,
    pub matches: Vec<Match>,
    /// Languages whose grammar could not compile the query; their files
    /// were not searched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// `vvv rewrite`: one edit per selected match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rewrite {
    pub intent: RewriteIntent,
    /// `true` when the files were written; otherwise this is a preview.
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    pub files: Vec<FileChange>,
}

/// `vvv rename`: the declaration and every occurrence, each judged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rename {
    pub intent: RenameIntent,
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    /// Where `name` is declared; more than one means the rename is ambiguous.
    pub declarations: Vec<Match>,
    /// Every identifier spelling the name, each judged against the target;
    /// their ids feed a later `--select`.
    pub occurrences: Vec<Occurrence>,
    pub files: Vec<FileChange>,
}

/// `vvv move`: a file or directory moved, its importers respelled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Move {
    pub intent: MoveIntent,
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    /// Normalised, workspace-relative source and destination.
    pub from: RelPath,
    pub to: RelPath,
    /// The module address before and after, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_address: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_address: Option<Address>,
    /// References vvv found but could not rewrite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// References rewritten in place; every other edit in `files` is
    /// structural (a `mod` line moved, a visibility widened).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub respellings: Vec<Respelling>,
    pub files: Vec<FileChange>,
}

/// `vvv move --symbol`: one declaration moved between files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveSymbol {
    pub intent: MoveSymbolIntent,
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    /// The declaration's address before and after.
    pub from: Address,
    pub to: Address,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// Consumers rewritten in place; every other edit in `files` is the
    /// declaration itself moving.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub respellings: Vec<Respelling>,
    pub files: Vec<FileChange>,
}

/// `vvv batch`: several intents planned in sequence and applied as one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub intents: Vec<Intent>,
    /// `true` when the files were written; otherwise this is a preview.
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// Every file any step touches, before the first step against after the
    /// last. Edits are not listed per file: they belong to the steps, each in
    /// the coordinates of the state before it.
    pub files: Vec<FileChange>,
}

/// One apply that can still be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// What was applied, as data; the display layer renders it.
    pub intent: Intent,
    /// How many files it wrote.
    pub files: usize,
    /// The files it wrote, at their pre-apply paths: what undo restores.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<RelPath>,
    /// Moves it made, as (from, to) pairs: what undo reverts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves: Vec<(RelPath, RelPath)>,
}

/// `vvv history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    /// Oldest first; the last entry is what `vvv undo` would reverse.
    pub entries: Vec<HistoryEntry>,
}

/// `vvv undo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Undo {
    pub undone: HistoryEntry,
    /// Files restored to their pre-apply contents, at their pre-apply paths.
    pub restored: Vec<RelPath>,
    /// Moves reverted, as (original, moved-to) pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves_reverted: Vec<(RelPath, RelPath)>,
}

/// One file a plan touches: its edits and the whole-file diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Path before the change.
    pub path: RelPath,
    /// Path after, when the file is moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<RelPath>,
    pub edits: Vec<Edit>,
    pub diff: Diff,
}

impl FileChange {
    /// One entry per previewed file, with the plan's edits for it when a
    /// single plan made them (a batch's belong to its steps).
    pub(crate) fn all(plan: Option<&Plan>, preview: &[FilePreview]) -> Vec<Self> {
        preview
            .iter()
            .map(|file| FileChange {
                path: file.path.clone(),
                moved_to: file.moved_to.clone(),
                edits: plan
                    .map_or_else(Vec::new, |p| p.change_set().edits_for(&file.path).to_vec()),
                diff: Diff::between(
                    &file.path,
                    file.moved_to.as_deref().unwrap_or(&file.path),
                    &file.before,
                    &file.after,
                ),
            })
            .collect()
    }
}

/// A result that can be written: what `Engine::apply` needs from it and
/// tells it.
pub trait Mutation {
    /// What is recorded in history.
    fn intent(&self) -> Intent;
    /// Mark the result as written by history entry `id`.
    fn applied(&mut self, id: u64);
}

macro_rules! mutation {
    ($t:ty, $variant:ident) => {
        impl Mutation for $t {
            fn intent(&self) -> Intent {
                Intent::$variant(self.intent.clone())
            }
            fn applied(&mut self, id: u64) {
                self.applied = true;
                self.history_id = Some(id);
            }
        }
    };
}

mutation!(Rewrite, Rewrite);
mutation!(Rename, Rename);
mutation!(Move, Move);
mutation!(MoveSymbol, MoveSymbol);

impl Mutation for Batch {
    fn intent(&self) -> Intent {
        Intent::Batch(BatchIntent {
            intents: self.intents.clone(),
        })
    }
    fn applied(&mut self, id: u64) {
        self.applied = true;
        self.history_id = Some(id);
    }
}
