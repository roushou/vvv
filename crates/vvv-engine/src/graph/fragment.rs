//! One file's edges: what it declares, at which addresses, and what its
//! imports and paths point at. Built from the file's facts and its
//! language's namespace and captured binding inputs, and kept with the
//! candidate so a session pays for resolution only when something changed.

use std::sync::Arc;

use super::import_bindings::{ImportBindings, ModuleInput, Resolution, Targets};
use vvv_core::{Address, ImportRef, Span, Symbol};

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
    resolution: Targets,
    pub(super) prefixes: Vec<Vec<Address>>,
}

impl Edge {
    /// A unique target only; callers that can represent ambiguity use addresses.
    pub fn address(&self) -> Option<&Address> {
        (self.resolution.values.len() == 1).then(|| &self.resolution.values[0].address)
    }
    pub(super) fn addresses(&self) -> impl Iterator<Item = &Address> {
        self.resolution.values.iter().map(|r| &r.address)
    }
    pub(super) fn resolutions(&self) -> &[Resolution] {
        &self.resolution.values
    }
    pub(crate) fn foreign_binding(&self, path: &std::path::Path) -> Option<(&Address, &Address)> {
        self.address()?;
        self.resolution.values[0]
            .via
            .iter()
            .find(|origin| origin.anchor.path.as_path() != path)
            .map(|origin| (&origin.address, &origin.target))
    }
    /// Public re-export addresses remain visible in dependency reports, with
    /// their canonical target reported separately as the origin.
    pub(super) fn dependency_address(&self, file: &std::path::Path) -> Option<Address> {
        let target = self.address()?;
        self.resolution.values[0]
            .via
            .iter()
            .find(|origin| origin.reexport && origin.anchor.path.as_path() != file)
            .and_then(|origin| {
                target
                    .strip_prefix(&origin.target)
                    .map(|suffix| origin.address.extend(suffix.iter().cloned()))
            })
            .or_else(|| Some(target.clone()))
    }
    pub(super) fn reexported(&self, file: &std::path::Path) -> bool {
        self.resolution.values.iter().any(|resolution| {
            resolution
                .via
                .iter()
                .any(|origin| origin.reexport && origin.anchor.path.as_path() != file)
        })
    }
    pub(super) fn cyclic(&self) -> bool {
        self.resolution.cyclic
    }
    pub fn binding(&self) -> Option<Span> {
        self.address()
            .and_then(|_| self.resolution.values[0].binding)
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
    pub(super) inputs: Vec<crate::SourceVersion>,
    modules: Vec<ModuleInput>,
}

impl Fragment {
    pub(super) fn build(
        candidate: &super::candidate::SourceFacts,
        ns: &Namespace,
    ) -> Result<Self, EngineError> {
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
        let mut bindings = ImportBindings::new(ns);
        let mut edges = Vec::with_capacity(facts.imports.len());
        for import in &facts.imports {
            let resolution = bindings.import(candidate, import)?;
            let mut prefixes = Vec::new();
            for index in 0..import.path.segments.len().saturating_sub(1) {
                prefixes.push(
                    bindings
                        .path(candidate, &import.path.prefix(index))?
                        .values
                        .into_iter()
                        .map(|r| r.address)
                        .collect(),
                );
            }
            edges.push(Edge {
                resolution,
                import: import.clone(),
                prefixes,
            });
        }
        let (inputs, modules) = bindings.inputs();
        Ok(Self {
            module,
            declarations,
            edges,
            inputs,
            modules,
        })
    }

    pub(super) fn is_current(&self, ns: &Namespace) -> bool {
        self.modules.iter().all(|input| input.is_current(ns))
            && self.inputs.iter().all(|input| {
                ns.sources
                    .get(input.path.as_path())
                    .is_some_and(|source| source.file().content_id() == input.content)
            })
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
