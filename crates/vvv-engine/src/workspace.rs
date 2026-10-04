//! A root directory viewed through a [`Vfs`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::{DiskVfs, Overlay, Vfs, VfsError};
use vvv_core::{RelPath, SourceText};

use crate::plan::{ApplyError, Fingerprint};

#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
    vfs: Arc<dyn Vfs>,
    execution: bool,
}

impl Workspace {
    /// The tree under `root` on disk, `root` made absolute.
    pub fn disk(root: impl AsRef<Path>) -> Result<Self, VfsError> {
        let root = root.as_ref();
        let root = root.canonicalize().map_err(|source| VfsError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Ok(Self {
            root,
            vfs: Arc::new(DiskVfs::new()),
            execution: true,
        })
    }

    pub fn new(root: impl Into<PathBuf>, vfs: Arc<dyn Vfs>) -> Self {
        Self {
            root: root.into(),
            vfs,
            execution: false,
        }
    }

    /// This workspace as a plan would leave it: the same root over an
    /// overlay of its file system. Writes land in the overlay; the real
    /// files are untouched until a plan is applied here.
    pub fn staged(&self) -> Workspace {
        Workspace::new(self.root.clone(), Arc::new(Overlay::over(self.vfs.clone())))
    }

    pub(crate) fn execution_root(&self) -> Option<&Path> {
        self.execution.then_some(&self.root)
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
    witness: OnceLock<SourceWitness>,
}

impl SourceFile {
    /// Bind plugin facts to this source before any caller reads their coordinates.
    pub(crate) fn facts(
        &self,
        language: &dyn vvv_core::Language,
    ) -> Result<vvv_core::Facts, vvv_core::SearchError> {
        let facts = language.facts(self.text())?;
        facts.validate_in(self.text())?;
        Ok(facts)
    }

    pub(crate) fn find(
        &self,
        language: &dyn vvv_core::Language,
        query: &vvv_core::Query,
    ) -> Result<Vec<vvv_core::RawMatch>, vvv_core::SearchError> {
        let matches = language.find(self.text(), query)?;
        for matched in &matches {
            matched.validate_in(self.text())?;
        }
        Ok(matches)
    }
    pub fn new(path: impl Into<PathBuf>, text: impl Into<SourceText>) -> Self {
        Self {
            path: path.into(),
            source: text.into(),
            witness: OnceLock::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The identity of the immutable text from which edits are computed.
    pub(crate) fn witness(&self) -> &SourceWitness {
        self.witness.get_or_init(|| SourceWitness {
            path: self.path().into(),
            fingerprint: Fingerprint::of(self.text()),
        })
    }

    pub fn content_id(&self) -> crate::ContentId {
        crate::ContentId::from_fingerprint(self.witness().fingerprint())
    }

    pub fn source(&self) -> &SourceText {
        &self.source
    }

    pub fn text(&self) -> &str {
        self.source.as_str()
    }
}

/// A file path paired with the content identity of one observed snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceWitness {
    path: RelPath,
    fingerprint: Fingerprint,
}

impl SourceWitness {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    pub fn check(&self, other: &Self) -> Result<(), ApplyError> {
        if self != other {
            return Err(ApplyError::Stale {
                path: self.path.clone(),
            });
        }
        Ok(())
    }
}
