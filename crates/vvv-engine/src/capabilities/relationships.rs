//! Evidence-bearing call and reference sites for an exact symbol.
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{
    ContentId, DefinitionLocation, Engine, EngineError, NavigationOrigin, NavigationQuery,
    Position, ResolutionEvidence, ResolutionOutcome, ResolutionQuery, SearchScope, Selection,
    SnapshotId, SourceAnchor, SymbolRef, UnavailableReason,
};
use serde::{Deserialize, Serialize};
use vvv_core::{CallKind, CallSite, Span, Symbol, SymbolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Callers,
    Callees,
    References,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct RelationshipBudget {
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 1024)))]
    pub max_items: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 16384)))]
    pub max_lookups: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 4096)))]
    pub max_files: usize,
}
impl Default for RelationshipBudget {
    fn default() -> Self {
        Self {
            max_bytes: 16_384,
            max_items: 64,
            max_lookups: 1024,
            max_files: 128,
        }
    }
}
impl RelationshipBudget {
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(1024..=1_048_576).contains(&self.max_bytes)
            || !(1..=1024).contains(&self.max_items)
            || !(1..=16_384).contains(&self.max_lookups)
            || !(1..=4096).contains(&self.max_files)
        {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RelationshipsQuery {
    pub origin: NavigationOrigin,
    #[serde(default)]
    pub selection: Selection,
    pub kind: RelationshipKind,
    #[serde(default)]
    pub scope: SearchScope,
    #[serde(default)]
    pub budget: RelationshipBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Relationships {
    pub snapshot: SnapshotId,
    pub subject: ResolutionOutcome,
    pub kind: RelationshipKind,
    pub scope: SearchScope,
    pub items: Vec<Relationship>,
    pub coverage: RelationshipCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Relationship {
    /// Exact identifier (or unsupported callee expression) providing the evidence.
    pub site: SourceAnchor,
    pub start: Position,
    pub spelling: String,
    /// Owner of a call site; absent for non-call references and anonymous callables.
    pub caller: Option<SymbolRef>,
    /// None denotes a non-call reference, including imports and value references.
    pub call: Option<CallKind>,
    #[serde(flatten)]
    pub resolution: RelationshipResolution,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RelationshipResolution {
    Confirmed {
        target: SymbolRef,
        evidence: ResolutionEvidence,
    },
    Ambiguous {
        candidates: Vec<DefinitionLocation>,
    },
    Unavailable {
        reason: UnavailableReason,
    },
    /// A call to a local/parameter/value binding does not identify the invoked function.
    Indirect {
        binding: Option<SymbolRef>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RelationshipLimit {
    Files,
    Lookups,
    Items,
    Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RelationshipLimitation {
    ReceiverTypes,
    IndirectTargets,
    MacroExpansion,
    AnonymousCallers,
    UnsupportedBindings,
    UnenumeratedAliases,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RelationshipCoverage {
    /// Known gaps remain even when the candidate scan finishes.
    pub limitations: Vec<RelationshipLimitation>,
    pub files_scanned: usize,
    pub files_remaining: usize,
    pub lookups: usize,
    pub omitted_items: usize,
    pub stopped_by: Option<RelationshipLimit>,
    /// Files whose plugin does not provide syntactic call facts.
    pub unsupported_files: usize,
    /// Import bindings that could not be resolved while discovering alias spellings.
    pub unresolved_imports: usize,
    /// Completion of the candidate scan, not proof of a complete runtime call graph.
    pub scan_complete: bool,
}
impl RelationshipsQuery {
    pub fn new(origin: NavigationOrigin, kind: RelationshipKind) -> Self {
        Self {
            origin,
            kind,
            selection: Selection::All,
            scope: SearchScope::default(),
            budget: RelationshipBudget::default(),
        }
    }
    pub fn execute(self, engine: &Engine) -> Result<Relationships, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<Relationships, EngineError> {
        self.budget.validate()?;
        self.scope.validate()?;
        let (mut graph, snapshot) = QuerySnapshot::capture(engine)?;
        let seed = ResolutionQuery(NavigationQuery {
            origin: self.origin,
            selection: self.selection,
        })
        .execute_in(&mut graph)?;
        let mut result = Relationships {
            snapshot: ContentId::of(
                &serde_json::to_string(&snapshot).expect("snapshot serializes"),
            )
            .into(),
            subject: seed.outcome,
            kind: self.kind,
            scope: self.scope,
            items: vec![],
            coverage: RelationshipCoverage {
                limitations: vec![
                    RelationshipLimitation::ReceiverTypes,
                    RelationshipLimitation::IndirectTargets,
                    RelationshipLimitation::MacroExpansion,
                    RelationshipLimitation::AnonymousCallers,
                    RelationshipLimitation::UnsupportedBindings,
                    RelationshipLimitation::UnenumeratedAliases,
                ],
                files_scanned: 0,
                files_remaining: 0,
                lookups: 0,
                omitted_items: 0,
                stopped_by: None,
                unsupported_files: 0,
                unresolved_imports: 0,
                scan_complete: false,
            },
        };
        if let ResolutionOutcome::Resolved { definition, .. } = &result.subject {
            let target = definition.target.clone();
            let name = definition.name.clone();
            let mut files = graph.relationship_files(&target.language, &result.scope)?;
            if self.kind == RelationshipKind::Callees {
                files.retain(|f| f.path() == target.declaration.path.as_path());
            }
            result.coverage.files_remaining = files.len();
            'files: for file in files {
                graph.check_read()?;
                if result.coverage.files_scanned == self.budget.max_files {
                    result.coverage.stopped_by = Some(RelationshipLimit::Files);
                    break;
                }
                result.coverage.files_scanned += 1;
                result.coverage.files_remaining -= 1;
                let facts = file.facts()?;
                if !facts.calls_supported {
                    result.coverage.unsupported_files += 1;
                    if self.kind != RelationshipKind::References {
                        continue;
                    }
                }
                let mut names = std::collections::BTreeSet::from([name.clone()]);
                if self.kind != RelationshipKind::Callees {
                    // Resolve import bindings before adding their local spellings.
                    // A renamed re-export is followed by the ordinary resolver.
                    let mut imports: std::collections::BTreeMap<&str, Span> = facts
                        .named_imports
                        .iter()
                        .filter(|i| i.imported != "*")
                        .map(|i| (i.local.as_str(), i.name_span))
                        .collect();
                    if !facts.named_modules {
                        for import in facts.imports.iter().filter(|i| i.declares && !i.glob) {
                            if let (Some(local), Some(span)) = (
                                import.binding(),
                                facts
                                    .tokens()
                                    .filter(|(_, _, span)| import.span.contains(span))
                                    .max_by_key(|(_, _, span)| span.end)
                                    .map(|(_, _, span)| span),
                            ) {
                                imports.insert(local.as_str(), span);
                            }
                        }
                    }
                    for (local, span) in imports {
                        graph.check_read()?;
                        if local == name
                            || (self.kind == RelationshipKind::Callers
                                && !facts.calls.iter().any(|c| {
                                    file.text().get(c.callee.start..c.callee.end) == Some(local)
                                }))
                        {
                            continue;
                        }
                        if result.coverage.lookups == self.budget.max_lookups {
                            result.coverage.stopped_by = Some(RelationshipLimit::Lookups);
                            break 'files;
                        }
                        result.coverage.lookups += 1;
                        let origin = SourceAnchor {
                            path: file.path().into(),
                            content: file.file().content_id(),
                            span,
                        };
                        let binding = ResolutionQuery(NavigationQuery::occurrence(origin))
                            .execute_in(&mut graph)?;
                        match binding.outcome {
                            ResolutionOutcome::Resolved { definition, .. }
                                if definition.target == target =>
                            {
                                names.insert(local.to_owned());
                            }
                            ResolutionOutcome::Ambiguous { candidates }
                                if candidates.iter().any(|c| c.target == target) =>
                            {
                                names.insert(local.to_owned());
                            }
                            ResolutionOutcome::Unavailable { .. } => {
                                result.coverage.unresolved_imports += 1;
                            }
                            _ => {}
                        }
                    }
                }
                let sites: Vec<(Span, Option<&CallSite>)> = match self.kind {
                    RelationshipKind::Callees => facts
                        .calls
                        .iter()
                        .filter(|c| c.owner == Some(target.name_span))
                        .map(|c| (c.callee, Some(c)))
                        .collect(),
                    RelationshipKind::Callers => facts
                        .calls
                        .iter()
                        .filter(|c| {
                            file.text()
                                .get(c.callee.start..c.callee.end)
                                .is_some_and(|n| names.contains(n))
                        })
                        .map(|c| (c.callee, Some(c)))
                        .collect(),
                    RelationshipKind::References => facts
                        .tokens()
                        .filter(|(n, _, span)| {
                            names.contains(*n)
                                && !(file.path() == target.declaration.path.as_path()
                                    && *span == target.name_span)
                        })
                        .map(|(_, _, span)| (span, facts.calls.iter().find(|c| c.callee == span)))
                        .collect(),
                };
                for (span, call) in sites {
                    graph.check_read()?;
                    let limit = if result.items.len() == self.budget.max_items {
                        Some(RelationshipLimit::Items)
                    } else if result.coverage.lookups == self.budget.max_lookups {
                        Some(RelationshipLimit::Lookups)
                    } else {
                        None
                    };
                    if let Some(limit) = limit {
                        result.coverage.stopped_by = Some(limit);
                        break 'files;
                    }
                    let site = SourceAnchor {
                        path: file.path().into(),
                        content: file.file().content_id(),
                        span,
                    };
                    let spelling = file
                        .text()
                        .get(span.start..span.end)
                        .ok_or_else(|| EngineError::InvalidAnchor {
                            path: file.path().into(),
                        })?
                        .to_owned();
                    result.coverage.lookups += 1;
                    let resolution = if call.is_some_and(|c| c.kind == CallKind::Indirect) {
                        RelationshipResolution::Indirect { binding: None }
                    } else {
                        let found = ResolutionQuery(NavigationQuery::occurrence(site.clone()))
                            .execute_in(&mut graph)?;
                        match found.outcome {
                            ResolutionOutcome::Resolved { definition, .. } => {
                                if call.is_some()
                                    && self.kind != RelationshipKind::References
                                    && !matches!(
                                        definition.target.kind,
                                        SymbolKind::Function | SymbolKind::Method
                                    )
                                {
                                    if matches!(
                                        definition.target.kind,
                                        SymbolKind::Variable
                                            | SymbolKind::Parameter
                                            | SymbolKind::Const
                                            | SymbolKind::Static
                                    ) {
                                        RelationshipResolution::Indirect {
                                            binding: Some(definition.target),
                                        }
                                    } else {
                                        RelationshipResolution::Unavailable {
                                            reason: UnavailableReason::UnsupportedContext,
                                        }
                                    }
                                } else {
                                    if self.kind != RelationshipKind::Callees
                                        && definition.target != target
                                    {
                                        continue;
                                    }
                                    RelationshipResolution::Confirmed {
                                        target: definition.target,
                                        evidence: definition.evidence,
                                    }
                                }
                            }
                            ResolutionOutcome::Ambiguous { candidates } => {
                                if self.kind != RelationshipKind::Callees
                                    && !candidates.iter().any(|c| c.target == target)
                                {
                                    continue;
                                }
                                RelationshipResolution::Ambiguous { candidates }
                            }
                            ResolutionOutcome::Unavailable { reason } => {
                                RelationshipResolution::Unavailable { reason }
                            }
                        }
                    };
                    // Unresolved incoming sites are candidates, never confirmed edges.
                    let caller = call
                        .and_then(|c| c.owner)
                        .and_then(|owner| facts.symbols.iter().find(|s| s.name_span == owner))
                        .map(|s| Relationship::symbol(&site, &target.language, s));
                    result.items.push(Relationship {
                        start: file.file().source().position(span.start),
                        site,
                        spelling,
                        caller,
                        call: call.map(|c| c.kind),
                        resolution,
                    });
                }
            }
            result.coverage.scan_complete =
                result.coverage.stopped_by.is_none() && result.coverage.unsupported_files == 0;
        }
        result.fit(&self.budget)?;
        snapshot.validate(engine)?;
        Ok(result)
    }
}
impl Relationship {
    fn symbol(site: &SourceAnchor, language: &crate::LanguageId, symbol: &Symbol) -> SymbolRef {
        SymbolRef {
            language: language.clone(),
            declaration: SourceAnchor {
                path: site.path.clone(),
                content: site.content.clone(),
                span: symbol.extent,
            },
            name_span: symbol.name_span,
            kind: symbol.kind,
        }
    }
}
impl Relationships {
    fn fit(&mut self, budget: &RelationshipBudget) -> Result<(), EngineError> {
        loop {
            let required_bytes = serde_json::to_vec(self)
                .expect("relationships serialize")
                .len();
            if required_bytes <= budget.max_bytes {
                return Ok(());
            }
            if self.items.len() <= 1 {
                return Err(EngineError::OutputLimit {
                    max_bytes: budget.max_bytes,
                    required_bytes,
                });
            }
            self.items.pop();
            self.coverage.omitted_items += 1;
            self.coverage.stopped_by = Some(RelationshipLimit::Bytes);
            self.coverage.scan_complete = false;
        }
    }
}

impl crate::report::Document {
    pub(crate) fn relationships(result: &Relationships) -> Self {
        use crate::protocol::display::{Line, Role};
        use crate::protocol::vocabulary::Plural;
        let mut report = Self::resolution(&crate::ResolutionReply {
            snapshot: result.snapshot.clone(),
            outcome: result.subject.clone(),
        });
        if !matches!(result.subject, ResolutionOutcome::Resolved { .. }) {
            return report;
        }
        report.block_body(crate::report::Block::Relationships(result.items.clone()));
        report.notes([Line::of(
            Role::Dim,
            format!(
                "{}; {} scanned; {}",
                Plural(result.items.len(), "site"),
                Plural(result.coverage.files_scanned, "file"),
                Plural(result.coverage.lookups, "lookup")
            ),
        )]);
        if !result.coverage.scan_complete {
            report.notes([Line::of(
                Role::Dim,
                "Candidate scan incomplete; narrow the scope or increase the budget",
            )]);
        }
        report.notes([Line::of(Role::Dim, "Unresolved sites are possible relationships, not confirmed calls. Receiver types, indirect targets, and macro expansion are not inferred.")]);
        report
    }
}
