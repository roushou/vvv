use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Edit;
use crate::paths::RelPath;
use crate::text::Span;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditConflict {
    #[error("{}: edit at {new:?} overlaps existing edit at {existing:?}", path.display())]
    Overlap {
        path: RelPath,
        existing: Span,
        new: Span,
    },
    #[error("{} is already being moved to {}", from.display(), existing.display())]
    AlreadyMoved { from: RelPath, existing: RelPath },
}

/// Non-overlapping edits grouped by file, kept sorted by span, plus file moves.
///
/// Edits to a moved file are keyed by its *current* path; the move is applied
/// after the edits, so a planner never has to think about ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    files: BTreeMap<PathBuf, Vec<Edit>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    moves: BTreeMap<PathBuf, PathBuf>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `from` ends up at `to`. Registers `from` as a touched file.
    pub fn move_file(
        &mut self,
        from: impl Into<PathBuf>,
        to: impl Into<PathBuf>,
    ) -> Result<(), EditConflict> {
        let from = from.into();
        if let Some(existing) = self.moves.get(&from) {
            return Err(EditConflict::AlreadyMoved {
                from: from.into(),
                existing: existing.clone().into(),
            });
        }
        self.files.entry(from.clone()).or_default();
        self.moves.insert(from, to.into());
        Ok(())
    }

    /// Where `path` ends up, if it is moved.
    pub fn destination(&self, path: &Path) -> Option<&Path> {
        self.moves.get(path).map(PathBuf::as_path)
    }

    pub fn moves(&self) -> impl Iterator<Item = (&Path, &Path)> {
        self.moves.iter().map(|(f, t)| (f.as_path(), t.as_path()))
    }

    /// Add an edit, refusing one that overlaps an edit already recorded for the file.
    pub fn insert(&mut self, path: impl Into<PathBuf>, edit: Edit) -> Result<(), EditConflict> {
        let path = path.into();
        let edits = self.files.entry(path.clone()).or_default();
        if let Some(existing) = edits.iter().find(|e| e.span.overlaps(&edit.span)) {
            return Err(EditConflict::Overlap {
                path: path.into(),
                existing: existing.span,
                new: edit.span,
            });
        }
        let at = edits.partition_point(|e| e.span.start <= edit.span.start);
        edits.insert(at, edit);
        Ok(())
    }

    pub fn merge(mut self, other: ChangeSet) -> Result<Self, EditConflict> {
        for (path, edits) in other.files {
            for edit in edits {
                self.insert(&path, edit)?;
            }
        }
        for (from, to) in other.moves {
            self.move_file(from, to)?;
        }
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.files.values().all(Vec::is_empty) && self.moves.is_empty()
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    pub fn edits_for(&self, path: &Path) -> &[Edit] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }

    /// Produce the new contents of one file without touching any storage.
    pub fn apply_to(&self, path: &Path, original: &str) -> String {
        let mut out = String::with_capacity(original.len());
        let mut cursor = 0;
        for edit in self.edits_for(path) {
            out.push_str(&original[cursor..edit.span.start]);
            out.push_str(&edit.replacement);
            cursor = edit.span.end;
        }
        out.push_str(&original[cursor..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_in_span_order_regardless_of_insertion_order() {
        let mut cs = ChangeSet::new();
        let p = Path::new("f.rs");
        cs.insert(p, Edit::replace(Span::new(8, 11), "qux"))
            .unwrap();
        cs.insert(p, Edit::replace(Span::new(0, 3), "baz")).unwrap();
        assert_eq!(cs.apply_to(p, "foo and bar"), "baz and qux");
    }

    #[test]
    fn overlapping_edits_are_rejected() {
        let mut cs = ChangeSet::new();
        let p = Path::new("f.rs");
        cs.insert(p, Edit::replace(Span::new(0, 5), "x")).unwrap();
        let err = cs.insert(p, Edit::delete(Span::new(4, 8))).unwrap_err();
        assert!(matches!(
            err,
            EditConflict::Overlap { existing, new, .. }
                if existing == Span::new(0, 5) && new == Span::new(4, 8)
        ));
    }

    #[test]
    fn insertions_at_the_same_offset_are_allowed() {
        let mut cs = ChangeSet::new();
        let p = Path::new("f.rs");
        cs.insert(p, Edit::insert(0, "a")).unwrap();
        cs.insert(p, Edit::insert(0, "b")).unwrap();
        assert_eq!(cs.apply_to(p, "-"), "ab-");
    }
}
