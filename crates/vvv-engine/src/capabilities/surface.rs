//! Public package exposure and its consumers.
use crate::graph::{Node, Structure};
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::Mark;
use crate::report::{Block, Document};
use crate::{EngineError, Placed, Reach};
use serde::{Deserialize, Serialize};
use vvv_core::{Address, PackageId};

/// `vvv surface [package]`: what a package offers to everyone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SurfaceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// `surface [package]`: what a package offers to everyone, and who takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Surface {
    /// The package asked about; every package when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<vvv_core::PackageId>,
    pub items: Vec<Exposed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Exposed {
    #[serde(flatten)]
    pub declaration: Placed,
    /// The other addresses it is offered at, through re-exports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<Address>,
    /// Files in the workspace importing it, at any of its addresses.
    pub importers: usize,
}

/// What a package offers to everyone: its `pub` declarations — a re-export
/// never widens what it re-exports — plus every address a re-export offers
/// them at, with how many files take them.
impl SurfaceQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Surface, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
    ) -> Result<Surface, EngineError> {
        let package = self.package.as_deref().map(PackageId::new);
        let mut items: Vec<Exposed> = Vec::new();
        let structure = Structure::of(graph)?;
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
                        graph.aliases_of(ns, &declared.address, &declared.symbol.name)?;
                    let importers = fragments
                        .iter()
                        .filter(|n| n.path() != candidate.path())
                        .filter(|n| {
                            n.fragment
                                .imports()
                                .any(|e| e.address().is_some_and(|a| addresses.contains(a)))
                        })
                        .count();
                    items.push(Exposed {
                        declaration: candidate.placed(declared),
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

impl Document {
    pub(crate) fn surface(result: &Surface) -> Self {
        let mut report = Self::new();
        let scope = result
            .package
            .as_ref()
            .map_or_else(|| "every package".to_owned(), ToString::to_string);
        report.title(format!("surface {scope}"));
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " nothing exposed"),
            ));
            return report;
        }
        report.block_body(Block::Exposed(result.items.clone()));
        report.block_note(Block::Summary(Self::surface_summary(result)));
        report
    }

    fn surface_summary(result: &Surface) -> Line {
        let importers: usize = result.items.iter().map(|i| i.importers).sum();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Strong, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::ImportedBy))
            .and(Role::Plain, " ")
            .and(Role::Dim, format!("{importers} imports"))
    }
}
