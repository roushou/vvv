//! Shared import binding lookup over captured source facts, without recursive cache locks.
use super::{Namespace, candidate::SourceFacts};
use crate::{EngineError, Reach, RelPath, SourceAnchor, SourceVersion};
use std::collections::{BTreeMap, HashMap, HashSet};
use vvv_core::{Address, ImportRef, ModulePath, PathHead, PathSyntax, Span};

#[derive(Debug, Clone)]
pub(super) struct BindingOrigin {
    pub anchor: SourceAnchor,
    pub address: Address,
    pub target: Address,
    pub reexport: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Resolution {
    pub address: Address,
    pub binding: Option<Span>,
    pub via: Vec<BindingOrigin>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Targets {
    pub values: Vec<Resolution>,
    pub cyclic: bool,
}
impl Targets {
    fn merge(&mut self, other: Self) {
        self.cyclic |= other.cyclic;
        for value in other.values {
            if !self.values.iter().any(|held| held.address == value.address) {
                self.values.push(value);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ModuleInput {
    address: Address,
    sources: Vec<RelPath>,
}
impl ModuleInput {
    pub(super) fn is_current(&self, ns: &Namespace) -> bool {
        self.sources
            == ns
                .module_sources(&self.address)
                .iter()
                .map(|source| source.path().into())
                .collect::<Vec<RelPath>>()
    }
}

pub(super) struct ImportBindings<'a> {
    ns: &'a Namespace,
    inputs: BTreeMap<RelPath, crate::ContentId>,
    modules: BTreeMap<Address, Vec<RelPath>>,
    imports: HashMap<(RelPath, Span), Targets>,
    active: HashSet<(RelPath, Span)>,
    steps: usize,
}
impl<'a> ImportBindings<'a> {
    pub(super) fn new(ns: &'a Namespace) -> Self {
        Self {
            ns,
            inputs: BTreeMap::new(),
            modules: BTreeMap::new(),
            imports: HashMap::new(),
            active: HashSet::new(),
            steps: 0,
        }
    }
    pub(super) fn inputs(self) -> (Vec<SourceVersion>, Vec<ModuleInput>) {
        (
            self.inputs
                .into_iter()
                .map(|(path, content)| SourceVersion { path, content })
                .collect(),
            self.modules
                .into_iter()
                .map(|(address, sources)| ModuleInput { address, sources })
                .collect(),
        )
    }
    fn observe(&mut self, source: &SourceFacts) {
        self.inputs
            .insert(source.path().into(), source.file().content_id());
    }
    fn check(&mut self) -> Result<(), EngineError> {
        self.ns.check_read()?;
        self.steps += 1;
        if self.steps > 1024 {
            return Err(EngineError::NavigationLimit);
        }
        Ok(())
    }
    pub(super) fn import(
        &mut self,
        source: &SourceFacts,
        import: &ImportRef,
    ) -> Result<Targets, EngineError> {
        self.check()?;
        let key = (RelPath::from(source.path()), import.span);
        if let Some(held) = self.imports.get(&key) {
            return Ok(held.clone());
        }
        if self.active.contains(&key) {
            return Ok(Targets {
                values: vec![],
                cyclic: true,
            });
        }
        if self.active.len() >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        self.active.insert(key.clone());
        let resolved = self.path(source, &import.path);
        self.active.remove(&key);
        let resolved = resolved?;
        // A partial result reached through a cycle is context-dependent.
        if !resolved.cyclic {
            self.imports.insert(key, resolved.clone());
        }
        Ok(resolved)
    }
    pub(super) fn path(
        &mut self,
        source: &SourceFacts,
        path: &ModulePath,
    ) -> Result<Targets, EngineError> {
        self.check()?;
        self.observe(source);
        if path.syntax() == PathSyntax::Scoped
            && path.head == PathHead::Named
            && !source.facts()?.named_modules
        {
            let bindings: Vec<_> = source
                .facts()?
                .imports
                .iter()
                .filter(|i| i.declares && i.binding() == path.first())
                .filter(|i| {
                    !(i.path == *path
                        && self.active.contains(&(source.path().into(), i.span))
                        && self.ns.resolve(source.path(), path).is_some())
                })
                .cloned()
                .collect();
            if !bindings.is_empty() {
                let mut result = Targets::default();
                for binding in bindings {
                    let targets = self.import(source, &binding)?;
                    result.cyclic |= targets.cyclic;
                    for target in targets.values {
                        let mut next = self.address(
                            source,
                            target.address.extend(path.segments[1..].iter().cloned()),
                            0,
                        )?;
                        for value in &mut next.values {
                            value.binding = Some(binding.span);
                            let mut via = vec![BindingOrigin {
                                anchor: SourceAnchor {
                                    path: source.path().into(),
                                    content: source.file().content_id(),
                                    span: binding.span,
                                },
                                target: target.address.clone(),
                                reexport: binding.reexport,
                                address: self
                                    .ns
                                    .address(source.path())
                                    .ok()
                                    .map(|m| m.join(binding.binding().unwrap().as_str()))
                                    .unwrap_or_else(|| target.address.clone()),
                            }];
                            via.extend(target.via.clone());
                            via.append(&mut value.via);
                            value.via = via;
                        }
                        result.merge(next);
                    }
                }
                return Ok(result);
            }
        }
        let Some(address) = self.ns.resolve(source.path(), path) else {
            return Ok(Targets::default());
        };
        if path.syntax() != PathSyntax::Scoped || source.facts()?.named_modules {
            return Ok(Targets {
                values: vec![Resolution {
                    address,
                    binding: None,
                    via: vec![],
                }],
                cyclic: false,
            });
        }
        self.address(source, address, 0)
    }
    fn address(
        &mut self,
        from: &SourceFacts,
        address: Address,
        depth: usize,
    ) -> Result<Targets, EngineError> {
        self.check()?;
        if depth >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        let Some(consumer) = self.ns.address(from.path()).ok() else {
            return Ok(Targets::default());
        };
        let mut owner = Address::root(address.package().clone());
        for (index, name) in address.path().iter().enumerate() {
            let mut result = Targets::default();
            let mut found = false;
            let sources = self.ns.module_sources(&owner);
            self.modules.insert(
                owner.clone(),
                sources.iter().map(|source| source.path().into()).collect(),
            );
            for source in sources {
                self.check()?;
                self.observe(&source);
                let facts = source.facts()?;
                if facts.named_modules {
                    continue;
                }
                let imports: Vec<_> = facts
                    .imports
                    .iter()
                    .filter(|i| i.declares && i.binding() == Some(name))
                    .filter(|i| {
                        self.ns.resolve(source.path(), &i.path).as_ref()
                            != Some(&owner.join(name.clone()))
                    })
                    .filter_map(|import| {
                        facts
                            .import_bindings
                            .iter()
                            .find(|b| b.span == import.span)
                            .map(|b| (import.clone(), b.clone()))
                    })
                    .collect();
                for (import, binding) in imports {
                    found = true;
                    let restriction = binding
                        .restriction
                        .as_ref()
                        .and_then(|p| self.ns.resolve(source.path(), p));
                    let reach = Reach::of(
                        self.ns
                            .semantics()
                            .reach_kind(binding.visibility.as_ref().map(|v| v.text.as_str())),
                        &owner,
                        restriction,
                    );
                    if !reach.admits(&consumer) {
                        continue;
                    }
                    let targets = self.import(&source, &import)?;
                    result.cyclic |= targets.cyclic;
                    for target in targets.values {
                        let mut next = self.address(
                            from,
                            target
                                .address
                                .extend(address.path()[index + 1..].iter().cloned()),
                            depth + 1,
                        )?;
                        for value in &mut next.values {
                            let mut via = vec![BindingOrigin {
                                anchor: SourceAnchor {
                                    path: source.path().into(),
                                    content: source.file().content_id(),
                                    span: import.span,
                                },
                                address: owner.join(name.clone()),
                                target: target.address.clone(),
                                reexport: import.reexport,
                            }];
                            via.extend(target.via.clone());
                            via.append(&mut value.via);
                            value.via = via;
                        }
                        result.merge(next);
                    }
                }
                // Competing declarations remain alternatives, never first-match winners.
                if found
                    && facts.symbols.iter().any(|s| {
                        self.ns.is_addressable(s.kind)
                            && s.name == name.as_str()
                            && !facts
                                .symbols
                                .iter()
                                .any(|outer| outer.span != s.span && outer.span.contains(&s.span))
                    })
                {
                    result.merge(Targets {
                        values: vec![Resolution {
                            address: address.clone(),
                            binding: None,
                            via: vec![],
                        }],
                        cyclic: false,
                    });
                }
            }
            if found {
                return Ok(result);
            }
            owner = owner.join(name.clone());
        }
        Ok(Targets {
            values: vec![Resolution {
                address,
                binding: None,
                via: vec![],
            }],
            cyclic: false,
        })
    }
}
