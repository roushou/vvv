use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Stamp, Vfs, VfsError};

/// In-memory file tree. Paths are stored exactly as given. Every write and
/// rename gives the file a fresh version, so stamps behave like mtimes.
#[derive(Debug, Default)]
pub struct MemoryVfs {
    files: RwLock<BTreeMap<PathBuf, Entry>>,
    versions: AtomicU64,
}

#[derive(Debug)]
struct Entry {
    contents: String,
    version: u64,
}

impl MemoryVfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style insertion for fixtures.
    pub fn with_file(self, path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        self.write(&path.into(), &contents.into())
            .expect("MemoryVfs write cannot fail");
        self
    }

    fn entry(&self, contents: String) -> Entry {
        Entry {
            contents,
            version: self.versions.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl<P: Into<PathBuf>, S: Into<String>> FromIterator<(P, S)> for MemoryVfs {
    fn from_iter<I: IntoIterator<Item = (P, S)>>(iter: I) -> Self {
        iter.into_iter()
            .fold(MemoryVfs::new(), |vfs, (path, contents)| {
                vfs.with_file(path, contents)
            })
    }
}

impl Vfs for MemoryVfs {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        self.files
            .read()
            .expect("MemoryVfs lock poisoned")
            .get(path)
            .map(|e| e.contents.clone())
            .ok_or_else(|| VfsError::NotFound(path.to_path_buf()))
    }

    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        self.files
            .read()
            .expect("MemoryVfs lock poisoned")
            .get(path)
            .map(|e| Stamp::new(u128::from(e.version), e.contents.len() as u64))
            .ok_or_else(|| VfsError::NotFound(path.to_path_buf()))
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError> {
        let entry = self.entry(contents.to_owned());
        self.files
            .write()
            .expect("MemoryVfs lock poisoned")
            .insert(path.to_path_buf(), entry);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files
            .read()
            .expect("MemoryVfs lock poisoned")
            .contains_key(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), VfsError> {
        let mut files = self.files.write().expect("MemoryVfs lock poisoned");
        let moved = files
            .remove(from)
            .ok_or_else(|| VfsError::NotFound(from.to_path_buf()))?;
        let entry = Entry {
            contents: moved.contents,
            version: self.versions.fetch_add(1, Ordering::Relaxed),
        };
        files.insert(to.to_path_buf(), entry);
        Ok(())
    }

    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError> {
        Ok(self
            .files
            .read()
            .expect("MemoryVfs lock poisoned")
            .keys()
            .filter(|path| path.starts_with(root))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_is_scoped_and_sorted() {
        let vfs = MemoryVfs::new()
            .with_file("/ws/b.rs", "")
            .with_file("/ws/a.rs", "")
            .with_file("/other/c.rs", "");
        let files = vfs.walk(Path::new("/ws")).unwrap();
        assert_eq!(
            files,
            vec![PathBuf::from("/ws/a.rs"), PathBuf::from("/ws/b.rs")]
        );
    }
}
