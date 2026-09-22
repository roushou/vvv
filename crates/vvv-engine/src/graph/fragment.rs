//! One file's edges: what it declares, at which addresses, and what its
//! imports and paths point at. Built from the file's facts and its
//! language's namespace, once per (file stamp, project), and kept with the
//! candidate so a session pays for resolution only when something changed.

use std::sync::Arc;

use vvv_core::{Address, ImportRef, Name, PathHead, Symbol};

use super::{Candidate, Graph, Namespace};
use crate::{EngineError, Reach};

/// A declaration a path can reach, placed.
#[derive(Debug, Clone)]
pub struct Declared {
    pub symbol: Symbol,
    pub address: Address,
    pub reach: Reach,
}

/// An import or qualified path, resolved when the layout could.
#[derive(Debug, Clone)]
pub struct Edge {
    pub import: ImportRef,
    pub address: Option<Address>,
}

#[derive(Debug, Clone)]
pub struct Fragment {
    /// The file's own module address, when its language places it.
    pub module: Option<Address>,
    /// Every addressable declaration, in source order.
    pub declarations: Vec<Declared>,
    /// Every import statement and qualified path, in source order.
    pub edges: Vec<Edge>,
}

impl Fragment {
    pub(super) fn build(candidate: &super::Candidate, ns: &Namespace) -> Result<Self, EngineError> {
        let facts = candidate.facts()?;
        let path = candidate.path();
        let module = ns.address(path).ok();
        let declarations = module
            .as_ref()
            .map(|module| {
                facts
                    .symbols
                    .iter()
                    .filter(|s| ns.is_addressable(s.kind))
                    .map(|symbol| Declared {
                        address: module.join(symbol.name.as_str()),
                        reach: ns.reach(module, symbol),
                        symbol: symbol.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut edges: Vec<Edge> = facts
            .imports
            .iter()
            .map(|import| Edge {
                address: ns.resolve(path, &import.path),
                import: import.clone(),
            })
            .collect();
        // A path whose head is a name another import binds continues that
        // import: `SymbolKind::*` after `use vvv_core::SymbolKind`.
        let bound: Vec<(Name, Address)> = edges
            .iter()
            .filter(|e| e.import.declares)
            .filter_map(|e| Some((e.import.binding()?.clone(), e.address.clone()?)))
            .collect();
        for edge in edges.iter_mut().filter(|e| e.address.is_none()) {
            let path = &edge.import.path;
            if path.head != PathHead::Named {
                continue;
            }
            let Some((_, base)) = path
                .first()
                .and_then(|head| bound.iter().find(|(name, _)| name == head))
            else {
                continue;
            };
            edge.address = Some(base.extend(path.segments[1..].iter().cloned()));
        }
        Ok(Self {
            module,
            declarations,
            edges,
        })
    }

    /// The declared imports: statements, not paths in expressions.
    pub fn imports(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(|e| e.import.declares)
    }

    /// Whether anything here — an import or a path — resolves under `address`.
    pub fn reaches(&self, address: &Address) -> bool {
        self.edges
            .iter()
            .any(|e| e.address.as_ref().is_some_and(|a| a.starts_with(address)))
    }
}

/// A file of the graph with its edges.
#[derive(Clone)]
pub struct Node {
    pub candidate: Candidate,
    pub fragment: Arc<Fragment>,
}

impl Node {
    pub fn path(&self) -> &std::path::Path {
        self.candidate.path()
    }
}

/// Every placed file of the tree, by language: what the whole-tree
/// questions walk. One `Namespace` per language with a layout, each with its
/// nodes in path order.
pub struct Structure {
    namespaces: Vec<(Namespace, Vec<Node>)>,
}

impl Structure {
    pub fn of(graph: &mut Graph) -> Result<Self, EngineError> {
        let mut namespaces = Vec::new();
        for id in graph.language_ids() {
            if let Some(ns) = graph.namespace(&id) {
                let fragments = graph.fragments(&ns)?;
                namespaces.push((ns, fragments));
            }
        }
        Ok(Self { namespaces })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Namespace, &[Node])> {
        self.namespaces
            .iter()
            .map(|(ns, fragments)| (ns, fragments.as_slice()))
    }
}
