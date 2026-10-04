//! Plan preconditions, staging, and preview. Applying delegates attempted effects
//! and recovery to [`Transaction`], retaining a [`Receipt`] for undo.
//!
//! A plan remembers a fingerprint of every file it touches. Preview and apply
//! refuse to proceed if a file changed since planning. Retained session handles
//! apply the original reviewed plan; separate preview/apply CLI invocations each
//! build a new plan and do not retain the first invocation's edits.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

mod fingerprint;
mod planned;
mod receipt;
mod transaction;

pub use fingerprint::Fingerprint;
pub use planned::Planned;
pub use receipt::Receipt;
pub(crate) use transaction::Transaction;

use crate::protocol::Diff;
use crate::{FileChange, VfsError, Workspace};
use vvv_core::{ChangeSet, Edit, RelPath};

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Edit(#[from] vvv_core::EditConflict),
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error("{} changed since the plan was made", path.display())]
    Stale { path: RelPath },
    #[error("{} already exists", path.display())]
    DestinationExists { path: RelPath },
    #[error("{} changed since it was written; refusing to undo", path.display())]
    Modified { path: RelPath },
    #[error("writing {} failed: {source}", path.display())]
    Write {
        path: RelPath,
        #[source]
        source: VfsError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    change_set: ChangeSet,
    fingerprints: BTreeMap<PathBuf, Fingerprint>,
}

impl Plan {
    /// Retain the source fingerprints observed by the edit producers.
    pub(crate) fn new(change: crate::change::WitnessedChangeSet) -> Self {
        let (change_set, fingerprints) = change.into_parts();
        Self {
            change_set,
            fingerprints,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.change_set.is_empty()
    }

    pub fn preview(&self, workspace: &Workspace) -> Result<Preview, ApplyError> {
        Ok(Preview {
            files: self.stage(workspace)?,
        })
    }

    /// A single plan's preview carries edits in that plan's source coordinates.
    pub(crate) fn file_changes(&self, preview: &Preview) -> Vec<FileChange> {
        preview
            .files
            .iter()
            .map(|file| file.file_change(self.change_set.edits_for(&file.path).to_vec()))
            .collect()
    }

    pub fn apply(self, workspace: &Workspace) -> Result<Receipt, crate::EngineError> {
        let mut transaction = Transaction::new(workspace);
        match transaction.apply(self) {
            Ok(()) => Ok(transaction.receipt()),
            Err(error) => Err(transaction.recover(error.into())),
        }
    }

    /// Read every touched file, check it is unchanged, and compute its new contents.
    fn stage(&self, workspace: &Workspace) -> Result<Vec<FilePreview>, ApplyError> {
        // Check every move before any file is written. This is a preflight
        // condition, not an atomic reservation against external writers.
        for (from, to) in self.change_set.moves() {
            if workspace
                .vfs()
                .entry_kind(&workspace.absolute(to))?
                .is_some()
                && !workspace
                    .vfs()
                    .same_entry(&workspace.absolute(from), &workspace.absolute(to))?
            {
                return Err(ApplyError::DestinationExists { path: to.into() });
            }
        }
        self.fingerprints
            .iter()
            .map(|(path, expected)| {
                let before = workspace.vfs().read(&workspace.absolute(path))?;
                if &Fingerprint::of(&before) != expected {
                    return Err(ApplyError::Stale {
                        path: path.clone().into(),
                    });
                }
                let after = self.change_set.apply_to(path, &before)?;
                Ok(FilePreview {
                    path: path.clone().into(),
                    moved_to: self.change_set.destination(path).map(Into::into),
                    before,
                    after,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub files: Vec<FilePreview>,
}

impl Preview {
    /// A combined preview has whole-file diffs. Its steps own their edits,
    /// each in the coordinates of the source before that step.
    pub(crate) fn file_changes(&self) -> Vec<FileChange> {
        self.files
            .iter()
            .map(|file| file.file_change(Vec::new()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    /// Path before the plan runs.
    pub path: RelPath,
    /// Path after, when the plan moves the file.
    pub moved_to: Option<RelPath>,
    pub before: String,
    pub after: String,
}

impl FilePreview {
    fn file_change(&self, edits: Vec<Edit>) -> FileChange {
        FileChange {
            path: self.path.clone(),
            moved_to: self.moved_to.clone(),
            edits,
            diff: Diff::between(
                &self.path,
                self.moved_to.as_deref().unwrap_or(&self.path),
                &self.before,
                &self.after,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::MemoryVfs;
    use vvv_core::{Edit, Span};

    struct Fixture {
        workspace: Workspace,
    }

    impl Fixture {
        fn new() -> Self {
            let vfs = MemoryVfs::new()
                .with_file("/ws/a.txt", "hello world")
                .with_file("/ws/b.txt", "foo");
            Self {
                workspace: Workspace::new("/ws", Arc::new(vfs)),
            }
        }

        fn plan(&self, cs: ChangeSet) -> Plan {
            let mut change = crate::change::Change::new();
            for path in cs.paths() {
                let file = self.workspace.load(path).unwrap();
                change
                    .edits(file.witness(), cs.edits_for(path).iter().cloned())
                    .unwrap();
                if let Some(to) = cs.destination(path) {
                    change.move_file(file.witness(), to).unwrap();
                }
            }
            Plan::new(change.bind().unwrap().change_set)
        }

        fn replacement(&self) -> Plan {
            let mut cs = ChangeSet::new();
            cs.insert("a.txt", Edit::replace(Span::new(0, 5), "goodbye"))
                .unwrap();
            cs.insert("b.txt", Edit::replace(Span::new(0, 3), "bar"))
                .unwrap();
            self.plan(cs)
        }
    }

    #[test]
    fn preview_does_not_write() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let preview = fixture.replacement().preview(ws).unwrap();
        assert_eq!(preview.files[0].after, "goodbye world");
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn preview_refuses_a_newly_occupied_destination() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let mut cs = ChangeSet::new();
        cs.move_file("a.txt", "new.txt").unwrap();
        let plan = fixture.plan(cs);
        ws.vfs()
            .write(Path::new("/ws/new.txt"), "precious new file")
            .unwrap();
        assert!(
            matches!(plan.preview(ws), Err(ApplyError::DestinationExists { path }) if path == Path::new("new.txt"))
        );
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
        assert_eq!(
            ws.vfs().read(Path::new("/ws/new.txt")).unwrap(),
            "precious new file"
        );
    }

    #[test]
    fn apply_then_rollback_round_trips() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let receipt = fixture.replacement().apply(ws).unwrap();
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "goodbye world"
        );
        assert_eq!(ws.vfs().read(Path::new("/ws/b.txt")).unwrap(), "bar");
        assert_eq!(receipt.rollback(ws).unwrap(), 2);
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn moves_apply_after_edits_and_roll_back_in_reverse() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let mut cs = ChangeSet::new();
        cs.insert("a.txt", Edit::replace(Span::new(0, 5), "moved"))
            .unwrap();
        cs.move_file("a.txt", "dir/z.txt").unwrap();
        let plan = fixture.plan(cs);

        let preview = plan.preview(ws).unwrap();
        assert_eq!(
            preview.files[0].moved_to.as_deref(),
            Some(Path::new("dir/z.txt"))
        );
        assert_eq!(preview.files[0].after, "moved world");

        let receipt = plan.apply(ws).unwrap();
        assert!(!ws.vfs().exists(Path::new("/ws/a.txt")));
        assert_eq!(
            ws.vfs().read(Path::new("/ws/dir/z.txt")).unwrap(),
            "moved world"
        );

        receipt.rollback(ws).unwrap();
        assert!(!ws.vfs().exists(Path::new("/ws/dir/z.txt")));
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn undo_refuses_files_edited_after_apply() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let receipt = fixture.replacement().apply(ws).unwrap();
        ws.vfs()
            .write(Path::new("/ws/b.txt"), "edited later")
            .unwrap();
        assert!(
            matches!(receipt.undo(ws), Err(crate::EngineError::Apply(ApplyError::Modified { path })) if path == Path::new("b.txt"))
        );
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "goodbye world",
            "nothing touched"
        );
    }

    #[test]
    fn stale_file_is_refused() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let plan = fixture.replacement();
        ws.vfs().write(Path::new("/ws/b.txt"), "changed").unwrap();
        assert!(
            matches!(plan.apply(ws), Err(crate::EngineError::Apply(ApplyError::Stale { path })) if path == Path::new("b.txt"))
        );
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    /// Two applies chained: the second edits a file the first moved. Rolling
    /// the chained receipt back restores the tree exactly.
    #[test]
    fn chained_receipts_roll_back_both_steps() {
        let fixture = Fixture::new();
        let ws = &fixture.workspace;
        let mut first = ChangeSet::new();
        first.move_file("a.txt", "moved.txt").unwrap();
        first
            .insert("b.txt", Edit::replace(Span::new(0, 3), "bar"))
            .unwrap();
        let r1 = fixture.plan(first).apply(ws).unwrap();
        let mut second = ChangeSet::new();
        second
            .insert("moved.txt", Edit::replace(Span::new(0, 5), "goodbye"))
            .unwrap();
        second
            .insert("b.txt", Edit::replace(Span::new(0, 3), "baz"))
            .unwrap();
        let r2 = fixture.plan(second).apply(ws).unwrap();
        let read = |p: &str| ws.vfs().read(Path::new(p)).unwrap();
        assert_eq!(read("/ws/moved.txt"), "goodbye world");
        assert_eq!(read("/ws/b.txt"), "baz");

        let chained = r1.then(r2);
        assert_eq!(
            chained.paths().count(),
            2,
            "originals keyed by pre-batch paths"
        );
        chained.undo(ws).unwrap();
        assert_eq!(read("/ws/a.txt"), "hello world");
        assert_eq!(read("/ws/b.txt"), "foo");
        assert!(!ws.vfs().exists(Path::new("/ws/moved.txt")));
    }
}
