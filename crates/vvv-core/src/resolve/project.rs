use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::facts::Facts;
use crate::import::ImportRef;
use crate::resolve::Packages;
use crate::text::SourceText;

/// The workspace as data: which files exist and which packages they belong
/// to. What a [`Layout`](crate::Layout) and a [`Surgery`](crate::Surgery)
/// see instead of a file system.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub packages: Packages,
    pub files: FileSet,
}

/// Every file of the workspace, workspace-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileSet(BTreeSet<PathBuf>);

impl FileSet {
    pub fn contains(&self, path: &Path) -> bool {
        self.0.contains(path)
    }

    /// Whether any file lies under `dir`.
    pub fn has_under(&self, dir: &Path) -> bool {
        self.under(dir).next().is_some()
    }

    /// The files under `dir`, in path order.
    pub fn under<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a Path> + 'a {
        self.0
            .range(dir.to_path_buf()..)
            .map(PathBuf::as_path)
            .take_while(move |p| p.starts_with(dir))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<P: Into<PathBuf>> FromIterator<P> for FileSet {
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> Self {
        Self(iter.into_iter().map(Into::into).collect())
    }
}

/// One file the engine has read and parsed, as a surgery sees it.
#[derive(Debug, Clone, Copy)]
pub struct Parsed<'a> {
    pub path: &'a Path,
    pub source: &'a SourceText,
    pub facts: &'a Facts,
}

impl Parsed<'_> {
    /// Whether `import` is the prefix of a grouped statement (`std::path` in
    /// `use std::path::{Path, PathBuf}`) rather than an import of its own:
    /// an entry of the same statement is grouped under its path.
    pub fn is_group_prefix(&self, import: &ImportRef) -> bool {
        self.facts.imports.iter().any(|other| {
            other.group.as_ref().is_some_and(|group| {
                group.statement.contains(&import.span) && group.prefix == import.path
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_is_a_prefix_range() {
        let files: FileSet = [
            "src/a.rs",
            "src/a/b.rs",
            "src/a/c.rs",
            "src/ab.rs",
            "src/b.rs",
        ]
        .into_iter()
        .collect();
        let dir = Path::new("src/a");
        assert_eq!(
            files.under(dir).collect::<Vec<_>>(),
            [Path::new("src/a/b.rs"), Path::new("src/a/c.rs")]
        );
        assert!(files.has_under(dir) && !files.has_under(Path::new("src/c")));
        assert!(files.contains(Path::new("src/ab.rs")));
    }
}
