//! What a command proposes, before it is bound to the tree: edits by file,
//! files moved, what a human should look at, and the paths re-spelled.
//!
//! Every operation answers with a [`Change`]; a command merges the changes
//! of its operations into one and hands it to the plan. Edits are kept as
//! proposed and checked for overlap only when the change becomes a
//! [`ChangeSet`], so an operation never has to know what another one did.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vvv_core::{ChangeSet, Edit, EditConflict};

use crate::{Notice, Respelling};

#[derive(Debug, Default)]
pub(crate) struct Change {
    edits: BTreeMap<PathBuf, Vec<Edit>>,
    moves: Vec<(PathBuf, PathBuf)>,
    notices: Vec<Notice>,
    respellings: Vec<Respelling>,
}

impl Change {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn edit(&mut self, path: impl Into<PathBuf>, edit: Edit) {
        self.edits.entry(path.into()).or_default().push(edit);
    }

    pub fn edits(&mut self, path: &Path, edits: impl IntoIterator<Item = Edit>) {
        self.edits
            .entry(path.to_path_buf())
            .or_default()
            .extend(edits);
    }

    pub fn move_file(&mut self, from: impl Into<PathBuf>, to: impl Into<PathBuf>) {
        self.moves.push((from.into(), to.into()));
    }

    pub fn notice(&mut self, notice: Notice) {
        self.notices.push(notice);
    }

    pub fn respell(&mut self, respelling: Respelling) {
        self.respellings.push(respelling);
    }

    /// Keep only the respellings `keep` accepts.
    pub fn retain_respellings(&mut self, keep: impl FnMut(&Respelling) -> bool) {
        self.respellings.retain(keep);
    }

    /// Everything `other` proposes, after what this one already does.
    pub fn merge(&mut self, other: Change) {
        for (path, edits) in other.edits {
            self.edits.entry(path).or_default().extend(edits);
        }
        self.moves.extend(other.moves);
        self.notices.extend(other.notices);
        self.respellings.extend(other.respellings);
    }

    /// Take the edits proposed for `path` out of the change: an operation
    /// that relocates text takes the edits inside it along.
    pub fn take_edits(&mut self, path: &Path) -> Vec<Edit> {
        self.edits.remove(path).unwrap_or_default()
    }

    /// Bind the proposal to a change set: edits sorted per file, overlaps
    /// refused, moves registered. What is not an edit stays with the change.
    pub fn bind(self) -> Result<Bound, EditConflict> {
        let mut change_set = ChangeSet::new();
        for (path, edits) in self.edits {
            for edit in edits {
                change_set.insert(&path, edit)?;
            }
        }
        for (from, to) in self.moves {
            change_set.move_file(from, to)?;
        }
        Ok(Bound {
            change_set,
            notices: self.notices,
            respellings: self.respellings,
        })
    }
}

/// A change bound to a change set, with what the plan does not carry.
pub(crate) struct Bound {
    pub change_set: ChangeSet,
    pub notices: Vec<Notice>,
    pub respellings: Vec<Respelling>,
}
