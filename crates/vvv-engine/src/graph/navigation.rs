//! Occurrence resolution over captured candidates; no whole-tree reference scan.
use super::{Candidate, Graph, Namespace};
use crate::{
    ContentId, DefinitionCandidate, DefinitionPreview, EngineError, File, Match, NavigationOrigin,
    NavigationOutcome, NavigationQuery, NavigationReply, ResolutionEvidence, SnapshotId,
    SourceAnchor, SymbolRef, UnavailableReason,
};
use std::collections::HashSet;
use vvv_core::{Address, RawMatch, Role, Symbol, SymbolKind};

/// Inputs and traversal budget for one navigation answer.
struct Navigation<'a, 'p> {
    semantic: Option<&'p crate::capabilities::semantic::SemanticNavigation<'p>>,
    provider_version: Option<crate::ProviderVersion>,
    semantic_inputs: Vec<crate::SourceFile>,
    origin: Option<SourceAnchor>,
    graph: &'a mut Graph,
    inputs: Vec<Candidate>,
    steps: usize,
}

impl Graph {
    pub fn navigate(&mut self, query: NavigationQuery) -> Result<NavigationReply, EngineError> {
        self.navigate_with(query, None)
    }
    pub(crate) fn navigate_with(
        &mut self,
        query: NavigationQuery,
        semantic: Option<&crate::capabilities::semantic::SemanticNavigation<'_>>,
    ) -> Result<NavigationReply, EngineError> {
        self.navigate_recorded(query, semantic, &mut Vec::new())
    }
    pub(crate) fn navigate_observed(
        &mut self,
        query: NavigationQuery,
        observed: &mut Vec<crate::SourceVersion>,
    ) -> Result<NavigationReply, EngineError> {
        self.navigate_recorded(query, None, observed)
    }
    fn navigate_recorded(
        &mut self,
        query: NavigationQuery,
        semantic: Option<&crate::capabilities::semantic::SemanticNavigation<'_>>,
        observed: &mut Vec<crate::SourceVersion>,
    ) -> Result<NavigationReply, EngineError> {
        let result = (|| {
            let mut navigation = Navigation {
                graph: &mut *self,
                inputs: vec![],
                steps: 0,
                semantic,
                provider_version: None,
                semantic_inputs: vec![],
                origin: None,
            };
            let selection = query.selection.clone();
            let outcome = navigation.resolve(query)?;
            let outcome = navigation.semantic(outcome)?;
            let outcome = navigation.selected(outcome, selection)?;
            navigation.finish(outcome, observed)
        })();
        if let Err(EngineError::StaleSource { path }) = &result {
            self.entries
                .retain(|entry| entry.candidate.path() != path.as_path());
            self.touched();
        }
        result
    }
    pub(crate) fn validate_versions(
        &mut self,
        versions: &[crate::SourceVersion],
    ) -> Result<(), EngineError> {
        for version in versions {
            if self.workspace.load(&version.path)?.content_id() != version.content {
                self.touched();
                return Err(EngineError::StaleSource {
                    path: version.path.clone(),
                });
            }
        }
        Ok(())
    }
}

impl Navigation<'_, '_> {
    fn semantic(&mut self, syntax: NavigationOutcome) -> Result<NavigationOutcome, EngineError> {
        use crate::{SemanticFailure, SemanticTarget};
        let Some(semantic) = self.semantic else {
            return Ok(syntax);
        };
        semantic.cancellation.check()?;
        if !matches!(
            syntax,
            NavigationOutcome::Unavailable {
                reason: UnavailableReason::UnsupportedContext | UnavailableReason::Unresolved
            }
        ) {
            return Ok(syntax);
        }
        let Some(origin) = self.origin.clone() else {
            return Ok(syntax);
        };
        let version = semantic.provider.version();
        if version.provider.is_empty() || version.revision.is_empty() {
            return Err(EngineError::InvalidSemantic);
        }
        self.provider_version = Some(version.clone());
        let source = self.capture(&origin.path)?.text().to_owned();
        let request = crate::SemanticRequest {
            version: version.clone(),
            origin: origin.clone(),
            source,
        };
        let reply = match semantic.provider.navigate(&request, semantic.cancellation) {
            Ok(reply) => reply,
            Err(SemanticFailure::Unavailable) => return Ok(syntax),
            Err(SemanticFailure::Incomplete) => return Err(EngineError::NavigationLimit),
            Err(SemanticFailure::Cancelled) => return Err(EngineError::NavigationCancelled),
        };
        semantic.cancellation.check()?;
        if reply.version != version || semantic.provider.version() != version {
            return Err(EngineError::StaleSemantic);
        }
        if reply.origin != origin {
            return Err(EngineError::InvalidSemantic);
        }
        if reply.targets.len() > 1024 || reply.dependencies.len() > 1024 {
            return Err(EngineError::NavigationLimit);
        }
        for dependency in reply.dependencies {
            if !dependency
                .path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
            {
                return Err(EngineError::InvalidSemantic);
            }
            let file = self.graph.workspace.load(&dependency.path)?;
            if file.content_id() != dependency.content {
                return Err(EngineError::StaleSource {
                    path: dependency.path,
                });
            }
            self.semantic_inputs.push(file);
        }
        let mut candidates = Vec::new();
        let mut external = false;
        for target in reply.targets {
            semantic.cancellation.check()?;
            match target {
                SemanticTarget::External { .. } => external = true,
                SemanticTarget::Workspace { symbol } => {
                    if !symbol
                        .declaration
                        .path
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                    {
                        return Err(EngineError::InvalidSemantic);
                    }
                    let file = self.capture(&symbol.declaration.path)?;
                    if symbol.declaration.content != file.file().content_id() {
                        return Err(EngineError::StaleSource {
                            path: symbol.declaration.path,
                        });
                    }
                    if symbol.language != file.language() {
                        return Err(EngineError::InvalidSemantic);
                    }
                    let declared = file
                        .facts()?
                        .navigation_symbols()
                        .find(|s| {
                            s.name_span == symbol.name_span
                                && s.extent == symbol.declaration.span
                                && s.kind == symbol.kind
                        })
                        .ok_or(EngineError::InvalidSemantic)?;
                    candidates.push(DefinitionCandidate {
                        target: symbol,
                        declaration: Self::declaration(&file, declared)?,
                        evidence: ResolutionEvidence {
                            addresses: vec![],
                            semantic: Some(version.clone()),
                        },
                    });
                }
            }
        }
        // A mixed set is incomplete to this workspace, never silently narrowed.
        if external {
            return Ok(Self::unavailable(
                UnavailableReason::ExternalSourceUnavailable,
            ));
        }
        candidates.sort_by(|a, b| {
            a.declaration
                .path
                .cmp(&b.declaration.path)
                .then(a.target.name_span.start.cmp(&b.target.name_span.start))
        });
        candidates.dedup_by(|a, b| a.target == b.target);
        match candidates.len() {
            0 => Ok(syntax),
            1 => {
                let candidate = candidates.remove(0);
                let file = self.capture(&candidate.target.declaration.path)?;
                self.resolved(
                    &file,
                    candidate
                        .declaration
                        .symbol
                        .as_ref()
                        .expect("validated symbol"),
                    vec![],
                )
            }
            _ => Ok(NavigationOutcome::Ambiguous { candidates }),
        }
    }

    fn capture(&mut self, path: &std::path::Path) -> Result<Candidate, EngineError> {
        let candidate = self.graph.file(path)?;
        if !self.inputs.iter().any(|c| c.path() == candidate.path()) {
            self.inputs.push(candidate.clone());
        }
        Ok(candidate)
    }

    fn resolve(&mut self, query: NavigationQuery) -> Result<NavigationOutcome, EngineError> {
        let (path, expected) = match &query.origin {
            NavigationOrigin::Position {
                path,
                expected_content,
                ..
            } => (path, expected_content.as_ref()),
            NavigationOrigin::Occurrence { anchor } => (&anchor.path, Some(&anchor.content)),
            NavigationOrigin::Symbol { symbol } => {
                (&symbol.declaration.path, Some(&symbol.declaration.content))
            }
        };
        let candidate = self.capture(path)?;
        if expected.is_some_and(|id| *id != candidate.file().content_id()) {
            return Err(EngineError::StaleSource { path: path.clone() });
        }
        let invalid = || EngineError::InvalidAnchor { path: path.clone() };
        let span = match &query.origin {
            NavigationOrigin::Position { position, .. } => {
                let offset = candidate.file().source().offset(*position).ok_or_else(|| {
                    EngineError::NoSuchPosition {
                        path: path.clone(),
                        position: *position,
                    }
                })?;
                candidate
                    .facts()?
                    .tokens()
                    .find(|(_, _, s)| s.start <= offset && offset < s.end)
                    .map(|(_, _, s)| s)
            }
            NavigationOrigin::Occurrence { anchor } => {
                if anchor.span.is_empty()
                    || candidate
                        .text()
                        .get(anchor.span.start..anchor.span.end)
                        .is_none()
                {
                    return Err(invalid());
                }
                Some(anchor.span)
            }
            NavigationOrigin::Symbol { symbol } => {
                if symbol.language != candidate.language() {
                    return Err(invalid());
                }
                let declared = candidate
                    .facts()?
                    .navigation_symbols()
                    .find(|s| {
                        s.name_span == symbol.name_span
                            && s.extent == symbol.declaration.span
                            && s.kind == symbol.kind
                    })
                    .ok_or_else(invalid)?;
                return self.resolved(&candidate, declared, vec![]);
            }
        };
        let Some(span) = span else {
            return Ok(Self::unavailable(UnavailableReason::NoIdentifier));
        };
        self.origin = Some(SourceAnchor {
            path: candidate.path().into(),
            content: candidate.file().content_id(),
            span,
        });
        let facts = candidate.facts()?;
        if let Some(symbol) = facts
            .navigation_symbols()
            .find(|s| s.name_span == span && s.kind != SymbolKind::Impl)
        {
            return self.resolved(&candidate, symbol, vec![]);
        }
        let Some((name, _, _)) = facts.tokens().find(|(_, _, s)| *s == span) else {
            return Ok(Self::unavailable(UnavailableReason::NoIdentifier));
        };
        let namespace = if facts.navigation_types.contains(&span) {
            vvv_core::BindingNamespace::Type
        } else {
            vvv_core::BindingNamespace::Value
        };
        if facts.lexical_tokens.contains(&span)
            && let Some(binding) = facts
                .lexical
                .iter()
                .filter(|b| b.visible(name, span, namespace))
                .min_by_key(|b| (b.scope.len(), std::cmp::Reverse(b.visible_from)))
        {
            return self.resolved(&candidate, &binding.symbol, vec![]);
        }
        if facts.lexical.iter().any(|b| {
            b.symbol.name == name
                && b.namespace == namespace
                && b.scope.contains(&span)
                && b.excluded.iter().any(|s| s.contains(&span))
        }) {
            return Ok(Self::unavailable(UnavailableReason::UnsupportedContext));
        }
        let imported_here = facts
            .named_imports
            .iter()
            .any(|i| i.name_span == span || i.alias_span == span);
        let qualified = facts.qualified_imports.iter().find(|q| q.span == span);
        if !facts.navigation.contains(&span) && !imported_here && qualified.is_none() {
            return Ok(Self::unavailable(UnavailableReason::UnsupportedContext));
        }
        let Some(ns) = self.graph.namespace(&candidate.language()) else {
            return Ok(Self::unavailable(UnavailableReason::UnsupportedContext));
        };
        self.validate_project(&ns)?;
        let fragment = candidate.fragment(&ns)?;
        let bindings = if facts.named_modules {
            let mut direct = Vec::new();
            let mut explicit = false;
            for binding in &facts.named_imports {
                if (binding.local == qualified.map_or(name, |q| q.binding.as_str())
                    && !binding.reexport)
                    || binding.name_span == span
                    || binding.alias_span == span
                {
                    explicit = true;
                    if (!binding.type_only
                        || namespace == vvv_core::BindingNamespace::Type
                        || imported_here)
                        && let Some(module) = binding
                            .module
                            .as_ref()
                            .and_then(|m| ns.resolve(candidate.path(), m))
                            .or_else(|| {
                                binding
                                    .module
                                    .is_none()
                                    .then(|| ns.address(candidate.path()).ok())
                                    .flatten()
                            })
                    {
                        if binding.imported == "*" {
                            if let Some(q) = qualified {
                                direct.push(module.join(&q.member));
                            }
                        } else if qualified.is_none() {
                            direct.push(module.join(&binding.imported));
                        }
                    }
                }
            }
            if !explicit
                && qualified.is_none()
                && let Ok(module) = ns.address(candidate.path())
            {
                direct.push(module.join(name));
            }
            super::scope::Bindings {
                direct,
                opened: vec![],
                explicit,
            }
        } else {
            candidate.scope(&ns)?.navigation(span, name, &fragment)
        };
        let mut candidates = Vec::new();
        let mut reason = UnavailableReason::Unresolved;
        for addresses in [bindings.direct, bindings.opened] {
            for address in addresses {
                self.targets(&ns, address, vec![], &mut candidates, &mut reason, false)?;
            }
            if facts.navigation_types.contains(&span) {
                candidates.retain(|c| {
                    matches!(
                        c.target.kind,
                        SymbolKind::Struct
                            | SymbolKind::Enum
                            | SymbolKind::Trait
                            | SymbolKind::TypeAlias
                            | SymbolKind::Class
                            | SymbolKind::Interface
                    )
                });
            }
            if !candidates.is_empty() || bindings.explicit {
                break;
            }
        }
        candidates.sort_by(|a, b| {
            a.declaration
                .path
                .cmp(&b.declaration.path)
                .then(a.target.name_span.start.cmp(&b.target.name_span.start))
        });
        candidates.dedup_by(|a, b| a.target == b.target);
        match candidates.len() {
            0 => Ok(Self::unavailable(reason)),
            1 => {
                let chosen = candidates.remove(0);
                let file = self.capture(&chosen.target.declaration.path)?;
                let symbol = file
                    .facts()?
                    .navigation_symbols()
                    .find(|s| {
                        s.name_span == chosen.target.name_span && s.kind == chosen.target.kind
                    })
                    .ok_or_else(|| EngineError::InvalidAnchor {
                        path: chosen.target.declaration.path.clone(),
                    })?;
                self.resolved(&file, symbol, chosen.evidence.addresses)
            }
            _ => Ok(NavigationOutcome::Ambiguous { candidates }),
        }
    }

    /// The shared project loader tolerates unreadable manifests for other
    /// capabilities. Navigation cannot use that incomplete configuration as
    /// proof of a target or of external source.
    fn validate_project(&self, ns: &Namespace) -> Result<(), EngineError> {
        let captured = self.graph.project_sources.get(&ns.id());
        for absolute in &self.graph.walked {
            if !absolute.file_name().is_some_and(|name| {
                ns.layout()
                    .manifests()
                    .iter()
                    .any(|manifest| name == *manifest)
            }) {
                continue;
            }
            let path = self.graph.workspace.relative(absolute);
            if !captured.is_some_and(|sources| sources.iter().any(|source| source.path() == path)) {
                // Propagate a persistent I/O error; a successful retry still
                // requires rebuilding the project from these recovered bytes.
                self.graph.workspace.load(&path)?;
                return Err(EngineError::StaleSource { path: path.into() });
            }
        }
        Ok(())
    }

    fn selected(
        &mut self,
        outcome: NavigationOutcome,
        selection: crate::Selection,
    ) -> Result<NavigationOutcome, EngineError> {
        if selection.is_all() {
            return Ok(outcome);
        }
        let candidates = match &outcome {
            NavigationOutcome::Resolved { preview, .. } => vec![preview.declaration.clone()],
            NavigationOutcome::Ambiguous { candidates } => {
                candidates.iter().map(|c| c.declaration.clone()).collect()
            }
            NavigationOutcome::Unavailable { .. } => vec![],
        };
        let kept = selection.narrow(candidates)?;
        if kept.len() != 1 {
            return Err(EngineError::NavigationSelection);
        }
        let NavigationOutcome::Ambiguous { candidates } = outcome else {
            return Ok(outcome);
        };
        let chosen = candidates
            .into_iter()
            .find(|c| c.declaration.id == kept[0].id)
            .expect("validated candidate");
        let file = self.capture(&chosen.target.declaration.path)?;
        let symbol = chosen
            .declaration
            .symbol
            .as_ref()
            .expect("declaration candidate");
        self.resolved(&file, symbol, chosen.evidence.addresses)
    }

    fn unavailable(reason: UnavailableReason) -> NavigationOutcome {
        NavigationOutcome::Unavailable { reason }
    }

    fn targets(
        &mut self,
        ns: &Namespace,
        address: Address,
        mut trail: Vec<Address>,
        output: &mut Vec<DefinitionCandidate>,
        reason: &mut UnavailableReason,
        local_export: bool,
    ) -> Result<(), EngineError> {
        self.steps += 1;
        if self.steps > 1024 || trail.len() >= 128 {
            return Err(EngineError::NavigationLimit);
        }
        let local_lookup = local_export && trail.last() == Some(&address);
        if trail.contains(&address) && !local_lookup {
            *reason = UnavailableReason::CyclicImports;
            return Ok(());
        }
        if !local_lookup {
            trail.push(address.clone());
        }
        let Some(module) = address.parent() else {
            return Ok(());
        };
        let Some(path) = ns.file_of(&module) else {
            if !ns.project().packages.is_member(address.package()) {
                *reason = UnavailableReason::ExternalSourceUnavailable;
            }
            return Ok(());
        };
        let file = self.capture(&path)?;
        let fragment = file.fragment(ns)?;
        if fragment.module.as_ref() != Some(&module) {
            return Ok(());
        }
        let mut declared = false;
        for d in &fragment.declarations {
            let default_export = address
                .path()
                .last()
                .is_some_and(|n| n.as_str() == "default")
                && file
                    .facts()?
                    .non_named_exports
                    .contains(&d.symbol.name_span);
            if d.address != address && !default_export {
                continue;
            }
            let exported_locally = file.facts()?.named_imports.iter().any(|i| {
                i.reexport
                    && i.module.is_none()
                    && i.imported == d.symbol.name
                    && address.path().last().is_some_and(|n| i.local == n.as_str())
            });
            if file.facts()?.named_modules
                && !local_export
                && !exported_locally
                && let Some(origin) = self.inputs.first()
                && let Ok(from) = ns.address(origin.path())
                && !d.reach.admits(&from)
            {
                continue;
            }
            if self
                .inputs
                .first()
                .is_some_and(|origin| origin.path() != file.path())
                && !default_export
                && !local_export
                && !exported_locally
                && file
                    .facts()?
                    .non_named_exports
                    .contains(&d.symbol.name_span)
            {
                continue;
            }
            // Fragment also holds nested items. Their lexical addresses are not
            // represented by the file module and must not confirm navigation.
            if file
                .facts()?
                .symbols
                .iter()
                .any(|outer| outer.span != d.symbol.span && outer.span.contains(&d.symbol.span))
            {
                continue;
            }
            declared = true;
            output.push(DefinitionCandidate {
                target: Self::symbol(&file, &d.symbol),
                declaration: Self::declaration(&file, &d.symbol)?,
                evidence: ResolutionEvidence {
                    semantic: None,
                    addresses: trail.clone(),
                },
            });
        }
        if declared {
            return Ok(());
        }
        let Some(name) = address.path().last() else {
            return Ok(());
        };
        if file.facts()?.named_modules {
            for binding in &file.facts()?.named_imports {
                if (if local_export {
                    !binding.reexport
                } else {
                    binding.reexport
                }) && binding.local == name.as_str()
                    && let Some(module) = binding
                        .module
                        .as_ref()
                        .and_then(|m| ns.resolve(file.path(), m))
                        .or_else(|| binding.module.is_none().then_some(module.clone()))
                {
                    self.targets(
                        ns,
                        module.join(&binding.imported),
                        trail.clone(),
                        output,
                        reason,
                        binding.module.is_none(),
                    )?;
                }
            }
            return Ok(());
        }
        for edge in fragment.imports().filter(|e| e.import.reexport) {
            let Some(base) = edge.address() else {
                continue;
            };
            let next = if edge.import.glob {
                base.join(name.as_str())
            } else if edge.import.binding() == Some(name) {
                base.clone()
            } else {
                continue;
            };
            self.targets(ns, next, trail.clone(), output, reason, false)?;
        }
        Ok(())
    }

    fn symbol(file: &Candidate, symbol: &Symbol) -> SymbolRef {
        SymbolRef {
            language: file.language(),
            declaration: SourceAnchor {
                path: file.path().into(),
                content: file.file().content_id(),
                span: symbol.extent,
            },
            name_span: symbol.name_span,
            kind: symbol.kind,
        }
    }

    fn declaration(file: &Candidate, symbol: &Symbol) -> Result<Match, EngineError> {
        let mut raw = RawMatch::plain(
            symbol.span,
            "declaration",
            file.text()
                .get(symbol.span.start..symbol.span.end)
                .ok_or_else(|| EngineError::InvalidAnchor {
                    path: file.path().into(),
                })?,
        );
        raw.symbol = Some(symbol.clone());
        raw.role = Role::Declaration;
        Ok(Match::locate(raw, file.file(), file.language()))
    }

    fn resolved(
        &self,
        file: &Candidate,
        symbol: &Symbol,
        addresses: Vec<Address>,
    ) -> Result<NavigationOutcome, EngineError> {
        let facts = file.facts()?;
        let container = if symbol.kind == SymbolKind::Variant {
            facts
                .symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Enum && s.span.contains(&symbol.span))
                .min_by_key(|s| s.span.end - s.span.start)
                .unwrap_or(symbol)
        } else {
            symbol
        };
        for span in [
            symbol.span,
            symbol.extent,
            symbol.name_span,
            container.extent,
        ] {
            if span.is_empty() || file.text().get(span.start..span.end).is_none() {
                return Err(EngineError::InvalidAnchor {
                    path: file.path().into(),
                });
            }
        }
        Ok(NavigationOutcome::Resolved {
            target: Self::symbol(file, symbol),
            evidence: ResolutionEvidence {
                semantic: self.provider_version.clone(),
                addresses,
            },
            preview: Box::new(DefinitionPreview {
                container: Self::symbol(file, container),
                declaration: Self::declaration(file, symbol)?,
                source: File {
                    path: file.path().into(),
                    text: file.text().into(),
                    highlights: facts.highlights.clone(),
                    symbols: facts.navigation_symbols().cloned().collect(),
                    identifiers: facts
                        .tokens()
                        .map(|(_, _, span)| SourceAnchor {
                            path: file.path().into(),
                            content: file.file().content_id(),
                            span,
                        })
                        .collect(),
                },
                selection: symbol.name_span,
                identifiers: facts
                    .tokens()
                    .filter(|(_, _, span)| container.extent.contains(span))
                    .map(|(_, _, span)| SourceAnchor {
                        path: file.path().into(),
                        content: file.file().content_id(),
                        span,
                    })
                    .collect(),
            }),
        })
    }

    fn finish(
        self,
        outcome: NavigationOutcome,
        observed: &mut Vec<crate::SourceVersion>,
    ) -> Result<NavigationReply, EngineError> {
        let mut inputs = Vec::new();
        let mut seen = HashSet::new();
        // Validate every consulted source, including manifests. These checks
        // detect observed edits; they do not lock out external writers.
        for file in self
            .inputs
            .iter()
            .map(Candidate::file)
            .chain(self.graph.project_sources.values().flatten())
            .chain(&self.semantic_inputs)
        {
            if !seen.insert(file.path()) {
                continue;
            }
            let fresh = self.graph.workspace.load(file.path())?;
            if fresh.content_id() != file.content_id() {
                return Err(EngineError::StaleSource {
                    path: file.path().into(),
                });
            }
            inputs.push((crate::RelPath::from(file.path()), file.content_id()));
        }
        observed.extend(inputs.iter().map(|(path, content)| crate::SourceVersion {
            path: path.clone(),
            content: content.clone(),
        }));
        inputs.sort_by(|a, b| a.0.cmp(&b.0));
        let paths: Vec<_> = self
            .graph
            .walked
            .iter()
            .map(|p| crate::RelPath::from(self.graph.workspace.relative(p)))
            .collect();
        if let Some(semantic) = self.semantic {
            semantic.cancellation.check()?;
            if self
                .provider_version
                .as_ref()
                .is_some_and(|v| *v != semantic.provider.version())
            {
                return Err(EngineError::StaleSemantic);
            }
        }
        let snapshot_input = if self.provider_version.is_some() {
            serde_json::to_string(&(inputs, paths, &self.provider_version))
        } else {
            serde_json::to_string(&(inputs, paths))
        };
        let snapshot = SnapshotId(ContentId::of(
            &snapshot_input.expect("source identities serialize"),
        ));
        Ok(NavigationReply { snapshot, outcome })
    }
}
