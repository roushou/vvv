//! The mutation lifecycle: a [`Plan`] is previewed, then consumed by `apply`
//! into a [`Receipt`] that can roll the change back.
//!
//! A plan remembers a fingerprint of every file it touches. Preview and apply
//! refuse to proceed if a file changed since planning, so an agent that runs
//! `rewrite` (preview) and later `rewrite --apply` cannot corrupt edits made
//! in between.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod fingerprint;
mod planned;

pub use fingerprint::Fingerprint;
pub use planned::Planned;

use crate::{VfsError, Workspace};
use vvv_core::ChangeSet;
use vvv_core::RelPath;

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error("{} changed since the plan was made", path.display())]
    Stale { path: RelPath },
    #[error("{} changed since it was written; refusing to undo", path.display())]
    Modified { path: RelPath },
    #[error("writing {} failed; {restored} file(s) restored: {source}", path.display())]
    Write {
        path: RelPath,
        restored: usize,
        #[source]
        source: VfsError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    change_set: ChangeSet,
    fingerprints: BTreeMap<PathBuf, Fingerprint>,
}

impl Plan {
    /// Bind a change set to the current contents of the files it touches.
    pub fn new(change_set: ChangeSet, workspace: &Workspace) -> Result<Self, VfsError> {
        let fingerprints = change_set
            .paths()
            .map(|path| {
                let text = workspace.vfs().read(&workspace.absolute(path))?;
                Ok((path.to_path_buf(), Fingerprint::of(&text)))
            })
            .collect::<Result<_, VfsError>>()?;
        Ok(Self {
            change_set,
            fingerprints,
        })
    }

    pub fn change_set(&self) -> &ChangeSet {
        &self.change_set
    }

    pub fn is_empty(&self) -> bool {
        self.change_set.is_empty()
    }

    pub fn preview(&self, workspace: &Workspace) -> Result<Preview, ApplyError> {
        Ok(Preview {
            files: self.stage(workspace)?,
        })
    }

    pub fn apply(self, workspace: &Workspace) -> Result<Receipt, ApplyError> {
        let staged = self.stage(workspace)?;
        let vfs = workspace.vfs();
        let mut receipt = Receipt::default();
        for file in &staged {
            let result = match &file.moved_to {
                Some(to) => vfs
                    .rename(&workspace.absolute(&file.path), &workspace.absolute(to))
                    .and_then(|()| {
                        receipt
                            .moves
                            .push((file.path.to_path_buf(), to.to_path_buf()));
                        vfs.write(&workspace.absolute(to), &file.after)
                    }),
                None => vfs.write(&workspace.absolute(&file.path), &file.after),
            };
            if let Err(source) = result {
                let restored = receipt.rollback(workspace).unwrap_or(0);
                return Err(ApplyError::Write {
                    path: file.path.clone(),
                    restored,
                    source,
                });
            }
            receipt
                .originals
                .insert(file.path.to_path_buf(), file.before.clone());
            let final_path = file.moved_to.clone().unwrap_or_else(|| file.path.clone());
            receipt
                .written
                .insert(final_path.to_path_buf(), Fingerprint::of(&file.after));
        }
        Ok(receipt)
    }

    /// Read every touched file, check it is unchanged, and compute its new contents.
    fn stage(&self, workspace: &Workspace) -> Result<Vec<FilePreview>, ApplyError> {
        self.fingerprints
            .iter()
            .map(|(path, expected)| {
                let before = workspace.vfs().read(&workspace.absolute(path))?;
                if &Fingerprint::of(&before) != expected {
                    return Err(ApplyError::Stale {
                        path: path.clone().into(),
                    });
                }
                let after = self.change_set.apply_to(path, &before);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    /// Path before the plan runs.
    pub path: RelPath,
    /// Path after, when the plan moves the file.
    pub moved_to: Option<RelPath>,
    pub before: String,
    pub after: String,
}

/// Proof that a plan was applied, holding what is needed to undo it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// Pre-apply contents keyed by pre-apply path.
    originals: BTreeMap<PathBuf, String>,
    /// Moves performed, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    moves: Vec<(PathBuf, PathBuf)>,
    /// What was written, keyed by post-apply path, so an undo can tell
    /// whether the files have been touched since.
    #[serde(default)]
    written: BTreeMap<PathBuf, Fingerprint>,
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
        self
    }

    /// Undo: move files back, then restore every file to its pre-apply
    /// contents. Returns how many files were restored. Does not check that
    /// the files are still as written; see [`Receipt::undo`].
    pub fn rollback(&self, workspace: &Workspace) -> Result<usize, VfsError> {
        let vfs = workspace.vfs();
        for (from, to) in self.moves.iter().rev() {
            vfs.rename(&workspace.absolute(to), &workspace.absolute(from))?;
        }
        for (path, original) in &self.originals {
            vfs.write(&workspace.absolute(path), original)?;
        }
        Ok(self.originals.len())
    }

    /// Roll back only if every file is still exactly as this apply left it.
    pub fn undo(&self, workspace: &Workspace) -> Result<usize, ApplyError> {
        for (path, expected) in &self.written {
            let current = workspace.vfs().read(&workspace.absolute(path))?;
            if &Fingerprint::of(&current) != expected {
                return Err(ApplyError::Modified {
                    path: path.clone().into(),
                });
            }
        }
        Ok(self.rollback(workspace)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::MemoryVfs;
    use vvv_core::{Edit, Span};

    fn workspace() -> Workspace {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a.txt", "hello world")
            .with_file("/ws/b.txt", "foo");
        Workspace::new("/ws", Arc::new(vfs))
    }

    fn plan(ws: &Workspace) -> Plan {
        let mut cs = ChangeSet::new();
        cs.insert("a.txt", Edit::replace(Span::new(0, 5), "goodbye"))
            .unwrap();
        cs.insert("b.txt", Edit::replace(Span::new(0, 3), "bar"))
            .unwrap();
        Plan::new(cs, ws).unwrap()
    }

    #[test]
    fn preview_does_not_write() {
        let ws = workspace();
        let preview = plan(&ws).preview(&ws).unwrap();
        assert_eq!(preview.files[0].after, "goodbye world");
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn apply_then_rollback_round_trips() {
        let ws = workspace();
        let receipt = plan(&ws).apply(&ws).unwrap();
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "goodbye world"
        );
        assert_eq!(ws.vfs().read(Path::new("/ws/b.txt")).unwrap(), "bar");
        assert_eq!(receipt.rollback(&ws).unwrap(), 2);
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn moves_apply_after_edits_and_roll_back_in_reverse() {
        let ws = workspace();
        let mut cs = ChangeSet::new();
        cs.insert("a.txt", Edit::replace(Span::new(0, 5), "moved"))
            .unwrap();
        cs.move_file("a.txt", "dir/z.txt").unwrap();
        let plan = Plan::new(cs, &ws).unwrap();

        let preview = plan.preview(&ws).unwrap();
        assert_eq!(
            preview.files[0].moved_to.as_deref(),
            Some(Path::new("dir/z.txt"))
        );
        assert_eq!(preview.files[0].after, "moved world");

        let receipt = plan.apply(&ws).unwrap();
        assert!(!ws.vfs().exists(Path::new("/ws/a.txt")));
        assert_eq!(
            ws.vfs().read(Path::new("/ws/dir/z.txt")).unwrap(),
            "moved world"
        );

        receipt.rollback(&ws).unwrap();
        assert!(!ws.vfs().exists(Path::new("/ws/dir/z.txt")));
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn undo_refuses_files_edited_after_apply() {
        let ws = workspace();
        let receipt = plan(&ws).apply(&ws).unwrap();
        ws.vfs()
            .write(Path::new("/ws/b.txt"), "edited later")
            .unwrap();
        assert!(
            matches!(receipt.undo(&ws), Err(ApplyError::Modified { path }) if path == Path::new("b.txt"))
        );
        assert_eq!(
            ws.vfs().read(Path::new("/ws/a.txt")).unwrap(),
            "goodbye world",
            "nothing touched"
        );
    }

    #[test]
    fn stale_file_is_refused() {
        let ws = workspace();
        let plan = plan(&ws);
        ws.vfs().write(Path::new("/ws/b.txt"), "changed").unwrap();
        assert!(
            matches!(plan.apply(&ws), Err(ApplyError::Stale { path }) if path == Path::new("b.txt"))
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
        let ws = workspace();
        let mut first = ChangeSet::new();
        first.move_file("a.txt", "moved.txt").unwrap();
        first
            .insert("b.txt", Edit::replace(Span::new(0, 3), "bar"))
            .unwrap();
        let r1 = Plan::new(first, &ws).unwrap().apply(&ws).unwrap();
        let mut second = ChangeSet::new();
        second
            .insert("moved.txt", Edit::replace(Span::new(0, 5), "goodbye"))
            .unwrap();
        second
            .insert("b.txt", Edit::replace(Span::new(0, 3), "baz"))
            .unwrap();
        let r2 = Plan::new(second, &ws).unwrap().apply(&ws).unwrap();
        let read = |p: &str| ws.vfs().read(Path::new(p)).unwrap();
        assert_eq!(read("/ws/moved.txt"), "goodbye world");
        assert_eq!(read("/ws/b.txt"), "baz");

        let chained = r1.then(r2);
        assert_eq!(
            chained.paths().count(),
            2,
            "originals keyed by pre-batch paths"
        );
        chained.undo(&ws).unwrap();
        assert_eq!(read("/ws/a.txt"), "hello world");
        assert_eq!(read("/ws/b.txt"), "foo");
        assert!(!ws.vfs().exists(Path::new("/ws/moved.txt")));
    }
}
