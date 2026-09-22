//! The questions about the whole tree rather than one file: what a package
//! offers, who depends on a declaration, what nothing refers to, which
//! imports are worth a look. Each is a walk over the graph's fragments —
//! every file's placed declarations and resolved edges — and answers with
//! plain data, the uncertainty counted rather than hidden.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use vvv_core::{Address, LanguageId, PackageId, Parsed};

use crate::command::{Command, Context};
use crate::graph::{Candidate, Declared, Node, Structure};
use crate::{
    Confidence, Consumer, Dead, DeadQuery, EngineError, Exposed, Impact, ImpactQuery, ImportSite,
    ImportsQuery, ImportsReport, Placed, Reach, ReferencesQuery, Surface, SurfaceQuery,
    Unreferenced,
};

impl Placed {
    /// A fragment's declaration as an answer names it: with its file and line.
    fn of(candidate: &Candidate, declared: &Declared) -> Self {
        Self {
            path: candidate.path().into(),
            symbol: declared.symbol.clone(),
            start: candidate
                .file()
                .source()
                .position(declared.symbol.name_span.start),
            address: declared.address.clone(),
            reach: declared.reach.clone(),
        }
    }
}

/// What a package offers to everyone: its `pub` declarations — a re-export
/// never widens what it re-exports — plus every address a re-export offers
/// them at, with how many files take them.
impl Command for SurfaceQuery {
    type Output = Surface;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let package = self.package.as_deref().map(PackageId::new);
        let mut items: Vec<Exposed> = Vec::new();
        let structure = Structure::of(&mut cx.graph)?;
        for (ns, fragments) in structure.iter() {
            for Node {
                candidate,
                fragment,
            } in fragments
            {
                for declared in &fragment.declarations {
                    if package
                        .as_ref()
                        .is_some_and(|p| declared.address.package() != p)
                    {
                        continue;
                    }
                    if declared.reach != Reach::Everyone {
                        continue;
                    }
                    // Every address a re-export chain offers it at, itself first.
                    let addresses =
                        cx.graph
                            .aliases_of(ns, &declared.address, &declared.symbol.name)?;
                    let importers = fragments
                        .iter()
                        .filter(|n| n.path() != candidate.path())
                        .filter(|n| {
                            n.fragment
                                .imports()
                                .any(|e| e.address.as_ref().is_some_and(|a| addresses.contains(a)))
                        })
                        .count();
                    items.push(Exposed {
                        declaration: Placed::of(candidate, declared),
                        via: addresses[1..].to_vec(),
                        importers,
                    });
                }
            }
        }
        items.sort_by(|a, b| a.declaration.address.cmp(&b.declaration.address));
        Ok(Surface { package, items })
    }
}

/// Who would feel a change to a declaration: the modules importing it (or
/// an address that re-exports it), then the modules importing those,
/// outward, each module once at the depth it is first reached.
impl Command for ImpactQuery {
    type Output = Impact;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let graph = &mut *cx.graph;
        let query = ReferencesQuery::new(self.name.as_str());
        let query = match &self.declared_in {
            Some(file) => query.declared_in(file),
            None => query,
        };
        let evidence = graph.references(&query)?;
        let declaration =
            evidence
                .declarations
                .first()
                .ok_or_else(|| EngineError::NoSuchSymbol {
                    name: self.name.clone(),
                    kind: None,
                })?;
        let ns = graph
            .namespace(&declaration.language)
            .ok_or_else(|| EngineError::NoLayout(declaration.language.clone()))?;
        let address = declaration
            .address
            .clone()
            .ok_or_else(|| EngineError::NoLayout(declaration.language.clone()))?;
        let aliases = graph.aliases_of(&ns, &address, &self.name)?;
        let fragments = graph.fragments(&ns)?;

        // Breadth first over module imports: what reaches an address in the
        // frontier joins the next ring.
        let mut consumers: Vec<Consumer> = Vec::new();
        let mut seen: HashSet<Address> = HashSet::new();
        let mut frontier: Vec<Address> = aliases.clone();
        let declaring = address.parent().unwrap_or_else(|| address.clone());
        let mut depth = 1;
        while !frontier.is_empty() && depth <= 8 {
            let mut next = Vec::new();
            for node in &fragments {
                let Some(module) = &node.fragment.module else {
                    continue;
                };
                if seen.contains(module) || module == &declaring {
                    continue;
                }
                let through = node
                    .fragment
                    .imports()
                    .filter_map(|e| e.address.as_ref())
                    .find(|a| frontier.iter().any(|f| a.starts_with(f)))
                    .and_then(|a| {
                        frontier.iter().find(|f| a.starts_with(f)).map(|f| {
                            if depth == 1 {
                                declaring.clone()
                            } else {
                                f.clone()
                            }
                        })
                    });
                if let Some(through) = through {
                    seen.insert(module.clone());
                    consumers.push(Consumer {
                        module: module.clone(),
                        path: node.path().into(),
                        depth,
                        through,
                    });
                    next.push(module.clone());
                }
            }
            frontier = next;
            depth += 1;
        }
        Ok(Impact {
            name: self.name,
            address,
            consumers,
        })
    }
}

/// Declarations nothing in the workspace refers to — no resolved token
/// other than the declaration's own name — with how many tokens vvv could
/// not judge and so might be a use after all.
impl Command for DeadQuery {
    type Output = Dead;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let graph = &mut *cx.graph;
        let mut items = Vec::new();
        let languages: Vec<LanguageId> = graph
            .language_ids()
            .into_iter()
            .filter(|id| self.language.as_ref().is_none_or(|l| l == id))
            .collect();
        for id in languages {
            let Some(ns) = graph.namespace(&id) else {
                continue;
            };
            let fragments = graph.fragments(&ns)?;
            let mut asked: BTreeSet<(PathBuf, String)> = BTreeSet::new();
            for Node {
                candidate,
                fragment,
            } in &fragments
            {
                for declared in &fragment.declarations {
                    // Pieces of one declaration (a type and its impls) are
                    // one question.
                    if !asked.insert((candidate.path().to_path_buf(), declared.symbol.name.clone()))
                    {
                        continue;
                    }
                    let query = ReferencesQuery::new(declared.symbol.name.as_str())
                        .in_language(id.clone())
                        .declared_in(candidate.path());
                    let evidence = graph.references(&query)?;
                    let own_names: Vec<_> = fragment
                        .declarations
                        .iter()
                        .filter(|d| d.symbol.name == declared.symbol.name)
                        .map(|d| d.symbol.name_span)
                        .collect();
                    let (mut used, mut unsure) = (false, 0);
                    for o in &evidence.occurrences {
                        let is_own_name =
                            o.m.path == candidate.path() && own_names.contains(&o.m.span);
                        match o.confidence {
                            Confidence::Resolved if !is_own_name => used = true,
                            Confidence::Unresolved => unsure += 1,
                            _ => {}
                        }
                    }
                    if !used {
                        items.push(Unreferenced {
                            declaration: Placed::of(candidate, declared),
                            unsure,
                        });
                    }
                }
            }
        }
        items.sort_by(|a, b| {
            (&a.declaration.path, a.declaration.start)
                .cmp(&(&b.declaration.path, b.declaration.start))
        });
        Ok(Dead { items })
    }
}

/// Import statements worth a look, per file or across the tree.
impl Command for ImportsQuery {
    type Output = ImportsReport;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let path = self.path.as_deref().map(|p| cx.workspace.normalize(p));
        let mut report = ImportsReport {
            path: path.clone().map(Into::into),
            unused: Vec::new(),
            unresolved: Vec::new(),
            redundant: Vec::new(),
            unplaced: Vec::new(),
        };
        let structure = Structure::of(&mut cx.graph)?;
        for (ns, fragments) in structure.iter() {
            for Node {
                candidate,
                fragment,
            } in fragments
            {
                if path.as_deref().is_some_and(|p| p != candidate.path()) {
                    continue;
                }
                if fragment.module.is_none() {
                    report.unplaced.push(candidate.path().into());
                    continue;
                }
                let facts = candidate.facts()?;
                let source = candidate.file().source();
                let parsed = Parsed {
                    path: candidate.path(),
                    source,
                    facts,
                };
                let site = |edge: &crate::graph::Edge| ImportSite {
                    path: candidate.path().into(),
                    import: edge.import.clone(),
                    start: source.position(edge.import.span.start),
                    address: edge.address.clone(),
                };
                // What each statement brings in: a glob of `X` and `X`
                // itself are different things.
                let mut brought: BTreeMap<(Address, bool), usize> = BTreeMap::new();
                for edge in fragment.imports() {
                    if parsed.is_group_prefix(&edge.import) {
                        continue;
                    }
                    if edge.address.is_none() {
                        report.unresolved.push(site(edge));
                        continue;
                    }
                    if let Some(address) = &edge.address {
                        let seen = brought
                            .entry((address.clone(), edge.import.glob))
                            .or_default();
                        *seen += 1;
                        if *seen > 1 {
                            report.redundant.push(site(edge));
                            continue;
                        }
                    }
                    // A re-export is used by its takers; a language that hides
                    // which names an import takes cannot be judged by tokens.
                    if edge.import.reexport || ns.semantics().import_scopes_names {
                        continue;
                    }
                    let Some(bound) = edge.import.binding() else {
                        continue;
                    };
                    let statement = edge
                        .import
                        .group
                        .as_ref()
                        .map_or(edge.import.span, |g| g.statement);
                    let used = facts
                        .tokens_named(bound.as_str())
                        .any(|(span, _)| !statement.contains(&span));
                    if !used {
                        report.unused.push(site(edge));
                    }
                }
            }
        }
        Ok(report)
    }
}
