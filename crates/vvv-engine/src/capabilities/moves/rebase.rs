use std::collections::BTreeMap;
use std::path::Path;

use super::Extraction;
use crate::change::Change;
use crate::{Notice, NoticeKind, Respelling, SourceFile};

use vvv_core::{
    Address, Edit, GroupedImport, GroupedImports, ImportGroup, ImportRef, ModulePath,
    RegroupedOutcome, Span,
};

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
pub(crate) struct Site<'a> {
    node: &'a Node,
    edges: Vec<&'a Edge>,
    render_from: &'a Path,
}

impl<'a> Site<'a> {
    pub fn of(node: &'a Node, render_from: &'a Path) -> Self {
        Self {
            node,
            edges: node.fragment.edges.iter().collect(),
            render_from,
        }
    }

    /// Assign source edges before rendering: moving paths use their new
    /// context, every other source path keeps its existing context.
    pub fn partition(
        node: &'a Node,
        render_from: &'a Path,
        extraction: &Extraction<'_>,
    ) -> (Self, Self) {
        let (moving, staying) = node
            .fragment
            .edges
            .iter()
            .partition(|edge| extraction.contains(edge.import.span));
        (
            Self {
                node,
                edges: moving,
                render_from,
            },
            Self {
                node,
                edges: staying,
                render_from: node.path(),
            },
        )
    }

    fn file(&self) -> &SourceFile {
        self.node.candidate.file()
    }
    fn path(&self) -> &Path {
        self.node.path()
    }
}

/// A site's edits and reports, still tied to the snapshot that produced them.
pub(crate) struct FileRewrite<'a> {
    file: &'a SourceFile,
    pub edits: Vec<Edit>,
    pub notices: Vec<Notice>,
    respellings: Vec<Respelling>,
    references: Vec<(Address, Address)>,
}

impl<'a> FileRewrite<'a> {
    fn new(file: &'a SourceFile) -> Self {
        Self {
            file,
            edits: Vec::new(),
            notices: Vec::new(),
            respellings: Vec::new(),
            references: Vec::new(),
        }
    }

    pub fn into_change(self) -> Result<(Change, Vec<(Address, Address)>), EngineError> {
        let mut change = Change::new();
        change.edits(self.file.witness(), self.edits)?;
        for notice in self.notices {
            change.notice(notice);
        }
        for respelling in self.respellings {
            change.respell(respelling);
        }
        Ok((change, self.references))
    }
}

impl<'a> Rebase<'a> {
    pub fn new(ns: &'a Namespace, old: Address, new: Address) -> Self {
        Self { ns, old, new }
    }

    /// Where `import` must point after the move, or `None` if it is unaffected.
    /// Under the moved address it is rebased; in the moved file every
    /// resolvable import is re-rendered from the new location.
    fn target(&self, site: &Site<'_>, edge: &Edge) -> Option<Address> {
        let resolved = edge.address()?;
        match resolved.rebase(&self.old, &self.new) {
            Some(rebased) => Some(rebased),
            None if site.render_from != site.path() => Some(resolved.clone()),
            None => None,
        }
    }

    /// Preserve an alias path when the binding's own rewrite already gives
    /// it the required meaning. Otherwise render the resolved target directly.
    fn binding_keeps_path(&self, site: &Site<'_>, edge: &Edge, target: &Address) -> bool {
        let Some(binding) = edge
            .binding()
            .and_then(|span| site.edges.iter().find(|e| e.import.span == span))
            .and_then(|e| e.address())
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
        out: &mut FileRewrite<'_>,
    ) -> Result<(), EngineError> {
        let rendered =
            self.ns
                .surgery()?
                .render(self.ns.project(), site.render_from, target, &import.path);
        if rendered != import.path {
            out.respellings
                .push(self.respelling(site, import, &rendered));
            out.edits
                .push(Edit::replace(import.span, rendered.to_string()));
        }
        Ok(())
    }

    fn respelling(&self, site: &Site<'_>, import: &ImportRef, to: &ModulePath) -> Respelling {
        Respelling {
            path: site.path().into(),
            span: import.span,
            start: site.file().source().position(import.span.start),
            from: import.path.to_string(),
            to: to.to_string(),
        }
    }

    /// The cached prefix after the standalone prefix or its binding is
    /// rebased. Prefix spelling is handled by that edge exactly once.
    fn prefix_after(&self, node: &Node, group: &ImportGroup) -> Option<Address> {
        let prefix = node
            .fragment
            .edges
            .iter()
            .find(|e| e.import.path == group.prefix && group.statement.contains(&e.import.span))
            .and_then(|e| e.address())?;
        Some(
            prefix
                .rebase(&self.old, &self.new)
                .unwrap_or_else(|| prefix.clone()),
        )
    }

    /// An unchanged entry suffix already names its target after prefix edits.
    fn prefix_covers(&self, edge: &Edge, prefix: Option<&Address>, target: &Address) -> bool {
        let Some(group) = &edge.import.group else {
            return false;
        };
        let Some(suffix) = edge.import.path.segments.get(group.prefix.segments.len()..) else {
            return false;
        };
        prefix.is_some_and(|p| p.extend(suffix.iter().cloned()) == *target)
    }

    fn regroup(
        &self,
        site: &Site<'_>,
        entries: Vec<GroupedImport>,
        out: &mut FileRewrite<'_>,
    ) -> Result<(), EngineError> {
        let imports = GroupedImports::new(entries)?;
        let surgery = self.ns.surgery()?;
        let regrouped = surgery.regroup(
            self.ns.project(),
            site.render_from,
            site.file().source(),
            &imports,
        );
        for (entry, outcome) in regrouped.validate(&imports)? {
            let import = &entry.import;
            match outcome {
                RegroupedOutcome::InPlace { replacement } => {
                    out.respellings
                        .push(self.respelling(site, import, replacement))
                }
                RegroupedOutcome::Structural => {}
                RegroupedOutcome::Skipped => out.notices.push(Notice {
                    path: site.path().into(),
                    start: site.file().source().position(import.span.start),
                    kind: NoticeKind::UnrewritableImport {
                        import: import.path.to_string(),
                        replacement: surgery
                            .render(
                                self.ns.project(),
                                site.render_from,
                                &entry.target,
                                &import.path,
                            )
                            .to_string(),
                    },
                }),
            }
        }
        out.edits.extend(regrouped.edits);
        Ok(())
    }

    /// What the move changes in `candidate`: its imports re-rendered, and a
    /// notice for each one the surgery could not rewrite.
    pub fn rewrite<'s>(&self, site: &Site<'s>) -> Result<FileRewrite<'s>, EngineError> {
        let mut out = FileRewrite::new(site.node.candidate.file());
        let mut grouped: BTreeMap<Span, Vec<GroupedImport>> = BTreeMap::new();
        let from = self.ns.address(site.render_from).ok();
        for edge in &site.edges {
            let import = &edge.import;
            let Some(target) = self.target(site, edge) else {
                continue;
            };
            if let Some(from) = &from {
                out.references.push((from.clone(), target.clone()));
            }
            match &import.group {
                None if self.binding_keeps_path(site, edge, &target) => {}
                None => self.standalone(site, import, &target, &mut out)?,
                Some(group) => {
                    let prefix = self.prefix_after(site.node, group);
                    if !self.prefix_covers(edge, prefix.as_ref(), &target) {
                        grouped
                            .entry(group.statement)
                            .or_default()
                            .push(GroupedImport {
                                import: import.clone(),
                                target,
                                prefix,
                            });
                    }
                }
            }
        }
        for entries in grouped.into_values() {
            self.regroup(site, entries, &mut out)?;
        }
        Ok(out)
    }
}
