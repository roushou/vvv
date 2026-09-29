//! Declaration usage: impact and declarations with no resolved uses.
use crate::graph::Node;
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, Mark};
use crate::report::{Block, Document};
use crate::{Confidence, EngineError, Placed, ReferencesQuery};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use vvv_core::{Address, LanguageId, RelPath};

/// `vvv impact <name>`: who would feel a change to a declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactQuery {
    pub name: String,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
}

/// `vvv dead`: declarations nothing in the workspace refers to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeadQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<vvv_core::LanguageId>,
}

/// `impact <name>`: every module that would feel a change to a declaration —
/// those importing it, then those importing them, outward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Impact {
    pub name: String,
    pub address: Address,
    /// Nearest first: depth 1 imports the declaration itself.
    pub consumers: Vec<Consumer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Consumer {
    pub module: Address,
    pub path: RelPath,
    pub depth: u32,
    /// The module it reaches the declaration through; the declaration's
    /// own module at depth 1.
    pub through: Address,
}

/// `dead [--lang]`: declarations nothing in the workspace refers to, with
/// how many tokens might.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Dead {
    pub items: Vec<Unreferenced>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Unreferenced {
    #[serde(flatten)]
    pub declaration: Placed,
    /// Tokens spelling the name that vvv could not judge: any of them might
    /// be a use.
    pub unsure: usize,
}

/// Who would feel a change to a declaration: the modules importing it (or
/// an address that re-exports it), then the modules importing those,
/// outward, each module once at the depth it is first reached.
impl ImpactQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Impact, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(self, graph: &mut crate::graph::Graph) -> Result<Impact, EngineError> {
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
                    .filter_map(|e| e.address())
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
impl DeadQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Dead, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(self, graph: &mut crate::graph::Graph) -> Result<Dead, EngineError> {
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
                            declaration: candidate.placed(declared),
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

impl Document {
    pub(crate) fn impact(result: &Impact) -> Self {
        let mut report = Self::new();
        report.body([Line::of(Role::Title, format!("impact {}", result.name))]);
        if result.consumers.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no module imports it"),
            ));
            return report;
        }
        report.block_body(Block::Consumers(result.consumers.clone()));
        report.block_note(Block::Summary(
            Line::mark(Mark::ImportedBy)
                .and(Role::Plain, " ")
                .and(Role::Strong, result.consumers.len().to_string())
                .and(Role::Plain, "   ")
                .and(
                    Role::Dim,
                    Files::among(result.consumers.iter().map(|c| c.path.as_path())).to_string(),
                ),
        ));
        report
    }

    pub(crate) fn dead(result: &Dead) -> Self {
        let mut report = Self::new();
        report.title("dead");
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " everything is referred to"),
            ));
            return report;
        }
        report.block_body(Block::Dead(result.items.clone()));
        report.block_note(Block::Summary(Self::dead_summary(result)));
        report
    }

    fn dead_summary(result: &Dead) -> Line {
        let unsure = result.items.iter().filter(|i| i.unsure > 0).count();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Strong, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::Unverified))
            .and(Role::Plain, " ")
            .and(
                Role::Dim,
                format!("{unsure} with tokens that might be uses"),
            )
    }
}
