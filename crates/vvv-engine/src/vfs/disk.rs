use std::path::{Path, PathBuf};

use super::{Stamp, Vfs, VfsError};

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

    fn rename(&self, from: &Path, to: &Path) -> Result<(), VfsError> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Self::io(parent, e))?;
        }
        std::fs::rename(from, to).map_err(|e| Self::io(from, e))
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

    #[test]
    fn write_and_rename_create_parent_directories() {
        let root = std::env::temp_dir().join(format!("vvv-disk-vfs-{}", std::process::id()));
        let vfs = DiskVfs::new();
        let deep = root.join("a/b/c.txt");
        vfs.write(&deep, "hi").unwrap();
        assert_eq!(vfs.read(&deep).unwrap(), "hi");
        let moved = root.join("x/y/z.txt");
        vfs.rename(&deep, &moved).unwrap();
        assert!(!vfs.exists(&deep));
        assert_eq!(vfs.read(&moved).unwrap(), "hi");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
