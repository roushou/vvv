//! What each command answers with. A mutating command answers the same shape
//! whether it previewed or wrote: `applied` and `history_id` say which.

use vvv_core::RelPath;

use serde::{Deserialize, Serialize};
use vvv_core::{Edit, Query};

use super::diff::Diff;
use super::{Intent, Match, Notice, RewriteIntent, Skipped};
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

/// The closed set of mutation payloads an executable plan can carry.
/// External payloads cannot authorize writes:
///
/// ```compile_fail,E0277
/// use vvv_engine::{Mutation, MutationAnswer};
/// struct External;
/// impl Mutation for External {
///     fn into_mutation(self) -> MutationAnswer { unimplemented!() }
///     fn applied(&mut self, _: u64) {}
/// }
/// ```
pub trait Mutation: sealed::Sealed {
    /// Wrap this payload in the closed mutation result.
    fn into_mutation(self) -> MutationAnswer;
    /// Mark the result as written by history entry `id`.
    fn applied(&mut self, id: u64);
}

macro_rules! mutation {
    ($t:ty, $variant:ident) => {
        impl Mutation for $t {
            fn into_mutation(self) -> MutationAnswer {
                MutationAnswer::$variant(self)
            }
            fn applied(&mut self, id: u64) {
                self.applied = true;
                self.history_id = Some(id);
            }
        }
    };
}

mutation!(Rewrite, Rewrite);

impl Mutation for Batch {
    fn into_mutation(self) -> MutationAnswer {
        MutationAnswer::Batch(self)
    }
    fn applied(&mut self, id: u64) {
        self.applied = true;
        self.history_id = Some(id);
    }
}

/// The result of a mutation intent. Queries have no variant here.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum MutationAnswer {
    Rewrite(Rewrite),
    Rename(super::Rename),
    Move(super::Move),
    MoveSymbol(super::MoveSymbol),
    Batch(Batch),
}

impl MutationAnswer {
    /// Notices retained by a mutation, if any.
    pub fn notices(&self) -> &[Notice] {
        match self {
            Self::Rewrite(_) | Self::Rename(_) => &[],
            Self::Move(result) => &result.notices,
            Self::MoveSymbol(result) => &result.notices,
            Self::Batch(result) => &result.notices,
        }
    }

    /// The history entry recorded by an applied mutation.
    pub fn history_id(&self) -> Option<u64> {
        match self {
            Self::Rewrite(result) => result.history_id,
            Self::Rename(result) => result.history_id,
            Self::Move(result) => result.history_id,
            Self::MoveSymbol(result) => result.history_id,
            Self::Batch(result) => result.history_id,
        }
    }
}

impl Mutation for MutationAnswer {
    fn into_mutation(self) -> Self {
        self
    }

    fn applied(&mut self, id: u64) {
        match self {
            Self::Rewrite(result) => result.applied(id),
            Self::Rename(result) => result.applied(id),
            Self::Move(result) => result.applied(id),
            Self::MoveSymbol(result) => result.applied(id),
            Self::Batch(result) => result.applied(id),
        }
    }
}

impl From<MutationAnswer> for super::Answer {
    fn from(result: MutationAnswer) -> Self {
        match result {
            MutationAnswer::Rewrite(result) => Self::Rewrite(result),
            MutationAnswer::Rename(result) => Self::Rename(result),
            MutationAnswer::Move(result) => Self::Move(result),
            MutationAnswer::MoveSymbol(result) => Self::MoveSymbol(result),
            MutationAnswer::Batch(result) => Self::Batch(result),
        }
    }
}

impl TryFrom<super::Answer> for MutationAnswer {
    type Error = super::Answer;

    fn try_from(answer: super::Answer) -> Result<Self, Self::Error> {
        match answer {
            super::Answer::Rewrite(result) => Ok(Self::Rewrite(result)),
            super::Answer::Rename(result) => Ok(Self::Rename(result)),
            super::Answer::Move(result) => Ok(Self::Move(result)),
            super::Answer::MoveSymbol(result) => Ok(Self::MoveSymbol(result)),
            super::Answer::Batch(result) => Ok(Self::Batch(result)),
            query => Err(query),
        }
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Rewrite {}
    impl Sealed for super::super::Rename {}
    impl Sealed for super::super::Move {}
    impl Sealed for super::super::MoveSymbol {}
    impl Sealed for super::Batch {}
    impl Sealed for super::MutationAnswer {}
}
