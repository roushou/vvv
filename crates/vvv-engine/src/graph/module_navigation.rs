//! Navigation-only module ownership and scoped imports over captured language facts.
use super::{Namespace, candidate::SourceFacts};
use crate::{EngineError, Reach, RelPath, UnavailableReason};
use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};
use vvv_core::{Address, BindingNamespace, ImportRef, ModulePath, ModuleScope, PathHead, Span};

#[derive(Clone)]
struct Module {
    source: Arc<SourceFacts>,
    scope: ModuleScope,
    address: Address,
    locals: Vec<vvv_core::ImportScope>,
}
#[derive(Clone)]
struct Target {
    address: Address,
    authority: Address,
    trail: Vec<Address>,
}
pub(super) struct ModuleTarget {
    pub path: RelPath,
    pub name_span: Span,
    pub trail: Vec<Address>,
}
pub(super) struct ModuleLookup {
    pub targets: Vec<ModuleTarget>,
    pub inputs: BTreeSet<RelPath>,
    pub reason: UnavailableReason,
}
pub(super) struct ModuleNavigation<'a> {
    ns: &'a Namespace,
    inputs: BTreeSet<RelPath>,
    active: HashSet<(RelPath, Span)>,
    steps: usize,
    resolving: HashSet<Address>,
    reason: UnavailableReason,
}
impl<'a> ModuleNavigation<'a> {
    pub(super) fn new(ns: &'a Namespace) -> Self {
        Self {
            ns,
            inputs: BTreeSet::new(),
            active: HashSet::new(),
            steps: 0,
            resolving: HashSet::new(),
            reason: UnavailableReason::Unresolved,
        }
    }
    fn tick(&mut self) -> Result<(), EngineError> {
        self.ns.check_read()?;
        self.steps += 1;
        if self.steps > 1024 || self.active.len() >= 128 || self.resolving.len() >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        Ok(())
    }
    fn observe(&mut self, source: &SourceFacts) {
        self.inputs.insert(source.path().into());
    }
    fn owners(&mut self, address: &Address) -> Result<Vec<Module>, EngineError> {
        self.tick()?;
        let mut result = Vec::new();
        let mut at = Some(address.clone());
        while let Some(base) = at {
            for source in self.ns.module_sources(&base) {
                self.observe(&source);
                for scope in &source.facts()?.module_scopes {
                    if base.extend(scope.path.iter().cloned()) == *address {
                        result.push(Module {
                            locals: vec![],
                            source: source.clone(),
                            scope: scope.clone(),
                            address: address.clone(),
                        });
                    }
                }
            }
            at = base.parent();
        }
        Ok(result)
    }
    fn restriction(&self, module: &Module, path: &ModulePath) -> Option<Address> {
        match path.head {
            PathHead::Here => Some(module.address.extend(path.segments.iter().cloned())),
            PathHead::Up(n) => (0..n)
                .try_fold(module.address.clone(), |base, _| base.parent())
                .map(|base| base.extend(path.segments.iter().cloned())),
            _ => self.ns.resolve(module.source.path(), path),
        }
    }
    fn visible(
        &self,
        module: &Module,
        modifier: Option<&str>,
        restriction: Option<&ModulePath>,
        consumer: &Address,
    ) -> bool {
        Reach::of(
            self.ns.semantics().reach_kind(modifier),
            &module.address,
            restriction.and_then(|path| self.restriction(module, path)),
        )
        .admits(consumer)
    }
    fn binding(&mut self, module: &Module, import: &ImportRef) -> Result<Vec<Target>, EngineError> {
        self.tick()?;
        let key = (RelPath::from(module.source.path()), import.span);
        if !self.active.insert(key.clone()) {
            self.reason = UnavailableReason::CyclicImports;
            return Ok(vec![]);
        }
        let mut owner = module.clone();
        owner
            .locals
            .retain(|scope| scope.span.contains(&import.span));
        let result = self.path(&owner, &import.path, &owner.address, vec![]);
        self.active.remove(&key);
        result
    }
    fn path(
        &mut self,
        module: &Module,
        path: &ModulePath,
        consumer: &Address,
        trail: Vec<Address>,
    ) -> Result<Vec<Target>, EngineError> {
        self.tick()?;
        self.observe(&module.source);
        if trail.len() >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        let direct = match path.head {
            PathHead::Package => Some(
                Address::root(module.address.package().clone())
                    .extend(path.segments.iter().cloned()),
            ),
            PathHead::Here => Some(module.address.extend(path.segments.iter().cloned())),
            PathHead::Up(n) => (0..n)
                .try_fold(module.address.clone(), |base, _| base.parent())
                .map(|base| base.extend(path.segments.iter().cloned())),
            PathHead::SelfType | PathHead::Root => None,
            PathHead::Named => {
                let Some(head) = path.first() else {
                    return Ok(vec![]);
                };
                let facts = module.source.facts()?;
                let local = module.locals.iter().find(|scope| {
                    scope.imports.iter().any(|binding| {
                        facts.imports.iter().any(|import| {
                            import.span == binding.span && import.binding() == Some(head)
                        })
                    })
                });
                let declarations = local.is_none()
                    && module
                        .scope
                        .declarations
                        .iter()
                        .filter_map(|declaration| {
                            facts
                                .symbols
                                .iter()
                                .find(|symbol| symbol.name_span == declaration.name_span)
                        })
                        .any(|symbol| {
                            symbol.name == head.as_str() && self.ns.is_addressable(symbol.kind)
                        });
                let imports: Vec<_> = local
                    .map_or(&module.scope.imports, |scope| &scope.imports)
                    .iter()
                    .filter_map(|binding| {
                        facts
                            .imports
                            .iter()
                            .find(|import| {
                                import.span == binding.span
                                    && import.declares
                                    && !import.glob
                                    && import.binding() == Some(head)
                            })
                            .cloned()
                    })
                    .collect();
                let mut result = Vec::new();
                let mut explicit = declarations;
                for import in imports {
                    // Repeated grouped module/crate prefixes are path anchors,
                    // not recursively imported aliases of one another.
                    if import.alias.is_none()
                        && import.path.head == PathHead::Named
                        && import.path.segments.len() == 1
                        && (declarations
                            || self
                                .ns
                                .resolve(module.source.path(), &import.path)
                                .is_some())
                    {
                        continue;
                    }
                    // Grouped prefixes can name an actual module with the same name.
                    if (declarations || self.ns.resolve(module.source.path(), path).is_some())
                        && self
                            .active
                            .contains(&(module.source.path().into(), import.span))
                        && import.path == *path
                    {
                        continue;
                    }
                    explicit = true;
                    for target in self.binding(module, &import)? {
                        let mut via = trail.clone();
                        via.push(module.address.join(head.clone()));
                        via.extend(target.trail);
                        if path.segments.len() == 1 {
                            result.push(Target {
                                address: target.address,
                                authority: target.authority,
                                trail: via,
                            });
                        } else {
                            result.extend(self.normalize(Target {
                                address: target.address.extend(path.segments[1..].iter().cloned()),
                                authority: consumer.clone(),
                                trail: via,
                            })?);
                        }
                    }
                }
                if declarations || !explicit {
                    result.extend(self.normalize(Target {
                        address: module.address.extend(path.segments.iter().cloned()),
                        authority: consumer.clone(),
                        trail: trail.clone(),
                    })?);
                }
                if !explicit && let Some(address) = self.ns.resolve(module.source.path(), path) {
                    result.extend(self.normalize(Target {
                        address,
                        authority: consumer.clone(),
                        trail,
                    })?);
                }
                return Ok(result);
            }
        };
        match direct {
            Some(address) => self.normalize(Target {
                address,
                authority: consumer.clone(),
                trail,
            }),
            None => Ok(vec![]),
        }
    }
    fn normalize(&mut self, target: Target) -> Result<Vec<Target>, EngineError> {
        self.tick()?;
        if target.trail.len() >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        let mut owner = Address::root(target.address.package().clone());
        for (index, name) in target.address.path().iter().enumerate() {
            let mut found = false;
            let mut result = Vec::new();
            for module in self.owners(&owner)? {
                let facts = module.source.facts()?;
                let declared_modules: Vec<_> = module
                    .scope
                    .declarations
                    .iter()
                    .filter_map(|declaration| {
                        let symbol = facts.symbols.iter().find(|symbol| {
                            symbol.name_span == declaration.name_span
                                && symbol.name == name.as_str()
                                && symbol.kind == vvv_core::SymbolKind::Module
                        })?;
                        Some((symbol, declaration))
                    })
                    .collect();
                if !declared_modules.is_empty()
                    && declared_modules.iter().all(|(symbol, declaration)| {
                        !self.visible(
                            &module,
                            symbol.modifier(),
                            declaration.restriction.as_ref(),
                            &target.authority,
                        )
                    })
                {
                    return Ok(vec![]);
                }
                let bindings: Vec<_> = module
                    .scope
                    .imports
                    .iter()
                    .filter_map(|binding| {
                        let import = facts.imports.iter().find(|import| {
                            import.span == binding.span
                                && import.declares
                                && !import.glob
                                && import.binding() == Some(name)
                        })?;
                        if import.alias.is_none()
                            && import.path.head == PathHead::Named
                            && import.path.segments.len() == 1
                            && (!declared_modules.is_empty()
                                || self
                                    .ns
                                    .resolve(module.source.path(), &import.path)
                                    .is_some())
                        {
                            return None;
                        }
                        // An identity import prefix contributes no new binding address.
                        if self.restriction(&module, &import.path).as_ref()
                            == Some(&owner.join(name.clone()))
                        {
                            return None;
                        }
                        Some((binding.clone(), import.clone()))
                    })
                    .collect();
                for (binding, import) in bindings {
                    found = true;
                    if !self.visible(
                        &module,
                        binding
                            .visibility
                            .as_ref()
                            .map(|modifier| modifier.text.as_str()),
                        binding.restriction.as_ref(),
                        &target.authority,
                    ) {
                        continue;
                    }
                    for resolved in self.binding(&module, &import)? {
                        let mut trail = target.trail.clone();
                        trail.push(owner.join(name.clone()));
                        trail.extend(resolved.trail);
                        if index + 1 == target.address.path().len() {
                            result.push(Target {
                                address: resolved.address,
                                authority: resolved.authority,
                                trail,
                            });
                        } else {
                            result.extend(self.normalize(
                                Target {
                                    address:
                                        resolved.address.extend(
                                            target.address.path()[index + 1..].iter().cloned(),
                                        ),
                                    authority: target.authority.clone(),
                                    trail,
                                },
                            )?);
                        }
                    }
                }
                // Conditional declarations and imported bindings remain alternatives.
                if found
                    && module.scope.declarations.iter().any(|declaration| {
                        facts.symbols.iter().any(|symbol| {
                            symbol.name_span == declaration.name_span
                                && symbol.name == name.as_str()
                        })
                    })
                {
                    result.push(target.clone());
                }
            }
            if found {
                return Ok(result);
            }
            owner = owner.join(name.clone());
        }
        Ok(vec![target])
    }
    fn declarations(
        &mut self,
        target: Target,
        namespace: BindingNamespace,
        output: &mut Vec<ModuleTarget>,
    ) -> Result<(), EngineError> {
        let address = target.address.clone();
        if !self.resolving.insert(address.clone()) {
            self.reason = UnavailableReason::CyclicImports;
            return Ok(());
        }
        let result = self.declaration_targets(target, namespace, output);
        self.resolving.remove(&address);
        result
    }
    fn declaration_targets(
        &mut self,
        target: Target,
        namespace: BindingNamespace,
        output: &mut Vec<ModuleTarget>,
    ) -> Result<(), EngineError> {
        self.tick()?;
        let Some(owner) = target.address.parent() else {
            return Ok(());
        };
        let owners = self.owners(&owner)?;
        if owners.is_empty()
            && target.address.package() != target.authority.package()
            && !self
                .ns
                .project()
                .packages
                .is_member(target.address.package())
        {
            self.reason = UnavailableReason::ExternalSourceUnavailable;
        }
        for module in owners {
            let facts = module.source.facts()?;
            let before = output.len();
            for declaration in &module.scope.declarations {
                let Some(symbol) = facts
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name_span == declaration.name_span)
                else {
                    continue;
                };
                if module.address.join(symbol.name.as_str()) != target.address
                    || !self.ns.is_addressable(symbol.kind)
                {
                    continue;
                }
                if namespace == BindingNamespace::Type
                    && !matches!(
                        symbol.kind,
                        vvv_core::SymbolKind::Struct
                            | vvv_core::SymbolKind::Enum
                            | vvv_core::SymbolKind::Trait
                            | vvv_core::SymbolKind::TypeAlias
                            | vvv_core::SymbolKind::Module
                    )
                {
                    continue;
                }
                if !self.visible(
                    &module,
                    symbol.modifier(),
                    declaration.restriction.as_ref(),
                    &target.authority,
                ) {
                    continue;
                }
                let mut trail = target.trail.clone();
                if trail.last() != Some(&target.address) {
                    trail.push(target.address.clone());
                }
                output.push(ModuleTarget {
                    path: module.source.path().into(),
                    name_span: symbol.name_span,
                    trail,
                });
            }
            if output.len() == before {
                for binding in &module.scope.imports {
                    let Some(import) = facts
                        .imports
                        .iter()
                        .find(|import| import.span == binding.span && import.glob)
                    else {
                        continue;
                    };
                    if !self.visible(
                        &module,
                        binding
                            .visibility
                            .as_ref()
                            .map(|modifier| modifier.text.as_str()),
                        binding.restriction.as_ref(),
                        &target.authority,
                    ) {
                        continue;
                    }
                    for opened in self.binding(&module, import)? {
                        let Some(name) = target.address.path().last() else {
                            continue;
                        };
                        let normalized = self.normalize(Target {
                            address: opened.address.join(name.clone()),
                            authority: target.authority.clone(),
                            trail: opened.trail,
                        })?;
                        for candidate in normalized {
                            self.declarations(candidate, namespace, output)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn resolve(
        mut self,
        source: Arc<SourceFacts>,
        span: Span,
        name: &str,
        namespace: BindingNamespace,
    ) -> Result<ModuleLookup, EngineError> {
        self.observe(&source);
        let facts = source.facts()?;
        let Some(scope) = facts
            .module_scopes
            .iter()
            .filter(|scope| scope.span.contains(&span))
            .min_by_key(|scope| scope.span.len())
            .cloned()
        else {
            return Ok(ModuleLookup {
                targets: vec![],
                inputs: self.inputs,
                reason: UnavailableReason::UnsupportedContext,
            });
        };
        let base = self.ns.address(source.path())?;
        let module = Module {
            locals: {
                let mut locals: Vec<_> = facts
                    .import_scopes
                    .iter()
                    .filter(|imports| {
                        imports.span.contains(&span) && scope.span.contains(&imports.span)
                    })
                    .cloned()
                    .collect();
                locals.sort_by_key(|imports| imports.span.len());
                locals
            },
            address: base.extend(scope.path.iter().cloned()),
            source: source.clone(),
            scope,
        };
        let import = facts
            .imports
            .iter()
            .filter(|import| import.span.contains(&span))
            .min_by_key(|import| import.span.len());
        let path = if let Some(import) = import {
            let skipped = import
                .group
                .as_ref()
                .and_then(|group| import.path.strip_prefix(&group.prefix))
                .map_or(0, |rest| import.path.segments.len() - rest.len());
            let spelled = if skipped == 0 {
                import.path.clone()
            } else {
                ModulePath::new(
                    import.path.syntax(),
                    PathHead::Named,
                    import.path.segments[skipped..].iter().cloned(),
                )
            };
            match spelled.segment_at(span.start - import.span.start) {
                Some(index) => import.path.prefix(index + skipped),
                None => import.path.clone(),
            }
        } else {
            ModulePath::new(vvv_core::PathSyntax::Scoped, PathHead::Named, [name])
        };
        let mut addresses = self.path(&module, &path, &module.address, vec![])?;
        let explicit = module.scope.imports.iter().any(|binding| {
            facts.imports.iter().any(|import| {
                import.span == binding.span
                    && !import.glob
                    && import.binding().is_some_and(|bound| bound.as_str() == name)
            })
        });
        let mut targets = Vec::new();
        for address in addresses.drain(..) {
            self.declarations(address, namespace, &mut targets)?;
        }
        if targets.is_empty() && import.is_none() && !explicit {
            for binding in &module.scope.imports {
                if let Some(import) = facts
                    .imports
                    .iter()
                    .find(|import| import.span == binding.span && import.glob)
                {
                    for opened in self.binding(&module, import)? {
                        let addresses = self.normalize(Target {
                            address: opened.address.join(name),
                            authority: module.address.clone(),
                            trail: opened.trail,
                        })?;
                        for address in addresses {
                            self.declarations(address, namespace, &mut targets)?;
                        }
                    }
                }
            }
        }
        Ok(ModuleLookup {
            targets,
            inputs: self.inputs,
            reason: self.reason,
        })
    }
}
