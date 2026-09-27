use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::change::Change;
use crate::{Notice, NoticeKind, Respelling, SourceFile};

use vvv_core::{Address, Edit, ImportGroup, ImportRef, ModulePath, Span};

use crate::EngineError;
use crate::graph::{Edge, Namespace, Node};

/// The move as imports see it: an old address becoming a new one. Asked, per
/// file, what has to change so every path still points where it did.
pub struct Rebase<'a> {
    ns: &'a Namespace,
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
    fn of(file: &'a SourceFile, render_from: &Path) -> Self {
        Self {
            file,
            render_from: render_from.to_path_buf(),
            is_moved: render_from != file.path(),
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
    pub fn new(ns: &'a Namespace, old: Address, new: Address) -> Self {
        Self { ns, old, new }
    }

    /// Where `import` must point after the move, or `None` if it is unaffected.
    /// Under the moved address it is rebased; in the moved file every
    /// resolvable import is re-rendered from the new location.
    fn target(&self, site: &Site<'_>, edge: &Edge) -> Option<(Address, bool)> {
        let resolved = edge.address()?;
        match resolved.rebase(&self.old, &self.new) {
            Some(rebased) => Some((rebased, true)),
            None if site.is_moved => Some((resolved.clone(), false)),
            None => None,
        }
    }

    /// Preserve an alias path when the binding's own rewrite already gives
    /// it the required meaning. Otherwise render the resolved target directly.
    fn binding_keeps_path(&self, node: &Node, edge: &Edge, target: &Address) -> bool {
        let Some(binding) = edge
            .binding()
            .and_then(|span| node.fragment.edges.iter().find(|e| e.import.span == span))
            .and_then(Edge::address)
        else {
            return false;
        };
        let after = binding
            .rebase(&self.old, &self.new)
            .unwrap_or_else(|| binding.clone());
        after.extend(edge.import.path.segments[1..].iter().cloned()) == *target
    }

    /// A standalone import: replace its span if the rendering changes.
    fn standalone(
        &self,
        site: &Site<'_>,
        import: &ImportRef,
        target: &Address,
        out: &mut FileRewrite,
    ) -> Result<(), EngineError> {
        let rendered =
            self.ns
                .surgery()?
                .render(self.ns.project(), &site.render_from, target, &import.path);
        if rendered != import.path {
            out.change.respell(self.respelling(site, import, &rendered));
            out.change.edit(
                site.file.witness(),
                Edit::replace(import.span, rendered.to_string()),
            )?;
        }
        Ok(())
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
    fn grouped_needs_change(
        &self,
        site: &Site<'_>,
        node: &Node,
        group: &ImportGroup,
        under_old: bool,
    ) -> bool {
        let prefix = &group.prefix;
        let prefix_before = node
            .fragment
            .edges
            .iter()
            .find(|e| e.import.path == *prefix && group.statement.contains(&e.import.span))
            .and_then(Edge::address);
        if prefix_before.is_some_and(|p| p.starts_with(&self.old)) {
            return false;
        }
        let prefix_now = if site.is_moved {
            self.ns.resolve(&site.render_from, prefix)
        } else {
            prefix_before.cloned()
        };
        under_old || prefix_now.as_ref() != prefix_before
    }

    /// Hand one statement's grouped entries to the surgery; what it cannot
    /// rewrite becomes a notice carrying the text it would have written.
    fn regroup(
        &self,
        site: &Site<'_>,
        entries: &[(ImportRef, Address)],
        out: &mut FileRewrite,
    ) -> Result<(), EngineError> {
        let surgery = self.ns.surgery()?;
        let regrouped = self.ns.surgery()?.regroup(
            self.ns.project(),
            &site.render_from,
            site.file.source(),
            entries,
        );
        // An entry rewritten within its span is a respelling; one the
        // surgery took out of the group is more than that and stays a hunk.
        for (import, target) in entries {
            if regrouped.edits.iter().any(|e| e.span == import.span) {
                let to = self.ns.surgery()?.render(
                    self.ns.project(),
                    &site.render_from,
                    target,
                    &import.path,
                );
                out.change.respell(self.respelling(site, import, &to));
            }
        }
        out.change.edits(site.file.witness(), regrouped.edits)?;
        for import in regrouped.skipped {
            let replacement = entries
                .iter()
                .find(|(r, _)| r.span == import.span)
                .map(|(_, t)| {
                    surgery
                        .render(self.ns.project(), &site.render_from, t, &import.path)
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
        Ok(())
    }

    /// What the move changes in `candidate`: its imports re-rendered, and a
    /// notice for each one the surgery could not rewrite.
    pub fn rewrite(&self, node: &Node, render_from: &Path) -> Result<FileRewrite, EngineError> {
        let site = Site::of(node.candidate.file(), render_from);

        let mut out = FileRewrite::default();
        let mut grouped: BTreeMap<Span, Vec<(ImportRef, Address)>> = BTreeMap::new();
        let from = self.ns.address(&site.render_from).ok();
        for edge in &node.fragment.edges {
            let import = &edge.import;
            let Some((target, under_old)) = self.target(&site, edge) else {
                continue;
            };
            if let Some(from) = &from {
                out.references.push((from.clone(), target.clone()));
            }
            match &import.group {
                None if self.binding_keeps_path(node, edge, &target) => {}
                None => self.standalone(&site, import, &target, &mut out)?,
                Some(group) if self.grouped_needs_change(&site, node, group, under_old) => grouped
                    .entry(group.statement)
                    .or_default()
                    .push((import.clone(), target)),
                Some(_) => {}
            }
        }
        for entries in grouped.values() {
            self.regroup(&site, entries, &mut out)?;
        }
        Ok(out)
    }
}
