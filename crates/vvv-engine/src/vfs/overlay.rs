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

    fn is_removed(&self, path: &Path) -> bool {
        self.state
            .read()
            .expect("overlay lock")
            .removed
            .contains(path)
    }
}

impl Vfs for Overlay {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        if let Some(entry) = self.state.read().expect("overlay lock").layer.get(path) {
            return Ok(entry.contents.clone());
        }
        if self.is_removed(path) {
            return Err(VfsError::NotFound(path.to_path_buf()));
        }
        self.base.read(path)
    }

    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        if let Some(entry) = self.state.read().expect("overlay lock").layer.get(path) {
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
        let mut state = self.state.write().expect("overlay lock");
        state.layer.insert(path.to_path_buf(), entry);
        state.removed.remove(path);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.state
            .read()
            .expect("overlay lock")
            .layer
            .contains_key(path)
            || (!self.is_removed(path) && self.base.exists(path))
    }

    fn entry_kind(&self, path: &Path) -> Result<Option<EntryKind>, VfsError> {
        if self
            .state
            .read()
            .expect("overlay lock")
            .layer
            .contains_key(path)
        {
            return Ok(Some(EntryKind::File));
        }
        if self.is_removed(path) {
            return Ok(None);
        }
        self.base.entry_kind(path)
    }

    fn prepare_parent(&self, _path: &Path) -> ParentCreation {
        ParentCreation::new(Vec::new(), Ok(()))
    }

    fn remove_file(&self, path: &Path) -> Result<(), VfsError> {
        if !self.exists(path) {
            return Err(VfsError::NotFound(path.to_path_buf()));
        }
        let mut state = self.state.write().expect("overlay lock");
        state.layer.remove(path);
        state.removed.insert(path.to_path_buf());
        Ok(())
    }

    fn remove_empty_dir(&self, path: &Path) -> Result<(), VfsError> {
        Err(VfsError::NotFound(path.to_path_buf()))
    }

    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), MoveError> {
        let mut state = self.state.write().expect("overlay lock");
        if state.layer.contains_key(to)
            || (!state.removed.contains(to)
                && self
                    .base
                    .entry_kind(to)
                    .map_err(|error| MoveError::new(error, MoveState::Unchanged))?
                    .is_some())
        {
            return Err(MoveError::new(
                VfsError::Exists(to.to_path_buf()),
                MoveState::Unchanged,
            ));
        }
        let contents = if let Some(entry) = state.layer.get(from) {
            entry.contents.clone()
        } else if state.removed.contains(from) {
            return Err(MoveError::new(
                VfsError::NotFound(from.to_path_buf()),
                MoveState::Unchanged,
            ));
        } else {
            self.base
                .read(from)
                .map_err(|error| MoveError::new(error, MoveState::Unchanged))?
        };
        state.layer.remove(from);
        state.removed.insert(from.to_path_buf());
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
        let mut files: BTreeSet<PathBuf> = self
            .base
            .walk(root)?
            .into_iter()
            .filter(|p| !self.is_removed(p))
            .collect();
        files.extend(
            self.state
                .read()
                .expect("overlay lock")
                .layer
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
