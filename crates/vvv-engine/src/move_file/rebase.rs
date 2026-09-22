use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::change::Change;
use crate::{Notice, NoticeKind, Respelling, SourceFile};

use vvv_core::{Address, Edit, ImportGroup, ImportRef, Layout, ModulePath, Project, Span, Surgery};

use super::MoveSet;
use crate::{Candidate, EngineError};

/// The move as imports see it: an old address becoming a new one. Asked, per
/// file, what has to change so every path still points where it did.
pub struct Rebase<'a> {
    layout: &'a dyn Layout,
    surgery: &'a dyn Surgery,
    project: &'a Project,
    moves: &'a MoveSet,
    old: Address,
    new: Address,
}

/// One file as the move sees it: read at its current path, but references
/// are rendered from where the file will be afterwards.
struct Site<'a> {
    file: &'a SourceFile,
    render_from: PathBuf,
    is_moved: bool,
}

impl<'a> Site<'a> {
    fn of(file: &'a SourceFile, moves: &MoveSet) -> Self {
        let destination = moves.destination(file.path());
        Self {
            file,
            render_from: destination.unwrap_or(file.path()).to_path_buf(),
            is_moved: destination.is_some(),
        }
    }

    fn path(&self) -> &Path {
        self.file.path()
    }
}

/// Everything the move changes in one file: the edits, respellings and
/// notices as a [`Change`], and every reference the move affects —
/// rewritten, or read from a new place — as (the module it is read from
/// afterwards, what it names afterwards), which reachability judges.
#[derive(Default)]
pub(crate) struct FileRewrite {
    pub change: Change,
    pub references: Vec<(Address, Address)>,
}

impl<'a> Rebase<'a> {
    pub fn new(
        layout: &'a dyn Layout,
        surgery: &'a dyn Surgery,
        project: &'a Project,
        from: &Path,
        to: &Path,
        moves: &'a MoveSet,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            layout,
            surgery,
            project,
            moves,
            old: layout.address(project, from)?,
            new: layout.address(project, to)?,
        })
    }

    /// A rebase of addresses alone — an item moving between files — with no
    /// file moving: every file is read from where it is.
    pub fn of_addresses(
        layout: &'a dyn Layout,
        surgery: &'a dyn Surgery,
        project: &'a Project,
        moves: &'a MoveSet,
        old: Address,
        new: Address,
    ) -> Self {
        Self {
            layout,
            surgery,
            project,
            moves,
            old,
            new,
        }
    }

    /// Where `import` must point after the move, or `None` if it is unaffected.
    /// Under the moved address it is rebased; in the moved file every
    /// resolvable import is re-rendered from the new location.
    fn target(&self, site: &Site<'_>, import: &ImportRef) -> Option<(Address, bool)> {
        let resolved = self
            .layout
            .resolve(self.project, site.path(), &import.path)?;
        match resolved.rebase(&self.old, &self.new) {
            Some(rebased) => Some((rebased, true)),
            None if site.is_moved => Some((resolved, false)),
            None => None,
        }
    }

    /// A standalone import: replace its span if the rendering changes.
    fn standalone(
        &self,
        site: &Site<'_>,
        import: &ImportRef,
        target: &Address,
        out: &mut FileRewrite,
    ) {
        let rendered = self
            .surgery
            .render(self.project, &site.render_from, target, &import.path);
        if rendered != import.path {
            out.change.respell(self.respelling(site, import, &rendered));
            out.change.edit(
                site.path(),
                Edit::replace(import.span, rendered.to_string()),
            );
        }
    }

    fn respelling(&self, site: &Site<'_>, import: &ImportRef, to: &ModulePath) -> Respelling {
        Respelling {
            path: site.path().into(),
            span: import.span,
            start: site.file.source().position(import.span.start),
            from: import.path.to_string(),
            to: to.to_string(),
        }
    }

    /// Whether a grouped entry needs the surgery's attention at all: not when
    /// the group's own prefix is being rewritten (that covers it), and not when
    /// the prefix keeps its meaning from the new location and the entry itself
    /// is not under the moved address.
    fn grouped_needs_change(&self, site: &Site<'_>, group: &ImportGroup, under_old: bool) -> bool {
        let prefix = &group.prefix;
        let prefix_before = self.layout.resolve(self.project, site.path(), prefix);
        if prefix_before
            .as_ref()
            .is_some_and(|p| p.starts_with(&self.old))
        {
            return false;
        }
        let prefix_now = self.layout.resolve(self.project, &site.render_from, prefix);
        under_old || prefix_now != prefix_before
    }

    /// Hand one statement's grouped entries to the surgery; what it cannot
    /// rewrite becomes a notice carrying the text it would have written.
    fn regroup(&self, site: &Site<'_>, entries: &[(ImportRef, Address)], out: &mut FileRewrite) {
        let regrouped =
            self.surgery
                .regroup(self.project, &site.render_from, site.file.source(), entries);
        // An entry rewritten within its span is a respelling; one the
        // surgery took out of the group is more than that and stays a hunk.
        for (import, target) in entries {
            if regrouped.edits.iter().any(|e| e.span == import.span) {
                let to = self
                    .surgery
                    .render(self.project, &site.render_from, target, &import.path);
                out.change.respell(self.respelling(site, import, &to));
            }
        }
        out.change.edits(site.path(), regrouped.edits);
        for import in regrouped.skipped {
            let replacement = entries
                .iter()
                .find(|(r, _)| r.span == import.span)
                .map(|(_, t)| {
                    self.surgery
                        .render(self.project, &site.render_from, t, &import.path)
                        .to_string()
                })
                .unwrap_or_default();
            out.change.notice(Notice {
                path: site.path().into(),
                start: site.file.source().position(import.span.start),
                kind: NoticeKind::UnrewritableImport {
                    import: import.path.to_string(),
                    replacement,
                },
            });
        }
    }

    /// What the move changes in `candidate`: its imports re-rendered, and a
    /// notice for each one the surgery could not rewrite.
    pub fn rewrite(&self, candidate: &Candidate) -> Result<FileRewrite, EngineError> {
        let site = Site::of(candidate.file(), self.moves);
        let imports = candidate.imports()?;

        let mut out = FileRewrite::default();
        let mut grouped: BTreeMap<Span, Vec<(ImportRef, Address)>> = BTreeMap::new();
        let from = self.layout.address(self.project, &site.render_from).ok();
        for import in imports {
            let Some((target, under_old)) = self.target(&site, &import) else {
                continue;
            };
            if let Some(from) = &from {
                out.references.push((from.clone(), target.clone()));
            }
            match &import.group {
                None => self.standalone(&site, &import, &target, &mut out),
                Some(group) if self.grouped_needs_change(&site, group, under_old) => grouped
                    .entry(group.statement)
                    .or_default()
                    .push((import, target)),
                Some(_) => {}
            }
        }
        for entries in grouped.values() {
            self.regroup(&site, entries, &mut out);
        }
        Ok(out)
    }
}
