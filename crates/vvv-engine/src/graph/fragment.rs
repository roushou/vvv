//! One file's edges: what it declares, at which addresses, and what its
//! imports and paths point at. Built from the file's facts and its
//! language's namespace, once per (file stamp, project), and kept with the
//! candidate so a session pays for resolution only when something changed.

use std::sync::Arc;

use vvv_core::{Address, ImportRef, PathHead, Span, Symbol};

use super::{Candidate, Graph, Namespace};
use crate::{EngineError, Reach};

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod common;

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
    resolution: Option<Resolution>,
}

/// How an edge acquired its meaning; binding identity survives path rendering.
#[derive(Debug, Clone)]
enum Resolution {
    Direct(Address),
    Bound { address: Address, binding: Span },
}

impl Edge {
    pub fn address(&self) -> Option<&Address> {
        self.resolution.as_ref().map(|r| match r {
            Resolution::Direct(address) | Resolution::Bound { address, .. } => address,
        })
    }

    pub fn binding(&self) -> Option<Span> {
        match &self.resolution {
            Some(Resolution::Bound { binding, .. }) => Some(*binding),
            _ => None,
        }
    }

    /// Continue a named path from the imported binding that supplies its head.
    /// The immediate binding's span links to its own resolution, retaining the
    /// provenance of every hop rather than flattening an alias into a raw path.
    fn through_binding(&self, edges: &[Edge]) -> Option<Resolution> {
        let path = &self.import.path;
        if path.head != PathHead::Named {
            return None;
        }
        let head = path.first()?;
        let binding = edges.iter().find(|edge| {
            edge.import.declares && edge.import.binding() == Some(head) && edge.address().is_some()
        })?;
        Some(Resolution::Bound {
            address: binding
                .address()?
                .extend(path.segments[1..].iter().cloned()),
            binding: binding.import.span,
        })
    }
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
        let edges: Vec<Edge> = facts
            .imports
            .iter()
            .map(|import| Edge {
                resolution: ns.resolve(path, &import.path).map(Resolution::Direct),
                import: import.clone(),
            })
            .collect();
        let mut fragment = Self {
            module,
            declarations,
            edges,
        };
        fragment.propagate_bindings();
        Ok(fragment)
    }

    /// Newly placed imports can supply other bindings, irrespective of source
    /// order. Only unresolved edges change; a cycle without a placed seed makes
    /// no progress and stays unresolved.
    fn propagate_bindings(&mut self) {
        loop {
            let resolved: Vec<_> = self
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.address().is_none())
                .filter_map(|(index, edge)| {
                    edge.through_binding(&self.edges)
                        .map(|resolution| (index, resolution))
                })
                .collect();
            if resolved.is_empty() {
                break;
            }
            for (index, resolution) in resolved {
                self.edges[index].resolution = Some(resolution);
            }
        }
    }

    /// The declared imports: statements, not paths in expressions.
    pub fn imports(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(|e| e.import.declares)
    }

    /// The exact import path under a position takes precedence over an entry
    /// whose grouped statement merely contains it. Outside entry spans, keep
    /// the containing statement's first grouped entry as the fallback.
    pub fn import_at(&self, offset: usize) -> Option<&Edge> {
        self.imports()
            .find(|edge| edge.import.span.contains_offset(offset))
            .or_else(|| {
                self.imports().find(|edge| {
                    edge.import
                        .group
                        .as_ref()
                        .is_some_and(|group| group.statement.contains_offset(offset))
                })
            })
    }

    /// Whether anything here — an import or a path — resolves under `address`.
    pub fn reaches(&self, address: &Address) -> bool {
        self.edges
            .iter()
            .any(|e| e.address().is_some_and(|a| a.starts_with(address)))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{common::Fake, *};
    use crate::{Engine, Languages, MemoryVfs, Workspace};

    struct FragmentQuery {
        path: PathBuf,
    }

    impl FragmentQuery {
        fn execute(self, engine: &Engine) -> Result<Arc<Fragment>, EngineError> {
            let _operation = engine.operation();
            let mut graph = engine.graph()?;
            let ns = graph.namespace_of(&self.path)?;
            graph.file(&self.path)?.fragment(&ns)
        }
    }

    #[test]
    fn fragment_propagates_alias_chains_with_binding_provenance() {
        let source = "use a.p as root\nuse root::child as parent\nuse parent::nested as leaf\nparent::Foo leaf::Child";
        let engine = Engine::new(
            Workspace::new(
                "/ws",
                Arc::new(MemoryVfs::new().with_file("/ws/consumer.p", source)),
            ),
            Languages::new()
                .with(Fake::default().with_unresolved_heads(&["root", "parent", "leaf"])),
        );
        let fragment = FragmentQuery {
            path: "consumer.p".into(),
        }
        .execute(&engine)
        .unwrap();
        let expected = [
            ("root::child", Address::new("ws", ["a.p", "child"]), "a.p"),
            (
                "parent::nested",
                Address::new("ws", ["a.p", "child", "nested"]),
                "root::child",
            ),
            (
                "parent::Foo",
                Address::new("ws", ["a.p", "child", "Foo"]),
                "root::child",
            ),
            (
                "leaf::Child",
                Address::new("ws", ["a.p", "child", "nested", "Child"]),
                "parent::nested",
            ),
        ];
        for (path, address, binding_path) in expected {
            let edge = fragment
                .edges
                .iter()
                .find(|edge| edge.import.path.to_string() == path)
                .unwrap();
            let binding = fragment
                .imports()
                .find(|edge| edge.import.path.to_string() == binding_path)
                .unwrap();
            assert_eq!(edge.address(), Some(&address), "{path}");
            assert_eq!(edge.binding(), Some(binding.import.span), "{path}");
        }
    }

    #[test]
    fn fragment_binding_resolution_is_independent_of_import_order() {
        let source = "use leaf::Inner as tip\nuse parent::nested as leaf\nuse root::child as parent\nuse a.p as root\ntip::Item";
        let engine = Engine::new(
            Workspace::new(
                "/ws",
                Arc::new(MemoryVfs::new().with_file("/ws/consumer.p", source)),
            ),
            Languages::new()
                .with(Fake::default().with_unresolved_heads(&["root", "parent", "leaf", "tip"])),
        );
        let fragment = FragmentQuery {
            path: "consumer.p".into(),
        }
        .execute(&engine)
        .unwrap();
        assert_eq!(
            fragment.edges.last().unwrap().address(),
            Some(&Address::new(
                "ws",
                ["a.p", "child", "nested", "Inner", "Item"]
            ))
        );
        assert!(fragment.edges.iter().all(|edge| edge.address().is_some()));
        for index in 0..3 {
            assert_eq!(
                fragment.edges[index].binding(),
                Some(fragment.edges[index + 1].import.span)
            );
        }
    }

    #[test]
    fn unseeded_alias_cycles_remain_unresolved() {
        let engine = Engine::new(
            Workspace::new(
                "/ws",
                Arc::new(MemoryVfs::new().with_file(
                    "/ws/consumer.p",
                    "use left::a as right\nuse right::b as left\nleft::Item",
                )),
            ),
            Languages::new().with(Fake::default().with_unresolved_heads(&["left", "right"])),
        );
        let fragment = FragmentQuery {
            path: "consumer.p".into(),
        }
        .execute(&engine)
        .unwrap();
        assert!(fragment.edges.iter().all(|edge| edge.address().is_none()));
    }
}
