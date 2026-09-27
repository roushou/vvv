//! Applied before-states, receipt composition, and undo validation/restoration.
use super::{ApplyError, Fingerprint, Transaction};
#[cfg(test)]
use crate::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use vvv_core::RelPath;

/// Proof that a plan was applied, holding what is needed to undo it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// Pre-apply contents keyed by pre-apply path.
    pub(super) originals: BTreeMap<PathBuf, String>,
    /// Moves performed, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) moves: Vec<(PathBuf, PathBuf)>,
    /// What was written, keyed by post-apply path, so an undo can tell
    /// whether the files have been touched since.
    #[serde(default)]
    pub(super) written: BTreeMap<PathBuf, Fingerprint>,
    /// Only directories actually created by the file plans, in creation order.
    /// Receipts without ownership evidence retain directories during undo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) directories: Vec<RelPath>,
}

impl Receipt {
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.originals.keys().map(PathBuf::as_path)
    }

    pub fn moves(&self) -> &[(PathBuf, PathBuf)] {
        &self.moves
    }

    /// This apply followed by `next`, as one receipt: rolling it back undoes
    /// both, checking it checks the files as `next` left them. Originals are
    /// the first ones seen for a file; a path `next` touched that this apply
    /// had already written keeps this apply's original.
    pub fn then(mut self, next: Receipt) -> Receipt {
        let written_here: BTreeSet<PathBuf> = self.written.keys().cloned().collect();
        for (path, original) in next.originals {
            if !written_here.contains(&path) {
                self.originals.entry(path).or_insert(original);
            }
        }
        let moved_away: BTreeSet<&PathBuf> = next.moves.iter().map(|(from, _)| from).collect();
        self.written.retain(|path, _| !moved_away.contains(path));
        self.written.extend(next.written);
        self.moves.extend(next.moves);
        self.directories.extend(next.directories);
        self
    }

    fn rollback_in(&self, transaction: &mut Transaction<'_>) -> Result<usize, ApplyError> {
        for (from, to) in self.moves.iter().rev() {
            let before = transaction
                .workspace
                .vfs()
                .read(&transaction.workspace.absolute(to))?;
            transaction.move_file(&to.clone().into(), &from.clone().into(), &before)?;
        }
        for (path, original) in &self.originals {
            let before = transaction
                .workspace
                .vfs()
                .read(&transaction.workspace.absolute(path))?;
            transaction.write(&path.clone().into(), &before, original)?;
        }
        for directory in self.directories.iter().rev() {
            transaction.remove_owned_directory(directory)?;
        }
        Ok(self.originals.len())
    }

    /// Keep undo's effects live until its caller also saves the history ledger.
    pub(crate) fn undo_in(&self, transaction: &mut Transaction<'_>) -> Result<usize, ApplyError> {
        for (path, expected) in &self.written {
            let current = transaction
                .workspace
                .vfs()
                .read(&transaction.workspace.absolute(path))?;
            if &Fingerprint::of(&current) != expected {
                return Err(ApplyError::Modified {
                    path: path.clone().into(),
                });
            }
        }
        self.rollback_in(transaction)
    }

    #[cfg(test)]
    pub(super) fn rollback(&self, workspace: &Workspace) -> Result<usize, crate::EngineError> {
        let mut transaction = Transaction::new(workspace);
        match self.rollback_in(&mut transaction) {
            Ok(restored) => Ok(restored),
            Err(error) => Err(transaction.recover(error.into())),
        }
    }

    #[cfg(test)]
    pub(super) fn undo(&self, workspace: &Workspace) -> Result<usize, crate::EngineError> {
        let mut transaction = Transaction::new(workspace);
        match self.undo_in(&mut transaction) {
            Ok(restored) => Ok(restored),
            Err(error) => Err(transaction.recover(error.into())),
        }
    }
}
