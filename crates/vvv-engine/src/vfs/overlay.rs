use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use super::{EntryKind, MoveError, MoveState, ParentCreation, Stamp, Vfs, VfsError};

/// Writes on top of a file system that is never touched: what a plan looks
/// like once applied, without applying it. A second plan can be made against
/// the overlay as if the first had happened, which is how plans compose.
pub struct Overlay {
    base: Arc<dyn Vfs>,
    /// Files written here, at their overlay paths.
    state: RwLock<OverlayState>,
    versions: AtomicU64,
}

#[derive(Debug, Default)]
struct OverlayState {
    layer: BTreeMap<PathBuf, Entry>,
    removed: BTreeSet<PathBuf>,
}

impl OverlayState {
    fn layer_path(&self, base: &dyn Vfs, path: &Path) -> Result<Option<PathBuf>, VfsError> {
        if self.layer.contains_key(path) {
            return Ok(Some(path.to_path_buf()));
        }
        for candidate in self.layer.keys() {
            if base.names_alias(candidate, path)? {
                return Ok(Some(candidate.clone()));
            }
        }
        Ok(None)
    }

    fn entry_path(&self, base: &dyn Vfs, path: &Path) -> Result<Option<PathBuf>, VfsError> {
        if let Some(path) = self.layer_path(base, path)? {
            return Ok(Some(path));
        }
        let stored = base.entry_path(path)?;
        Ok(stored.filter(|stored| !self.removed.contains(stored) && !self.removed.contains(path)))
    }
}

#[derive(Debug)]
struct Entry {
    contents: String,
    version: u64,
}

impl std::fmt::Debug for Overlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overlay")
            .field(
                "written",
                &self.state.read().expect("overlay lock").layer.len(),
            )
            .field(
                "removed",
                &self.state.read().expect("overlay lock").removed.len(),
            )
            .finish()
    }
}

impl Overlay {
    pub fn over(base: Arc<dyn Vfs>) -> Self {
        Self {
            base,
            state: RwLock::new(OverlayState::default()),
            versions: AtomicU64::new(1 << 62), // never collides with a base version
        }
    }
}

impl Vfs for Overlay {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        let state = self.state.read().expect("overlay lock");
        let stored = state
            .entry_path(self.base.as_ref(), path)?
            .ok_or_else(|| VfsError::NotFound(path.to_path_buf()))?;
        if let Some(entry) = state.layer.get(&stored) {
            return Ok(entry.contents.clone());
        }
        self.base.read(&stored)
    }

    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        let state = self.state.read().expect("overlay lock");
        let stored = state
            .entry_path(self.base.as_ref(), path)?
            .ok_or_else(|| VfsError::NotFound(path.to_path_buf()))?;
        if let Some(entry) = state.layer.get(&stored) {
            return Ok(Stamp::new(
                u128::from(entry.version),
                entry.contents.len() as u64,
            ));
        }
        self.base.stamp(&stored)
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError> {
        let mut state = self.state.write().expect("overlay lock");
        let stored = state
            .entry_path(self.base.as_ref(), path)?
            .unwrap_or_else(|| path.to_path_buf());
        state.layer.insert(
            stored.clone(),
            Entry {
                contents: contents.to_owned(),
                version: self.versions.fetch_add(1, Ordering::Relaxed),
            },
        );
        state.removed.remove(&stored);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.entry_path(path).is_ok_and(|stored| stored.is_some())
    }

    fn entry_kind(&self, path: &Path) -> Result<Option<EntryKind>, VfsError> {
        let state = self.state.read().expect("overlay lock");
        let Some(stored) = state.entry_path(self.base.as_ref(), path)? else {
            return Ok(None);
        };
        if state.layer.contains_key(&stored) {
            return Ok(Some(EntryKind::File));
        }
        self.base.entry_kind(&stored)
    }

    fn entry_path(&self, path: &Path) -> Result<Option<PathBuf>, VfsError> {
        self.state
            .read()
            .expect("overlay lock")
            .entry_path(self.base.as_ref(), path)
    }

    fn names_alias(&self, from: &Path, to: &Path) -> Result<bool, VfsError> {
        self.base.names_alias(from, to)
    }

    fn prepare_parent(&self, _path: &Path) -> ParentCreation {
        ParentCreation::new(Vec::new(), Ok(()))
    }

    fn remove_file(&self, path: &Path) -> Result<(), VfsError> {
        let mut state = self.state.write().expect("overlay lock");
        let stored = state
            .entry_path(self.base.as_ref(), path)?
            .ok_or_else(|| VfsError::NotFound(path.to_path_buf()))?;
        state.layer.remove(&stored);
        state.removed.insert(stored);
        Ok(())
    }

    fn remove_empty_dir(&self, path: &Path) -> Result<(), VfsError> {
        Err(VfsError::NotFound(path.to_path_buf()))
    }

    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), MoveError> {
        let mut state = self.state.write().expect("overlay lock");
        let unchanged = |error| MoveError::new(error, MoveState::Unchanged);
        if state
            .entry_path(self.base.as_ref(), to)
            .map_err(unchanged)?
            .is_some()
        {
            return Err(unchanged(VfsError::Exists(to.to_path_buf())));
        }
        let source = state
            .entry_path(self.base.as_ref(), from)
            .map_err(unchanged)?
            .ok_or_else(|| unchanged(VfsError::NotFound(from.to_path_buf())))?;
        let contents = if let Some(entry) = state.layer.get(&source) {
            entry.contents.clone()
        } else {
            self.base.read(&source).map_err(unchanged)?
        };
        state.layer.remove(&source);
        state.removed.insert(source);
        state.removed.remove(to);
        state.layer.insert(
            to.to_path_buf(),
            Entry {
                contents,
                version: self.versions.fetch_add(1, Ordering::Relaxed),
            },
        );
        Ok(())
    }

    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError> {
        let state = self.state.read().expect("overlay lock");
        let mut files: BTreeSet<PathBuf> = self
            .base
            .walk(root)?
            .into_iter()
            .filter(|path| !state.removed.contains(path))
            .collect();
        files.extend(
            state
                .layer
                .keys()
                .filter(|path| path.starts_with(root))
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
    fn overlay_moves_preserve_occupied_destinations() {
        let base = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a", "source")
                .with_file("/ws/b", "foreign"),
        );
        let vfs = Overlay::over(base.clone());
        let error = vfs
            .move_if_absent(Path::new("/ws/a"), Path::new("/ws/b"))
            .unwrap_err();
        assert_eq!(error.state, MoveState::Unchanged);
        assert!(matches!(error.source, VfsError::Exists(_)));
        assert_eq!(vfs.read(Path::new("/ws/a")).unwrap(), "source");
        assert_eq!(vfs.read(Path::new("/ws/b")).unwrap(), "foreign");
    }

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
            .move_if_absent(Path::new("/ws/b.txt"), Path::new("/ws/c.txt"))
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
