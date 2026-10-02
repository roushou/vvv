//! Resolved imports, dependency origins, and import diagnostics.
use crate::graph::{Node, Structure};
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, Mark};
use crate::report::lines as l;
use crate::report::{Block, Document};
use crate::{Dep, EngineError, Importer, Reach, Skipped};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vvv_core::{Address, ImportRef, Parsed, Position, RelPath, Symbol};

/// `vvv deps <path>`: what a file imports and who imports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DepsQuery {
    pub path: RelPath,
}

/// `vvv explain <path>:<line>:<col>`: what is at a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExplainQuery {
    pub path: RelPath,
    pub position: Position,
}

/// `vvv imports [path]`: import statements worth a look.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<RelPath>,
}

/// `deps <path>`: what a file imports and who imports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Deps {
    pub path: RelPath,
    /// The file's own module address, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    pub imports: Vec<Dep>,
    pub importers: Vec<Importer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// `explain <path>:<line>:<column>`: what is at a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Explanation {
    pub path: RelPath,
    pub position: Position,
    /// The innermost declaration whose extent contains the position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    /// Where that declaration's name starts, and the source line holding it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<Position>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The file's module address, and the declaration's when a path reaches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
    /// Every other address a re-export chain offers the declaration at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<Address>,
    /// The import statement at the position, when it is inside one: what
    /// it spells and where that really comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<Dep>,
    /// Files whose imports resolve to the declaration, to any address a
    /// re-export offers it at, or to its module when the declaration itself
    /// is not addressable.
    pub importers: Vec<RelPath>,
}

/// `imports [path]`: import statements worth a look — unused, unresolved,
/// or the same target twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportsReport {
    /// The file asked about; every file when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<RelPath>,
    /// Declared imports whose bound name the file never spells again.
    pub unused: Vec<ImportSite>,
    /// Declared imports the layout could not place.
    pub unresolved: Vec<ImportSite>,
    /// Declared imports naming an address another statement in the file
    /// already brings in.
    pub redundant: Vec<ImportSite>,
    /// Files whose language has a layout but which it cannot place — a
    /// crate's integration tests, say — so their imports were not judged.
    pub unplaced: Vec<RelPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportSite {
    pub path: RelPath,
    #[serde(flatten)]
    pub import: ImportRef,
    pub start: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
}

/// What a file imports and, across its language, who imports it — through
/// re-exports both ways: an import is followed to the declaration it
/// reaches, and a file importing one of this file's declarations under an
/// address a `pub use` offers it at is an importer.
impl DepsQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Deps, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph, engine.workspace())
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Deps, EngineError> {
        let path = workspace.normalize(&self.path);
        let ns = graph.namespace_of(&path)?;
        let imports = graph.imports_of(&path)?;
        let own = ns.address(&path).ok();
        let importers = match &own {
            Some(own) => {
                let mut offered: Vec<Address> = Vec::new();
                for declared in &graph.file(&path)?.fragment(&ns)?.declarations {
                    offered.extend(graph.aliases_of(
                        &ns,
                        &declared.address,
                        &declared.symbol.name,
                    )?);
                }
                graph.importers(&ns, &path, |a| a.starts_with(own) || offered.contains(a))?
            }
            None => Vec::new(),
        };
        Ok(Deps {
            module: own,
            path: path.into(),
            imports,
            importers,
            skipped: Vec::new(),
        })
    }
}

/// What is at a position: the enclosing declaration, its address and reach,
/// and the files whose imports lead to it.
impl ExplainQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Explanation, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph, engine.workspace())
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Explanation, EngineError> {
        let position = self.position;
        let path = workspace.normalize(&self.path);
        let candidate = graph.file(&path)?;
        let source = candidate.file().source();
        let offset = source
            .offset(position)
            .ok_or_else(|| EngineError::NoSuchPosition {
                path: path.clone().into(),
                position,
            })?;
        let symbol = candidate.facts()?.enclosing(offset).cloned();
        let declared = symbol.as_ref().map(|s| source.position(s.name_span.start));
        let line = declared.and_then(|d| source.line(d.line as usize).map(str::to_owned));
        let mut explanation = Explanation {
            path: path.clone().into(),
            position,
            symbol,
            declared,
            line,
            module: None,
            address: None,
            reach: None,
            via: Vec::new(),
            import: None,
            importers: Vec::new(),
        };
        let Some(ns) = graph.namespace(&candidate.language()) else {
            return Ok(explanation);
        };
        let fragment = candidate.fragment(&ns)?;
        if let Some(edge) = fragment.import_at(offset) {
            explanation.import = Some(graph.dep(&ns, &path, source, edge)?);
        }
        let Ok(module) = ns.address(&path) else {
            return Ok(explanation);
        };
        if let Some(symbol) = &explanation.symbol {
            explanation.address = ns.address_of(&module, symbol);
            explanation.reach = Some(ns.reach(&module, symbol));
        }
        // An import of the item (at any address a re-export offers it), of
        // something under it, or of an enclosing module leads to it; a bare
        // package root (`use vvv_core::{…}`'s prefix) leads everywhere and
        // counts for nothing.
        let target = explanation
            .address
            .clone()
            .unwrap_or_else(|| module.clone());
        let aliases = match (&explanation.address, &explanation.symbol) {
            (Some(address), Some(symbol)) => graph.aliases_of(&ns, address, &symbol.name)?,
            _ => vec![target.clone()],
        };
        explanation.importers = graph
            .importers(&ns, &path, |a| {
                aliases.contains(a)
                    || a.starts_with(&target)
                    || (!a.is_root() && target.starts_with(a))
            })?
            .into_iter()
            .map(|importer| importer.path)
            .collect();
        explanation.via = aliases[1..].to_vec();
        explanation.module = Some(module);
        Ok(explanation)
    }
}

/// Import statements worth a look, per file or across the tree.
impl ImportsQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<ImportsReport, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph, engine.workspace())
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<ImportsReport, EngineError> {
        let path = self.path.as_deref().map(|p| workspace.normalize(p));
        let mut report = ImportsReport {
            path: path.clone().map(Into::into),
            unused: Vec::new(),
            unresolved: Vec::new(),
            redundant: Vec::new(),
            unplaced: Vec::new(),
        };
        let structure = Structure::of(graph)?;
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
                    address: edge.address().cloned(),
                };
                // What each statement brings in: a glob of `X` and `X`
                // itself are different things.
                let mut brought: BTreeMap<(Address, bool), usize> = BTreeMap::new();
                for edge in fragment.imports() {
                    if parsed.is_group_prefix(&edge.import) {
                        continue;
                    }
                    if edge.address().is_none() {
                        report.unresolved.push(site(edge));
                        continue;
                    }
                    if let Some(address) = edge.address() {
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

impl Document {
    pub(crate) fn deps(result: &Deps) -> Self {
        let mut report = Self::new();
        report.file_header(&result.path);
        report.block_body(Block::Blank);
        let outgoing = l::DepGroups::statements(&result.imports);
        report.body([Line::mark(Mark::Import)
            .and(Role::Plain, " ")
            .and(Role::Strong, outgoing.to_string())]);
        if result.imports.is_empty() {
            report.body([Line::single(Role::Plain, "  ").and_line(Line::mark(Mark::Nothing))]);
        }
        let own = result
            .module
            .as_ref()
            .map(|m| m.package().as_str().to_owned());
        report.block_body(Block::DepGroups {
            path: result.path.clone(),
            imports: result.imports.clone(),
            own,
        });
        report.block_body(Block::Blank);
        let incoming = l::ImporterRows::sites(&result.importers).len();
        report.body([Line::mark(Mark::ImportedBy)
            .and(Role::Plain, " ")
            .and(Role::Strong, incoming.to_string())
            .and(Role::Plain, "   ")
            .and(
                Role::Dim,
                Files::among(result.importers.iter().map(|i| i.path.as_path())).to_string(),
            )]);
        if result.importers.is_empty() {
            report.body([Line::single(Role::Plain, "  ").and_line(Line::mark(Mark::Nothing))]);
        }
        report.block_body(Block::Importers(result.importers.clone()));
        report.block_note(Block::Summary(Self::deps_summary(outgoing, incoming)));
        for skipped in &result.skipped {
            report.warning(l::SkippedLine::new(skipped).line());
        }
        report
    }

    fn deps_summary(outgoing: usize, incoming: usize) -> Line {
        Line::mark(Mark::Import)
            .and(Role::Plain, format!(" {outgoing}  "))
            .and_line(Line::mark(Mark::ImportedBy))
            .and(Role::Plain, format!(" {incoming}"))
    }

    pub(crate) fn explain(result: &Explanation) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Explanation(Box::new(result.clone())));
        report
    }

    pub(crate) fn imports(result: &ImportsReport) -> Self {
        let mut report = Self::new();
        let scope = result.path.as_ref().map_or_else(
            || "every file".to_owned(),
            |path| path.display().to_string(),
        );
        report.title(format!("imports {scope}"));
        let sections: [(Mark, &str, &[ImportSite]); 3] = [
            (Mark::Nothing, "unresolved", &result.unresolved),
            (Mark::ByHand, "unused", &result.unused),
            (Mark::ByHand, "redundant", &result.redundant),
        ];
        let total: usize = sections.iter().map(|(_, _, s)| s.len()).sum();
        if total == 0 {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " nothing to look at"),
            ));
            return report;
        }
        report.block_body(Block::Imports {
            unresolved: result.unresolved.clone(),
            unused: result.unused.clone(),
            redundant: result.redundant.clone(),
        });
        report.block_note(Block::Summary(Self::imports_summary(
            &sections,
            result.unplaced.len(),
        )));
        report
    }

    fn imports_summary(sections: &[(Mark, &str, &[ImportSite])], unplaced: usize) -> Line {
        let mut counts: Vec<Line> = sections
            .iter()
            .filter(|(_, _, s)| !s.is_empty())
            .map(|(mark, word, s)| {
                Line::mark(*mark)
                    .and(Role::Plain, format!(" {word} "))
                    .and(Role::Plain, s.len().to_string())
            })
            .collect();
        if unplaced > 0 {
            counts.push(
                Line::mark(Mark::Nothing)
                    .and(Role::Plain, " ")
                    .and(Role::Dim, "not placed")
                    .and(Role::Plain, " ")
                    .and(Role::Dim, unplaced.to_string()),
            );
        }
        Self::join(counts, "   ")
    }
}
