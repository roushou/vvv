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
    /// Destinations that must still be absent when the plan stages.
    absent: BTreeSet<RelPath>,
}

impl Plan {
    /// Retain the source fingerprints observed by the edit producers.
    pub(crate) fn new(change: crate::change::WitnessedChangeSet) -> Self {
        let (change_set, fingerprints) = change.into_parts();
        let absent = change_set.moves().map(|(_, to)| to.into()).collect();
        Self {
            change_set,
            fingerprints,
            absent,
        }
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

    pub fn apply(self, workspace: &Workspace) -> Result<Receipt, crate::EngineError> {
        let mut transaction = Transaction::new(workspace);
        match transaction.apply(self) {
            Ok(()) => Ok(transaction.receipt()),
            Err(error) => Err(transaction.recover(error.into())),
        }
    }

    fn apply_in(self, transaction: &mut Transaction<'_>) -> Result<Receipt, ApplyError> {
        let staged = self.stage(transaction.workspace)?;
        let mut receipt = Receipt::default();
        for file in &staged {
            // Retain the original before either the move or the write is attempted.
            receipt
                .originals
                .insert(file.path.to_path_buf(), file.before.clone());
            let final_path = if let Some(to) = &file.moved_to {
                transaction.move_file(&file.path, to, &file.before)?;
                receipt
                    .moves
                    .push((file.path.to_path_buf(), to.to_path_buf()));
                to
            } else {
                &file.path
            };
            transaction.write(final_path, &file.before, &file.after)?;
            receipt
                .written
                .insert(final_path.to_path_buf(), Fingerprint::of(&file.after));
        }
        Ok(receipt)
    }

    /// Read every touched file, check it is unchanged, and compute its new contents.
    fn stage(&self, workspace: &Workspace) -> Result<Vec<FilePreview>, ApplyError> {
        // Check every move before any file is written. This is a preflight
        // condition, not an atomic reservation against external writers.
        for path in &self.absent {
            if workspace.vfs().exists(&workspace.absolute(path)) {
                return Err(ApplyError::DestinationExists { path: path.clone() });
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
            vfs.move_if_absent(&workspace.absolute(to), &workspace.absolute(from))
                .map_err(crate::MoveError::into_source)?;
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

/// In-memory recovery state for every attempted effect across a set of plans.
/// It is deliberately not a durable journal and does not recover from a crash.
pub(crate) struct Transaction<'a> {
    workspace: &'a Workspace,
    effects: Vec<Effect>,
    originals: BTreeMap<RelPath, Original>,
    receipts: Vec<Receipt>,
}

impl<'a> Transaction<'a> {
    pub(crate) fn new(workspace: &'a Workspace) -> Self {
        Self {
            workspace,
            effects: Vec::new(),
            originals: BTreeMap::new(),
            receipts: Vec::new(),
        }
    }

    pub(crate) fn apply(&mut self, plan: Plan) -> Result<(), ApplyError> {
        let receipt = plan.apply_in(self)?;
        self.receipts.push(receipt);
        Ok(())
    }

    pub(crate) fn receipt(&self) -> Receipt {
        self.receipts
            .iter()
            .cloned()
            .fold(Receipt::default(), Receipt::then)
    }

    fn prepare_parent(&mut self, path: &RelPath) -> Result<(), VfsError> {
        let parents = self
            .workspace
            .vfs()
            .prepare_parent(&self.workspace.absolute(path));
        for absolute in parents.created {
            let path: RelPath = self.workspace.relative(&absolute).into();
            self.originals
                .entry(path.clone())
                .or_insert(Original::Absent);
            self.effects.push(Effect::Directory(path));
        }
        parents.result
    }

    fn write(&mut self, path: &RelPath, before: &str, after: &str) -> Result<(), ApplyError> {
        self.prepare_parent(path)?;
        self.originals
            .entry(path.clone())
            .or_insert_with(|| Original::File(before.to_owned()));
        self.effects.push(Effect::Write {
            path: path.clone(),
            before: before.to_owned(),
        });
        self.workspace
            .vfs()
            .write(&self.workspace.absolute(path), after)
            .map_err(|source| ApplyError::Write {
                path: path.clone(),
                source,
            })
    }

    fn move_file(&mut self, from: &RelPath, to: &RelPath, before: &str) -> Result<(), ApplyError> {
        self.prepare_parent(to)?;
        self.originals
            .entry(from.clone())
            .or_insert_with(|| Original::File(before.to_owned()));
        let index = self.effects.len();
        self.effects.push(Effect::Move {
            from: from.clone(),
            to: to.clone(),
            before: before.to_owned(),
            state: crate::MoveState::Unknown,
        });
        let outcome = self
            .workspace
            .vfs()
            .move_if_absent(&self.workspace.absolute(from), &self.workspace.absolute(to));
        let state = outcome
            .as_ref()
            .map_or_else(|error| error.state, |_| crate::MoveState::Moved);
        if let Effect::Move {
            state: recorded, ..
        } = &mut self.effects[index]
        {
            *recorded = state;
        }
        // An unchanged failure has not acquired the destination. In particular,
        // a racing creator owns it even when its contents equal the source.
        if state != crate::MoveState::Unchanged {
            self.originals.entry(to.clone()).or_insert(Original::Absent);
        }
        outcome.map_err(|error| ApplyError::Vfs(error.into_source()))
    }

    pub(crate) fn recover(mut self, cause: crate::EngineError) -> crate::EngineError {
        let mut failures = Vec::new();
        for effect in self.effects.iter().rev() {
            if let Err(issue) = effect.restore(self.workspace) {
                failures.push(issue);
            }
        }
        let mut remaining = Vec::new();
        let mut unverified = Vec::new();
        for (path, expected) in std::mem::take(&mut self.originals) {
            match Original::observe(self.workspace, &path) {
                Ok(observed) if observed != expected => remaining.push(crate::RecoveryEffect {
                    path,
                    expected: expected.state(),
                    observed: observed.state(),
                }),
                Ok(_) => {}
                Err(error) => unverified.push(crate::RecoveryUnverified {
                    expected: expected.state(),
                    path,
                    code: crate::ErrorCode::Io,
                    message: error.to_string(),
                }),
            }
        }
        if remaining.is_empty() && unverified.is_empty() {
            return cause;
        }
        crate::RecoveryError {
            details: crate::Recovery {
                cause: Box::new(crate::Failure::from(&cause)),
                failures,
                remaining,
                unverified,
            },
            cause: Box::new(cause),
        }
        .into()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Original {
    Absent,
    File(String),
    Directory,
    Other,
}

impl Original {
    fn observe(workspace: &Workspace, path: &RelPath) -> Result<Self, VfsError> {
        let absolute = workspace.absolute(path);
        match workspace.vfs().entry_kind(&absolute)? {
            None => Ok(Self::Absent),
            Some(crate::EntryKind::File) => Ok(Self::File(workspace.vfs().read(&absolute)?)),
            Some(crate::EntryKind::Directory) => Ok(Self::Directory),
            Some(crate::EntryKind::Other) => Ok(Self::Other),
        }
    }

    fn state(&self) -> crate::RecoveryState {
        match self {
            Self::Absent => crate::RecoveryState::Absent,
            Self::File(text) => crate::RecoveryState::File {
                fingerprint: Fingerprint::of(text).as_str().to_owned(),
            },
            Self::Directory => crate::RecoveryState::Directory,
            Self::Other => crate::RecoveryState::Other,
        }
    }
}

enum Effect {
    Write {
        path: RelPath,
        before: String,
    },
    Move {
        from: RelPath,
        to: RelPath,
        before: String,
        state: crate::MoveState,
    },
    Directory(RelPath),
}

impl Effect {
    fn path(&self) -> &RelPath {
        match self {
            Self::Write { path, .. } | Self::Directory(path) => path,
            Self::Move { from, .. } => from,
        }
    }

    fn operation(&self) -> crate::RecoveryOperation {
        match self {
            Self::Write { .. } => crate::RecoveryOperation::RestoreFile,
            Self::Move { .. } => crate::RecoveryOperation::RestoreMove,
            Self::Directory(_) => crate::RecoveryOperation::RemoveDirectory,
        }
    }

    fn restore(&self, workspace: &Workspace) -> Result<(), crate::RecoveryIssue> {
        let vfs = workspace.vfs();
        let mut operation = self.operation();
        let mut failed_path = self.path();
        let result = (|| -> Result<(), VfsError> {
            match self {
                Self::Write { path, before } => {
                    if Original::observe(workspace, path)? == Original::File(before.clone()) {
                        return Ok(());
                    }
                    vfs.write(&workspace.absolute(path), before)
                }
                Self::Move {
                    state: crate::MoveState::Unchanged,
                    ..
                } => Ok(()),
                Self::Move {
                    from,
                    to,
                    before,
                    state,
                } => match Original::observe(workspace, from)? {
                    Original::Absent => {
                        if Original::observe(workspace, to)? != Original::Absent {
                            vfs.move_if_absent(&workspace.absolute(to), &workspace.absolute(from))
                                .map_err(crate::MoveError::into_source)?;
                        }
                        if Original::observe(workspace, from)? != Original::File(before.clone()) {
                            vfs.write(&workspace.absolute(from), before)?;
                        }
                        Ok(())
                    }
                    Original::File(text) if text == *before => {
                        match Original::observe(workspace, to)? {
                            Original::Absent => Ok(()),
                            Original::File(text)
                                if text == *before
                                    && *state == crate::MoveState::DestinationLinked =>
                            {
                                operation = crate::RecoveryOperation::RemoveFile;
                                failed_path = to;
                                vfs.remove_file(&workspace.absolute(to))
                            }
                            _ => Err(VfsError::Io {
                                path: workspace.absolute(to),
                                source: std::io::Error::new(
                                    std::io::ErrorKind::AlreadyExists,
                                    "recovery destination has different contents",
                                ),
                            }),
                        }
                    }
                    _ => Err(VfsError::Io {
                        path: workspace.absolute(from),
                        source: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "recovery refuses to overwrite an occupied source",
                        ),
                    }),
                },
                Self::Directory(path) => match vfs.entry_kind(&workspace.absolute(path))? {
                    None => Ok(()),
                    Some(_) => vfs.remove_empty_dir(&workspace.absolute(path)),
                },
            }
        })();
        result.map_err(|error| crate::RecoveryIssue {
            operation,
            path: failed_path.clone(),
            code: crate::ErrorCode::Io,
            message: error.to_string(),
        })
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
            matches!(receipt.undo(ws), Err(ApplyError::Modified { path }) if path == Path::new("b.txt"))
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
