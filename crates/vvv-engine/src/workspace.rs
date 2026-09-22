//! A root directory viewed through a [`Vfs`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{DiskVfs, Overlay, Vfs, VfsError};
use vvv_core::SourceText;

#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
    vfs: Arc<dyn Vfs>,
}

impl Workspace {
    /// The tree under `root` on disk, `root` made absolute.
    pub fn disk(root: impl AsRef<Path>) -> Result<Self, VfsError> {
        let root = root.as_ref();
        let root = root.canonicalize().map_err(|source| VfsError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Ok(Self::new(root, Arc::new(DiskVfs::new())))
    }

    pub fn new(root: impl Into<PathBuf>, vfs: Arc<dyn Vfs>) -> Self {
        Self {
            root: root.into(),
            vfs,
        }
    }

    /// This workspace as a plan would leave it: the same root over an
    /// overlay of its file system. Writes land in the overlay; the real
    /// files are untouched until a plan is applied here.
    pub fn staged(&self) -> Workspace {
        Workspace::new(self.root.clone(), Arc::new(Overlay::over(self.vfs.clone())))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn vfs(&self) -> &dyn Vfs {
        self.vfs.as_ref()
    }

    pub fn files(&self) -> Result<Vec<PathBuf>, VfsError> {
        self.vfs.walk(&self.root)
    }

    /// Load a file given either an absolute path or one relative to the root.
    pub fn load(&self, path: &Path) -> Result<SourceFile, VfsError> {
        let text = self.vfs.read(&self.absolute(path))?;
        Ok(SourceFile::new(self.relative(path), text))
    }

    /// A user-supplied path (absolute, `./x`, `a/../b`) as a clean root-relative path.
    pub fn normalize(&self, path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in self.absolute(path).components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        self.relative(&out)
    }

    /// Path as handed to the [`Vfs`].
    pub fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    /// Path as shown to users: relative to the root when possible.
    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// A loaded file, addressed by its workspace-relative path.
#[derive(Debug, Clone)]
pub struct SourceFile {
    path: PathBuf,
    source: SourceText,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>, text: impl Into<SourceText>) -> Self {
        Self {
            path: path.into(),
            source: text.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &SourceText {
        &self.source
    }

    pub fn text(&self) -> &str {
        self.source.as_str()
    }
}
