//! Attempted file effects and verified recovery across plans and the history save.
use super::{ApplyError, Fingerprint, Plan, Receipt};
use crate::{VfsError, Workspace};
use std::collections::BTreeMap;
use std::path::Path;
use vvv_core::RelPath;

/// In-memory recovery state for every attempted effect across a set of plans.
/// It is deliberately not a durable journal and does not recover from a crash.
pub(crate) struct Transaction<'a> {
    pub(super) workspace: &'a Workspace,
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
        let start = self.effects.len();
        let mut receipt = self.apply_plan(plan)?;
        receipt
            .directories
            .extend(self.effects[start..].iter().filter_map(|effect| {
                if let Effect::Directory(path) = effect {
                    Some(path.clone())
                } else {
                    None
                }
            }));
        self.receipts.push(receipt);
        Ok(())
    }

    fn apply_plan(&mut self, plan: Plan) -> Result<Receipt, ApplyError> {
        let staged = plan.stage(self.workspace)?;
        let mut receipt = Receipt::default();
        for file in &staged {
            // Preserve the actual stored source spelling in history, even if
            // the planned source has become a case alias before apply.
            let original_path: RelPath = if file.moved_to.is_some() {
                let stored = self
                    .workspace
                    .vfs()
                    .entry_path(&self.workspace.absolute(&file.path))?
                    .ok_or_else(|| VfsError::NotFound(self.workspace.absolute(&file.path)))?;
                self.workspace.relative(&stored).into()
            } else {
                file.path.clone()
            };
            receipt
                .originals
                .insert(original_path.to_path_buf(), file.before.clone());
            let final_path = if let Some(to) = &file.moved_to {
                self.move_file(&original_path, to, &file.before)?;
                receipt
                    .moves
                    .push((original_path.to_path_buf(), to.to_path_buf()));
                to
            } else {
                &file.path
            };
            self.write(final_path, &file.before, &file.after)?;
            receipt
                .written
                .insert(final_path.to_path_buf(), Fingerprint::of(&file.after));
        }
        Ok(receipt)
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

    pub(super) fn remove_owned_directory(&mut self, path: &RelPath) -> Result<(), VfsError> {
        let absolute = self.workspace.absolute(path);
        match self.workspace.vfs().entry_kind(&absolute)? {
            None => return Ok(()),
            Some(crate::EntryKind::Directory) => {}
            Some(_) => return Err(VfsError::Exists(absolute)),
        }
        self.originals
            .entry(path.clone())
            .or_insert(Original::Directory);
        self.effects.push(Effect::RemovedDirectory(path.clone()));
        match self.workspace.vfs().remove_empty_dir(&absolute) {
            // A directory containing another file is retained; only empty owned
            // directories are cleanup targets, even if a file arrives mid-check.
            Err(VfsError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::DirectoryNotEmpty =>
            {
                Ok(())
            }
            outcome => outcome,
        }
    }

    pub(super) fn write(
        &mut self,
        path: &RelPath,
        before: &str,
        after: &str,
    ) -> Result<(), ApplyError> {
        self.prepare_parent(path)?;
        self.write_existing(path, before, after)
            .map_err(|source| ApplyError::Write {
                path: path.clone(),
                source,
            })
    }

    fn write_existing(
        &mut self,
        path: &RelPath,
        before: &str,
        after: &str,
    ) -> Result<(), VfsError> {
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
    }

    /// Keep the ledger's exact prior contents (or absence) in the same effect log
    /// as the files, before attempting a potentially partial history write.
    pub(crate) fn save_file(
        &mut self,
        path: &RelPath,
        before: Option<&str>,
        after: &str,
    ) -> Result<(), VfsError> {
        self.prepare_parent(path)?;
        if let Some(before) = before {
            return self.write_existing(path, before, after);
        }
        self.originals
            .entry(path.clone())
            .or_insert(Original::Absent);
        self.effects.push(Effect::CreateFile(path.clone()));
        self.workspace
            .vfs()
            .write(&self.workspace.absolute(path), after)
    }

    pub(super) fn move_file(
        &mut self,
        from: &RelPath,
        to: &RelPath,
        before: &str,
    ) -> Result<(), ApplyError> {
        if !self
            .workspace
            .vfs()
            .same_entry(&self.workspace.absolute(from), &self.workspace.absolute(to))?
        {
            return self.move_entry(from, to, before);
        }
        let stored = self
            .workspace
            .vfs()
            .entry_path(&self.workspace.absolute(from))?
            .ok_or_else(|| VfsError::NotFound(self.workspace.absolute(from)))?;
        let original = match self.originals.get(from) {
            Some(Original::File(contents)) => Original::SpelledFile {
                contents: contents.clone(),
                spelling: self.workspace.relative(&stored).into(),
            },
            Some(original) => original.clone(),
            None => Original::SpelledFile {
                contents: before.to_owned(),
                spelling: self.workspace.relative(&stored).into(),
            },
        };
        self.originals.insert(from.clone(), original.clone());
        self.originals.entry(to.clone()).or_insert(original);
        static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        for _ in 0..32 {
            let id = IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let name = format!(".vvv-move-{}-{id}.tmp", std::process::id());
            let temporary: RelPath = from.parent().unwrap_or(Path::new("")).join(name).into();
            match self.move_entry(from, &temporary, before) {
                Err(ApplyError::Vfs(VfsError::Exists(_)))
                    if matches!(
                        self.effects.last(),
                        Some(Effect::Move {
                            state: crate::MoveState::Unchanged,
                            ..
                        })
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
                Ok(()) => return self.move_entry(&temporary, to, before),
            }
        }
        Err(VfsError::Io {
            path: self.workspace.absolute(from),
            source: std::io::Error::other(
                "could not acquire a temporary name for the case-only move",
            ),
        }
        .into())
    }

    fn move_entry(&mut self, from: &RelPath, to: &RelPath, before: &str) -> Result<(), ApplyError> {
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
            match expected.observe_like(self.workspace, &path) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Original {
    Absent,
    File(String),
    SpelledFile { contents: String, spelling: RelPath },
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

    fn observe_like(&self, workspace: &Workspace, path: &RelPath) -> Result<Self, VfsError> {
        let observed = Self::observe(workspace, path)?;
        if let (Self::SpelledFile { .. }, Self::File(contents)) = (self, &observed) {
            let absolute = workspace.absolute(path);
            let stored = workspace
                .vfs()
                .entry_path(&absolute)?
                .ok_or(VfsError::NotFound(absolute))?;
            return Ok(Self::SpelledFile {
                contents: contents.clone(),
                spelling: workspace.relative(&stored).into(),
            });
        }
        Ok(observed)
    }

    fn state(&self) -> crate::RecoveryState {
        match self {
            Self::Absent => crate::RecoveryState::Absent,
            Self::File(text) => crate::RecoveryState::File {
                fingerprint: Fingerprint::of(text).as_str().to_owned(),
                spelling: None,
            },
            Self::SpelledFile { contents, spelling } => crate::RecoveryState::File {
                fingerprint: Fingerprint::of(contents).as_str().to_owned(),
                spelling: Some(spelling.clone()),
            },
            Self::Directory => crate::RecoveryState::Directory,
            Self::Other => crate::RecoveryState::Other,
        }
    }
}

enum Effect {
    CreateFile(RelPath),
    RemovedDirectory(RelPath),
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
            Self::Write { path, .. }
            | Self::Directory(path)
            | Self::CreateFile(path)
            | Self::RemovedDirectory(path) => path,
            Self::Move { from, .. } => from,
        }
    }

    fn operation(&self) -> crate::RecoveryOperation {
        match self {
            Self::Write { .. } => crate::RecoveryOperation::RestoreFile,
            Self::CreateFile(_) => crate::RecoveryOperation::RemoveFile,
            Self::Move { .. } => crate::RecoveryOperation::RestoreMove,
            Self::Directory(_) => crate::RecoveryOperation::RemoveDirectory,
            Self::RemovedDirectory(_) => crate::RecoveryOperation::RestoreDirectory,
        }
    }

    fn restore(&self, workspace: &Workspace) -> Result<(), crate::RecoveryIssue> {
        let vfs = workspace.vfs();
        let mut operation = self.operation();
        let mut failed_path = self.path();
        let result = (|| -> Result<(), VfsError> {
            match self {
                Self::RemovedDirectory(path) => match vfs.entry_kind(&workspace.absolute(path))? {
                    Some(crate::EntryKind::Directory) => Ok(()),
                    None => vfs.create_dir(&workspace.absolute(path)),
                    Some(_) => Err(VfsError::Exists(workspace.absolute(path))),
                },
                Self::CreateFile(path) => match vfs.entry_kind(&workspace.absolute(path))? {
                    None => Ok(()),
                    Some(crate::EntryKind::File) => vfs.remove_file(&workspace.absolute(path)),
                    Some(_) => Err(VfsError::Exists(workspace.absolute(path))),
                },
                Self::Write { path, before } => {
                    if Original::observe(workspace, path)
                        .is_ok_and(|observed| observed == Original::File(before.clone()))
                    {
                        return Ok(());
                    }
                    // A partial write may leave unreadable text. The retained
                    // original is enough to attempt restoration without a reload.
                    vfs.write(&workspace.absolute(path), before)
                }
                Self::Move {
                    state: crate::MoveState::Unchanged,
                    ..
                } => Ok(()),
                Self::Move {
                    state: crate::MoveState::Unknown,
                    from,
                    ..
                } => Err(VfsError::Io {
                    path: workspace.absolute(from),
                    source: std::io::Error::other(
                        "move outcome is unknown; recovery cannot acquire or remove its destination",
                    ),
                }),
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
