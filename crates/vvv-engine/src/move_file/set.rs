//! Which paths a move touches: the file or every file under the directory,
//! plus whatever the language says travels with it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Workspace;

use vvv_core::{Layout, Project};

use crate::EngineError;

/// Current path → destination for every file that moves.
#[derive(Debug, Clone, Default)]
pub struct MoveSet {
    files: BTreeMap<PathBuf, PathBuf>,
}

impl MoveSet {
    pub fn compute(
        workspace: &Workspace,
        layout: &dyn Layout,
        project: &Project,
        from: &Path,
        to: &Path,
    ) -> Result<Self, EngineError> {
        let mut set = Self::default();
        set.add(workspace, from, to)?;
        for (f, t) in layout.companions(project, from, to) {
            set.add(workspace, &f, &t)?;
        }
        Ok(set)
    }

    /// A file maps to `to`; a directory maps every file under it to the same
    /// place under `to`. Walking a file yields just that file, so one rule
    /// covers both.
    fn add(&mut self, workspace: &Workspace, from: &Path, to: &Path) -> Result<(), EngineError> {
        for path in workspace.vfs().walk(&workspace.absolute(from))? {
            let rel = workspace.relative(&path);
            let destination = match rel.strip_prefix(from) {
                Ok(inside) if inside.as_os_str().is_empty() => to.to_path_buf(),
                Ok(inside) => to.join(inside),
                Err(_) => continue,
            };
            self.files.insert(rel, destination);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn destination(&self, path: &Path) -> Option<&Path> {
        self.files.get(path).map(PathBuf::as_path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Path, &Path)> {
        self.files.iter().map(|(f, t)| (f.as_path(), t.as_path()))
    }
}
