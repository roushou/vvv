use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use super::{Stamp, Vfs, VfsError};

/// Writes on top of a file system that is never touched: what a plan looks
/// like once applied, without applying it. A second plan can be made against
/// the overlay as if the first had happened, which is how plans compose.
pub struct Overlay {
    base: Arc<dyn Vfs>,
    /// Files written here, at their overlay paths.
    layer: RwLock<BTreeMap<PathBuf, Entry>>,
    /// Base files moved away or otherwise gone from the overlay's view.
    removed: RwLock<BTreeSet<PathBuf>>,
    versions: AtomicU64,
}

#[derive(Debug)]
struct Entry {
    contents: String,
    version: u64,
}

impl std::fmt::Debug for Overlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overlay")
            .field("written", &self.layer.read().expect("overlay lock").len())
            .field("removed", &self.removed.read().expect("overlay lock").len())
            .finish()
    }
}

impl Overlay {
    pub fn over(base: Arc<dyn Vfs>) -> Self {
        Self {
            base,
            layer: RwLock::new(BTreeMap::new()),
            removed: RwLock::new(BTreeSet::new()),
            versions: AtomicU64::new(1 << 62), // never collides with a base version
        }
    }

    fn is_removed(&self, path: &Path) -> bool {
        self.removed.read().expect("overlay lock").contains(path)
    }
}

impl Vfs for Overlay {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        if let Some(entry) = self.layer.read().expect("overlay lock").get(path) {
            return Ok(entry.contents.clone());
        }
        if self.is_removed(path) {
            return Err(VfsError::NotFound(path.to_path_buf()));
        }
        self.base.read(path)
    }

    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        if let Some(entry) = self.layer.read().expect("overlay lock").get(path) {
            return Ok(Stamp::new(
                u128::from(entry.version),
                entry.contents.len() as u64,
            ));
        }
        if self.is_removed(path) {
            return Err(VfsError::NotFound(path.to_path_buf()));
        }
        self.base.stamp(path)
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError> {
        let entry = Entry {
            contents: contents.to_owned(),
            version: self.versions.fetch_add(1, Ordering::Relaxed),
        };
        self.layer
            .write()
            .expect("overlay lock")
            .insert(path.to_path_buf(), entry);
        self.removed.write().expect("overlay lock").remove(path);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.layer.read().expect("overlay lock").contains_key(path)
            || (!self.is_removed(path) && self.base.exists(path))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), VfsError> {
        let contents = self.read(from)?;
        self.write(to, &contents)?;
        self.layer.write().expect("overlay lock").remove(from);
        self.removed
            .write()
            .expect("overlay lock")
            .insert(from.to_path_buf());
        Ok(())
    }

    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError> {
        let mut files: BTreeSet<PathBuf> = self
            .base
            .walk(root)?
            .into_iter()
            .filter(|p| !self.is_removed(p))
            .collect();
        files.extend(
            self.layer
                .read()
                .expect("overlay lock")
                .keys()
                .filter(|p| p.starts_with(root))
                .cloned(),
        );
        Ok(files.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryVfs;

    #[test]
    fn overlay_shadows_moves_and_never_writes_through() {
        let base = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a.txt", "a")
                .with_file("/ws/b.txt", "b"),
        );
        let overlay = Overlay::over(base.clone());
        overlay.write(Path::new("/ws/a.txt"), "A").unwrap();
        overlay
            .rename(Path::new("/ws/b.txt"), Path::new("/ws/c.txt"))
            .unwrap();
        overlay.write(Path::new("/ws/new.txt"), "n").unwrap();

        assert_eq!(overlay.read(Path::new("/ws/a.txt")).unwrap(), "A");
        assert!(matches!(
            overlay.read(Path::new("/ws/b.txt")),
            Err(VfsError::NotFound(_))
        ));
        assert_eq!(overlay.read(Path::new("/ws/c.txt")).unwrap(), "b");
        assert!(!overlay.exists(Path::new("/ws/b.txt")) && overlay.exists(Path::new("/ws/c.txt")));
        assert_eq!(
            overlay.walk(Path::new("/ws")).unwrap(),
            [
                PathBuf::from("/ws/a.txt"),
                PathBuf::from("/ws/c.txt"),
                PathBuf::from("/ws/new.txt")
            ]
        );
        assert_ne!(
            overlay.stamp(Path::new("/ws/a.txt")).unwrap(),
            base.stamp(Path::new("/ws/a.txt")).unwrap()
        );

        assert_eq!(
            base.read(Path::new("/ws/a.txt")).unwrap(),
            "a",
            "the base is untouched"
        );
        assert!(base.exists(Path::new("/ws/b.txt")));
        assert!(!base.exists(Path::new("/ws/c.txt")));
    }
}
