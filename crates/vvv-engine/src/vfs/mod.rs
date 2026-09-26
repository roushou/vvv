//! Virtual file system: the only way the engine touches files.
//!
//! Tests run against [`MemoryVfs`]; the CLI runs against [`DiskVfs`]; a plan
//! that has to be planned *against* (the next step of a batch) is applied to
//! an [`Overlay`] first. Because every read and write goes through the trait,
//! the planners never know which.

mod disk;
mod memory;
mod overlay;

use std::path::{Path, PathBuf};

pub use disk::DiskVfs;
pub use memory::MemoryVfs;
pub use overlay::Overlay;

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),
    #[error("destination already exists: {0}")]
    Exists(PathBuf),
    #[error("{path}: not valid UTF-8")]
    InvalidUtf8 { path: PathBuf },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The file effects of a failed move. Unknown outcomes require observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveState {
    Unchanged,
    DestinationLinked,
    Moved,
    Unknown,
}

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct MoveError {
    #[source]
    pub source: VfsError,
    pub state: MoveState,
}

impl MoveError {
    pub fn new(source: VfsError, state: MoveState) -> Self {
        Self { source, state }
    }

    pub fn into_source(self) -> VfsError {
        self.source
    }
}

/// Parent directories created before a file mutation, including partial failure.
#[derive(Debug)]
pub struct ParentCreation {
    pub created: Vec<PathBuf>,
    pub result: Result<(), VfsError>,
}

impl ParentCreation {
    pub fn new(created: Vec<PathBuf>, result: Result<(), VfsError>) -> Self {
        Self { created, result }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Other,
}

/// A cheap witness of a file's state: modification time and size on disk, a
/// version counter in memory. Two equal stamps mean the contents could not
/// have changed, which is what lets a warm corpus skip the read. Like `make`
/// and `git`, it trusts the clock: an edit that keeps the length and lands
/// within the same timestamp tick is invisible until the next one. Writes are
/// never trusted to a stamp — `Plan::apply` checks content fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stamp {
    modified: u128,
    len: u64,
}

impl Stamp {
    pub fn new(modified: u128, len: u64) -> Self {
        Self { modified, len }
    }
}

pub trait Vfs: Send + Sync {
    fn read(&self, path: &Path) -> Result<String, VfsError>;
    /// The file's current [`Stamp`], without reading it.
    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError>;
    /// Write a whole file, creating parent directories as needed.
    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError>;
    fn exists(&self, path: &Path) -> bool;
    /// Inspect a directory entry without following its final symlink.
    fn entry_kind(&self, path: &Path) -> Result<Option<EntryKind>, VfsError>;
    /// Create missing parents and retain every directory created, even on error.
    fn prepare_parent(&self, path: &Path) -> ParentCreation;
    fn remove_file(&self, path: &Path) -> Result<(), VfsError>;
    /// Remove only an empty directory.
    fn remove_empty_dir(&self, path: &Path) -> Result<(), VfsError>;
    /// Move a regular file without replacing an occupied destination.
    /// Prepare destination parents first with `prepare_parent`. Success removes
    /// the source; failure retains its observed effect state for recovery.
    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), MoveError>;
    /// Every regular file under `root`, in a deterministic order.
    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError>;
}
