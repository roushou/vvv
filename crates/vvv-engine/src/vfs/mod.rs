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
    #[error("{path}: not valid UTF-8")]
    InvalidUtf8 { path: PathBuf },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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
    /// Move a file, creating parent directories of `to` as needed.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), VfsError>;
    /// Every regular file under `root`, in a deterministic order.
    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError>;
}
