//! What a command proposes, before it is bound to the tree: edits by file,
//! files moved, what a human should look at, and the paths re-spelled.
//!
//! Every operation answers with a [`Change`]; a command merges the changes
//! of its operations into one and hands it to the plan. Edits are kept as
//! proposed and checked for overlap only when the change becomes a
//! [`ChangeSet`], so an operation never has to know what another one did.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use vvv_core::{ChangeSet, Edit, EditConflict};

use crate::plan::{ApplyError, Fingerprint};
use crate::workspace::SourceWitness;

use crate::{Notice, Respelling};

#[derive(Debug, Default)]
pub(crate) struct Change {
    files: BTreeMap<PathBuf, FileEdits>,
    moves: Vec<(PathBuf, PathBuf)>,
    notices: Vec<Notice>,
    respellings: Vec<Respelling>,
}

impl Change {
    pub fn new() -> Self {
        Self::default()
    }

    fn file(&mut self, source: &SourceWitness) -> Result<&mut FileEdits, ApplyError> {
        let file = self
            .files
            .entry(source.path().to_path_buf())
            .or_insert_with(|| FileEdits {
                source: source.clone(),
                edits: Vec::new(),
            });
        file.source.check(source)?;
        Ok(file)
    }

    pub fn edit(&mut self, source: &SourceWitness, edit: Edit) -> Result<(), ApplyError> {
        self.file(source)?.edits.push(edit);
        Ok(())
    }

    pub fn edits(
        &mut self,
        source: &SourceWitness,
        edits: impl IntoIterator<Item = Edit>,
    ) -> Result<(), ApplyError> {
        self.file(source)?.edits.extend(edits);
        Ok(())
    }

    pub fn move_file(
        &mut self,
        source: &SourceWitness,
        to: impl Into<PathBuf>,
    ) -> Result<(), ApplyError> {
        self.file(source)?;
        self.moves.push((source.path().to_path_buf(), to.into()));
        Ok(())
    }

    pub fn notice(&mut self, notice: Notice) {
        self.notices.push(notice);
    }

    pub fn respell(&mut self, respelling: Respelling) {
        self.respellings.push(respelling);
    }

    /// Everything `other` proposes, after what this one already does.
    pub fn merge(&mut self, other: Change) -> Result<(), ApplyError> {
        // Check before merging anything, so a rejected contribution changes nothing.
        for (path, file) in &other.files {
            if let Some(existing) = self.files.get(path) {
                existing.source.check(&file.source)?;
            }
        }
        for (_, file) in other.files {
            self.edits(&file.source, file.edits)?;
        }
        self.moves.extend(other.moves);
        self.notices.extend(other.notices);
        self.respellings.extend(other.respellings);
        Ok(())
    }

    /// Take the edits proposed for `path` out of the change: an operation
    /// that relocates text takes the edits inside it along.
    #[cfg(test)]
    pub fn take_edits(&mut self, path: &Path) -> Vec<Edit> {
        self.files
            .get_mut(path)
            .map(|file| std::mem::take(&mut file.edits))
            .unwrap_or_default()
    }

    /// Bind the proposal to a change set: edits sorted per file, overlaps
    /// refused, moves registered. What is not an edit stays with the change.
    pub fn bind(self) -> Result<Bound, EditConflict> {
        let mut change_set = ChangeSet::new();
        let mut fingerprints = BTreeMap::new();
        for (path, file) in self.files {
            if !file.edits.is_empty() || self.moves.iter().any(|(from, _)| from == &path) {
                fingerprints.insert(path.clone(), file.source.fingerprint().clone());
            }
            for edit in file.edits {
                change_set.insert(&path, edit)?;
            }
        }
        for (from, to) in self.moves {
            change_set.move_file(from, to)?;
        }
        Ok(Bound {
            change_set: WitnessedChangeSet {
                change_set,
                fingerprints,
            },
            notices: self.notices,
            respellings: self.respellings,
        })
    }
}

/// A change bound to a change set, with what the plan does not carry.
pub(crate) struct Bound {
    pub change_set: WitnessedChangeSet,
    pub notices: Vec<Notice>,
    pub respellings: Vec<Respelling>,
}

#[derive(Debug)]
struct FileEdits {
    source: SourceWitness,
    edits: Vec<Edit>,
}

/// A change set whose edited and moved sources all have observed fingerprints.
/// Only binding a witnessed Change can construct a nonempty value.
#[derive(Debug, Default)]
pub(crate) struct WitnessedChangeSet {
    change_set: ChangeSet,
    fingerprints: BTreeMap<PathBuf, Fingerprint>,
}

impl WitnessedChangeSet {
    pub fn into_parts(self) -> (ChangeSet, BTreeMap<PathBuf, Fingerprint>) {
        (self.change_set, self.fingerprints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryVfs, SourceFile, Workspace};
    use std::sync::Arc;
    use vvv_core::Span;

    #[test]
    fn changes_reject_edits_from_different_snapshots_of_one_file() {
        let before = SourceFile::new("a.p", "old");
        let after = SourceFile::new("a.p", "new");
        let mut change = Change::new();
        change
            .edit(before.witness(), Edit::replace(Span::new(0, 3), "kept"))
            .unwrap();
        assert!(matches!(
            change.edit(after.witness(), Edit::insert(0, "wrong")),
            Err(ApplyError::Stale { .. })
        ));
        assert!(matches!(
            change.edits(after.witness(), [Edit::insert(0, "wrong")]),
            Err(ApplyError::Stale { .. })
        ));
        assert!(matches!(
            change.move_file(after.witness(), "b.p"),
            Err(ApplyError::Stale { .. })
        ));
        let mut other = Change::new();
        let unrelated = SourceFile::new("0.p", "other");
        other
            .edit(unrelated.witness(), Edit::insert(0, "wrong"))
            .unwrap();
        other
            .edit(after.witness(), Edit::insert(0, "wrong"))
            .unwrap();
        assert!(matches!(change.merge(other), Err(ApplyError::Stale { .. })));
        let (cs, fingerprints) = change.bind().unwrap().change_set.into_parts();
        assert_eq!(cs.apply_to(Path::new("a.p"), "old").unwrap(), "kept");
        assert_eq!(cs.paths().collect::<Vec<_>>(), [Path::new("a.p")]);
        assert_eq!(fingerprints.len(), 1);
    }

    #[test]
    fn changes_preserve_provenance_when_edits_are_taken() {
        let before = SourceFile::new("a.p", "old");
        let after = SourceFile::new("a.p", "new");
        let mut change = Change::new();
        change
            .edit(before.witness(), Edit::replace(Span::new(0, 3), "kept"))
            .unwrap();
        let edits = change.take_edits(Path::new("a.p"));
        assert!(matches!(
            change.edits(after.witness(), edits.clone()),
            Err(ApplyError::Stale { .. })
        ));
        change.edits(before.witness(), edits).unwrap();
        let plan = crate::plan::Plan::new(change.bind().unwrap().change_set);
        let ws = Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", "new")),
        );
        assert!(matches!(plan.preview(&ws), Err(ApplyError::Stale { .. })));
    }

    #[test]
    fn binding_drops_sources_with_no_remaining_edits_or_moves() {
        let removed = SourceFile::new("a.p", "old");
        let active = SourceFile::new("b.p", "foo");
        let mut change = Change::new();
        change
            .edit(removed.witness(), Edit::delete(Span::new(0, 3)))
            .unwrap();
        change.take_edits(Path::new("a.p"));
        change
            .edit(active.witness(), Edit::replace(Span::new(0, 3), "bar"))
            .unwrap();
        let plan = crate::plan::Plan::new(change.bind().unwrap().change_set);
        let ws = Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/a.p", "changed")
                    .with_file("/ws/b.p", "foo"),
            ),
        );
        let preview = plan.preview(&ws).unwrap();
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].path, Path::new("b.p"));
        assert_eq!(preview.files[0].after, "bar");
    }
}
