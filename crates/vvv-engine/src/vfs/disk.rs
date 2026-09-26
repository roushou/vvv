use std::path::{Path, PathBuf};

use super::{EntryKind, MoveError, MoveState, ParentCreation, Stamp, Vfs, VfsError};

/// Real file system. Walks respect `.gitignore` and skip hidden entries.
#[derive(Debug, Default, Clone)]
pub struct DiskVfs;

impl DiskVfs {
    pub fn new() -> Self {
        Self
    }

    fn io(path: &Path, source: std::io::Error) -> VfsError {
        match source.kind() {
            std::io::ErrorKind::NotFound => VfsError::NotFound(path.to_path_buf()),
            std::io::ErrorKind::AlreadyExists => VfsError::Exists(path.to_path_buf()),
            _ => VfsError::Io {
                path: path.to_path_buf(),
                source,
            },
        }
    }
}

impl Vfs for DiskVfs {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        let bytes = std::fs::read(path).map_err(|e| Self::io(path, e))?;
        String::from_utf8(bytes).map_err(|_| VfsError::InvalidUtf8 {
            path: path.to_path_buf(),
        })
    }

    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        let meta = std::fs::metadata(path).map_err(|e| Self::io(path, e))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        Ok(Stamp::new(modified, meta.len()))
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| Self::io(parent, e))?;
        }
        std::fs::write(path, contents).map_err(|e| Self::io(path, e))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn entry_kind(&self, path: &Path) -> Result<Option<EntryKind>, VfsError> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) => Ok(Some(if meta.is_file() {
                EntryKind::File
            } else if meta.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::Other
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Self::io(path, error)),
        }
    }

    fn prepare_parent(&self, path: &Path) -> ParentCreation {
        let mut created = Vec::new();
        let parents: Vec<_> = path
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .filter(|parent| !parent.as_os_str().is_empty())
            .collect();
        for parent in parents.into_iter().rev() {
            match std::fs::create_dir(parent) {
                Ok(()) => created.push(parent.to_path_buf()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    match std::fs::metadata(parent) {
                        Ok(meta) if meta.is_dir() => {}
                        Ok(_) => return ParentCreation::new(created, Err(Self::io(parent, error))),
                        Err(error) => {
                            return ParentCreation::new(created, Err(Self::io(parent, error)));
                        }
                    }
                }
                Err(error) => return ParentCreation::new(created, Err(Self::io(parent, error))),
            }
        }
        ParentCreation::new(created, Ok(()))
    }

    fn remove_file(&self, path: &Path) -> Result<(), VfsError> {
        std::fs::remove_file(path).map_err(|error| Self::io(path, error))
    }

    fn remove_empty_dir(&self, path: &Path) -> Result<(), VfsError> {
        std::fs::remove_dir(path).map_err(|error| Self::io(path, error))
    }

    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), MoveError> {
        DiskMove::new(from, to)?.apply()
    }

    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError> {
        // The parallel walker is what makes ripgrep's traversal fast; on a
        // large tree the serial one costs more than reading every file.
        // Each thread collects into its own buffer and hands it over once.
        let all = std::sync::Mutex::new(Vec::new());
        ignore::WalkBuilder::new(root).build_parallel().run(|| {
            let mut mine = Buffer {
                paths: Vec::new(),
                all: &all,
            };
            Box::new(move |entry| {
                if let Ok(entry) = entry
                    && entry.file_type().is_some_and(|t| t.is_file())
                {
                    mine.paths.push(entry.into_path());
                }
                ignore::WalkState::Continue
            })
        });
        let mut files = all.into_inner().expect("walk lock");
        files.sort();
        Ok(files)
    }
}

/// Owns the paths and source identity of one destination-preserving disk move.
struct DiskMove<'a> {
    from: &'a Path,
    to: &'a Path,
    source: same_file::Handle,
}

impl<'a> DiskMove<'a> {
    fn new(from: &'a Path, to: &'a Path) -> Result<Self, MoveError> {
        let metadata = std::fs::symlink_metadata(from)
            .map_err(|error| MoveError::new(DiskVfs::io(from, error), MoveState::Unchanged))?;
        if !metadata.is_file() {
            return Err(MoveError::new(
                DiskVfs::io(
                    from,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "moves require a regular file",
                    ),
                ),
                MoveState::Unchanged,
            ));
        }
        let source = same_file::Handle::from_path(from)
            .map_err(|error| MoveError::new(DiskVfs::io(from, error), MoveState::Unchanged))?;
        Ok(Self { from, to, source })
    }

    fn apply(&self) -> Result<(), MoveError> {
        match self.native() {
            Ok(()) => Ok(()),
            Err(error) if self.unsupported(&error) => self.link()?.finish(),
            Err(error) => Err(self.failure(error)),
        }
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    fn native(&self) -> std::io::Result<()> {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            self.from,
            rustix::fs::CWD,
            self.to,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(Into::into)
    }

    #[cfg(windows)]
    fn native(&self) -> std::io::Result<()> {
        use std::os::windows::ffi::OsStrExt;
        if self
            .from
            .as_os_str()
            .encode_wide()
            .chain(self.to.as_os_str().encode_wide())
            .any(|unit| unit == 0)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a path contains NUL",
            ));
        }
        // Canonical parents retain lossless OS spelling and the extended-length
        // prefix, without canonicalizing away the requested destination name.
        let absolute = |path: &Path| -> std::io::Result<PathBuf> {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = path.file_name().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "a move requires a file name",
                )
            })?;
            Ok(std::fs::canonicalize(parent)?.join(name))
        };
        atomicwrites::move_atomic(&absolute(self.from)?, &absolute(self.to)?)
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        windows
    )))]
    fn native(&self) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    fn unsupported(&self, error: &std::io::Error) -> bool {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            let Some(raw) = error.raw_os_error() else {
                return false;
            };
            let code = rustix::io::Errno::from_raw_os_error(raw);
            code == rustix::io::Errno::NOSYS
                || code == rustix::io::Errno::NOTSUP
                || code == rustix::io::Errno::INVAL
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            error.kind() == std::io::ErrorKind::Unsupported
        }
    }

    fn failure(&self, error: std::io::Error) -> MoveError {
        let unchanged =
            same_file::Handle::from_path(self.from).is_ok_and(|handle| handle == self.source);
        if unchanged && std::fs::symlink_metadata(self.to).is_ok() {
            return MoveError::new(
                VfsError::Exists(self.to.to_path_buf()),
                MoveState::Unchanged,
            );
        }
        let state = if unchanged {
            MoveState::Unchanged
        } else {
            MoveState::Unknown
        };
        let path = if error.kind() == std::io::ErrorKind::AlreadyExists {
            self.to
        } else {
            self.from
        };
        MoveError::new(DiskVfs::io(path, error), state)
    }

    fn link(&self) -> Result<LinkedMove<'_>, MoveError> {
        std::fs::hard_link(self.from, self.to).map_err(|error| {
            // A link error may have completed remotely. If the destination now
            // names the source inode, retain uncertainty instead of claiming no effect.
            if same_file::Handle::from_path(self.to).is_ok_and(|handle| handle == self.source) {
                MoveError::new(DiskVfs::io(self.to, error), MoveState::Unknown)
            } else {
                self.failure(error)
            }
        })?;
        Ok(LinkedMove { from: self.from })
    }
}

struct LinkedMove<'a> {
    from: &'a Path,
}

impl LinkedMove<'_> {
    fn finish(self) -> Result<(), MoveError> {
        std::fs::remove_file(self.from).map_err(|error| {
            MoveError::new(DiskVfs::io(self.from, error), MoveState::DestinationLinked)
        })
    }
}

/// One walker thread's paths, merged into the shared list when the thread
/// is done with them.
struct Buffer<'a> {
    paths: Vec<PathBuf>,
    all: &'a std::sync::Mutex<Vec<PathBuf>>,
}

impl Drop for Buffer<'_> {
    fn drop(&mut self) {
        self.all.lock().expect("walk lock").append(&mut self.paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static IDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "vvv-disk-move-{}-{}",
                std::process::id(),
                IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn path(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn disk_moves_preserve_occupied_files_and_directories() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let file = fixture.path("file");
        let dir = fixture.path("dir");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&file, "foreign").unwrap();
        std::fs::create_dir(&dir).unwrap();
        for destination in [&file, &dir] {
            let error = DiskVfs.move_if_absent(&source, destination).unwrap_err();
            assert_eq!(error.state, MoveState::Unchanged);
            assert!(matches!(error.source, VfsError::Exists(_)));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), "source");
        }
        assert_eq!(std::fs::read_to_string(file).unwrap(), "foreign");
        assert!(dir.is_dir());
    }

    #[test]
    fn disk_moves_refuse_a_distinct_hard_link_to_the_source() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let destination = fixture.path("destination");
        std::fs::write(&source, "source").unwrap();
        std::fs::hard_link(&source, &destination).unwrap();
        let error = DiskVfs.move_if_absent(&source, &destination).unwrap_err();
        assert_eq!(error.state, MoveState::Unchanged);
        assert!(source.exists() && destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn disk_moves_preserve_a_dangling_destination_symlink() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let destination = fixture.path("destination");
        let target = fixture.path("missing");
        std::fs::write(&source, "source").unwrap();
        std::os::unix::fs::symlink(&target, &destination).unwrap();
        assert!(matches!(
            DiskVfs
                .move_if_absent(&source, &destination)
                .unwrap_err()
                .source,
            VfsError::Exists(_)
        ));
        assert_eq!(std::fs::read_link(destination).unwrap(), target);
        assert_eq!(std::fs::read_to_string(source).unwrap(), "source");
    }

    #[test]
    fn fallback_links_preserve_an_occupied_destination() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let destination = fixture.path("destination");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&destination, "foreign").unwrap();
        let error = DiskMove::new(&source, &destination)
            .unwrap()
            .link()
            .err()
            .unwrap();
        assert_eq!(error.state, MoveState::Unchanged);
        assert_eq!(std::fs::read_to_string(source).unwrap(), "source");
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "foreign");
    }

    #[test]
    fn fallback_moves_transfer_the_source_without_replacing_a_destination() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let destination = fixture.path("destination");
        std::fs::write(&source, "source").unwrap();
        DiskMove::new(&source, &destination)
            .unwrap()
            .link()
            .unwrap()
            .finish()
            .unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "source");
    }

    #[test]
    fn fallback_reports_both_names_when_source_removal_fails() {
        let fixture = Fixture::new();
        let source = fixture.path("source");
        let destination = fixture.path("destination");
        std::fs::write(&source, "source").unwrap();
        let request = DiskMove::new(&source, &destination).unwrap();
        let linked = request.link().unwrap();
        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        let error = linked.finish().unwrap_err();
        assert_eq!(error.state, MoveState::DestinationLinked);
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "source");
        assert!(source.is_dir());
    }

    #[test]
    fn write_and_rename_create_parent_directories() {
        let root = std::env::temp_dir().join(format!("vvv-disk-vfs-{}", std::process::id()));
        let vfs = DiskVfs::new();
        let deep = root.join("a/b/c.txt");
        vfs.write(&deep, "hi").unwrap();
        assert_eq!(vfs.read(&deep).unwrap(), "hi");
        let moved = root.join("x/y/z.txt");
        vfs.prepare_parent(&moved).result.unwrap();
        vfs.move_if_absent(&deep, &moved).unwrap();
        assert!(!vfs.exists(&deep));
        assert_eq!(vfs.read(&moved).unwrap(), "hi");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
