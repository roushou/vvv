//! Evidence-bearing call and reference sites for an exact symbol.
use super::incoming::{DiscoveryWork, IncomingReferences};
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{
    DefinitionLocation, Engine, EngineError, NavigationOrigin, NavigationQuery, Position,
    ResolutionEvidence, ResolutionOutcome, ResolutionQuery, SearchScope, Selection, SnapshotId,
    SourceAnchor, SymbolRef, UnavailableReason,
};
use serde::{Deserialize, Serialize};
use vvv_core::{CallKind, Symbol, SymbolKind};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<crate::Cursor>,
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
            origin: self.origin.clone(),
            selection: self.selection.clone(),
        })
        .execute_in(&mut graph)?;
        let data = RelationshipSeed {
            subject: seed.outcome,
            kind: self.kind,
            scope: self.scope.clone(),
            files: vec![],
        };
        let mut data = data;
        if let ResolutionOutcome::Resolved { definition, .. } = &data.subject {
            data.files = graph
                .relationship_files(&definition.target.language, &data.scope)?
                .into_iter()
                .filter(|file| {
                    data.kind != RelationshipKind::Callees
                        || file.path() == definition.target.declaration.path.as_path()
                })
                .map(|file| file.path().into())
                .collect();
        }
        let mut session = super::pagination::PageSession::new(
            engine,
            snapshot,
            &(self.origin, self.selection, self.kind, self.scope),
            crate::query_store::QueryData::Relationships(data),
        );
        let reply =
            RelationshipSession::default().page(engine, &mut graph, &mut session, self.budget);
        match session.finish(engine, reply)? {
            crate::PageReply::Relationships(reply) => Ok(reply),
            _ => unreachable!("capability returns its own page"),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct RelationshipSeed {
    subject: ResolutionOutcome,
    kind: RelationshipKind,
    scope: SearchScope,
    files: Vec<crate::RelPath>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub(crate) struct RelationshipSession {
    file: usize,
    incoming: Option<IncomingReferences>,
    pending: Option<Relationship>,
    unsupported_files: usize,
}

impl RelationshipSession {
    fn done(&self, seed: &RelationshipSeed) -> bool {
        self.pending.is_none()
            && (self.file == seed.files.len()
                || (self.file + 1 == seed.files.len()
                    && self.incoming.as_ref().is_some_and(|incoming| {
                        incoming
                            .sites
                            .as_ref()
                            .is_some_and(|sites| incoming.site == sites.len())
                    })))
    }

    fn cursor(
        &self,
        engine: &Engine,
        session: &super::pagination::PageSession,
        seed: &RelationshipSeed,
    ) -> Option<crate::Cursor> {
        (!self.done(seed)).then(|| {
            session.token(
                engine,
                &crate::query_store::Checkpoint::Relationships(self.clone()),
            )
        })
    }

    pub(crate) fn page(
        mut self,
        engine: &Engine,
        graph: &mut crate::graph::Graph,
        session: &mut super::pagination::PageSession,
        budget: RelationshipBudget,
    ) -> Result<crate::PageReply, EngineError> {
        let root = session.root.clone();
        let crate::query_store::QueryData::Relationships(seed) = &root.data else {
            return Err(EngineError::InvalidCursor);
        };
        let mut result = Relationships {
            snapshot: root.identity.clone(),
            subject: seed.subject.clone(),
            kind: seed.kind,
            scope: seed.scope.clone(),
            items: vec![],
            next_cursor: None,
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
                files_remaining: seed.files.len().saturating_sub(self.file),
                lookups: 0,
                omitted_items: 0,
                stopped_by: None,
                unsupported_files: self.unsupported_files,
                unresolved_imports: 0,
                scan_complete: false,
            },
        };
        let mut charged_file = None;
        while !self.done(seed) {
            graph.check_read()?;
            if result.items.len() == budget.max_items {
                result.coverage.stopped_by = Some(RelationshipLimit::Items);
                break;
            }
            if self.pending.is_none() {
                if charged_file != Some(self.file) {
                    if result.coverage.files_scanned == budget.max_files {
                        result.coverage.stopped_by = Some(RelationshipLimit::Files);
                        break;
                    }
                    result.coverage.files_scanned += 1;
                    charged_file = Some(self.file);
                }
                let file = graph.file(&seed.files[self.file])?;
                let facts = file.facts()?;
                let ResolutionOutcome::Resolved { definition, .. } = &seed.subject else {
                    break;
                };
                let target = &definition.target;
                if self.incoming.is_none() {
                    if !facts.calls_supported {
                        self.unsupported_files += 1;
                        if seed.kind != RelationshipKind::References {
                            self.file += 1;
                            continue;
                        }
                    }
                    self.incoming = Some(IncomingReferences::new(
                        &file,
                        &definition.name,
                        seed.kind == RelationshipKind::Callers,
                    )?);
                }
                let incoming = self.incoming.as_mut().expect("current file discovery");
                if seed.kind != RelationshipKind::Callees {
                    let mut work = DiscoveryWork {
                        lookups: result.coverage.lookups,
                        max_lookups: budget.max_lookups,
                        unresolved_imports: 0,
                    };
                    let ready = incoming.prepare(graph, &file, target, &mut work, &mut vec![])?;
                    result.coverage.lookups = work.lookups;
                    result.coverage.unresolved_imports += work.unresolved_imports;
                    if !ready {
                        result.coverage.stopped_by = Some(RelationshipLimit::Lookups);
                        break;
                    }
                }
                if incoming.sites.is_none() {
                    incoming.sites = Some(match seed.kind {
                        RelationshipKind::Callees => facts
                            .calls
                            .iter()
                            .filter(|call| call.owner == Some(target.name_span))
                            .map(|call| call.callee)
                            .collect(),
                        RelationshipKind::Callers => facts
                            .calls
                            .iter()
                            .filter(|call| {
                                file.text()
                                    .get(call.callee.start..call.callee.end)
                                    .is_some_and(|name| incoming.names().contains(name))
                            })
                            .map(|call| call.callee)
                            .collect(),
                        RelationshipKind::References => facts
                            .tokens()
                            .filter(|(name, _, span)| {
                                incoming.names().contains(*name)
                                    && !(file.path() == target.declaration.path.as_path()
                                        && *span == target.name_span)
                            })
                            .map(|(_, _, span)| span)
                            .collect(),
                    });
                }
                let sites = incoming.sites.as_ref().expect("discovered sites");
                let Some(&span) = sites.get(incoming.site) else {
                    self.file += 1;
                    self.incoming = None;
                    continue;
                };
                if result.coverage.lookups == budget.max_lookups {
                    result.coverage.stopped_by = Some(RelationshipLimit::Lookups);
                    break;
                }
                incoming.site += 1;
                let call = facts.calls.iter().find(|call| call.callee == span);
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
                        .execute_in(graph)?;
                    match found.outcome {
                        ResolutionOutcome::Resolved { definition, .. } => {
                            if call.is_some()
                                && seed.kind != RelationshipKind::References
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
                                if seed.kind != RelationshipKind::Callees
                                    && definition.target != *target
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
                            if seed.kind != RelationshipKind::Callees
                                && !candidates.iter().any(|c| c.target == *target)
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

                let caller = call
                    .and_then(|call| call.owner)
                    .and_then(|owner| {
                        facts
                            .symbols
                            .iter()
                            .find(|symbol| symbol.name_span == owner)
                    })
                    .map(|symbol| Relationship::symbol(&site, &target.language, symbol));
                self.pending = Some(Relationship {
                    start: file.file().source().position(span.start),
                    site,
                    spelling,
                    caller,
                    call: call.map(|call| call.kind),
                    resolution,
                });
            }
            let pending = self.pending.as_ref().expect("pending relationship").clone();
            let mut after = self.clone();
            after.pending = None;
            result.items.push(pending.clone());
            result.next_cursor = after.cursor(engine, session, seed);
            result.coverage.scan_complete = after.done(seed) && after.unsupported_files == 0;
            result.coverage.files_remaining = if after.done(seed) {
                0
            } else {
                seed.files.len().saturating_sub(after.file)
            };
            let required_bytes = serde_json::to_vec(&result)
                .expect("relationships serialize")
                .len();
            if required_bytes > budget.max_bytes {
                result.items.pop();
                if result.items.is_empty() {
                    return Err(EngineError::PageOutputLimit {
                        max_bytes: budget.max_bytes,
                        required_bytes,
                        anchor: Some(pending.site),
                    });
                }
                result.coverage.stopped_by = Some(RelationshipLimit::Bytes);
                break;
            }
            self = after;
        }
        result.coverage.unsupported_files = self.unsupported_files;
        result.coverage.scan_complete = self.done(seed)
            && self.unsupported_files == 0
            && matches!(seed.subject, ResolutionOutcome::Resolved { .. });
        result.coverage.files_remaining = if self.done(seed) {
            0
        } else {
            seed.files.len().saturating_sub(self.file)
        };
        result.next_cursor = self.cursor(engine, session, seed);
        let required_bytes = serde_json::to_vec(&result)
            .expect("relationships serialize")
            .len();
        if required_bytes > budget.max_bytes {
            return Err(EngineError::PageOutputLimit {
                max_bytes: budget.max_bytes,
                required_bytes,
                anchor: None,
            });
        }
        if result.next_cursor.is_some() {
            session.retain(crate::query_store::Checkpoint::Relationships(self));
        }
        Ok(crate::PageReply::Relationships(result))
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
        report.notes([Line::single(
            Role::Dim,
            format!(
                "{}; {} scanned; {}",
                Plural(result.items.len(), "site"),
                Plural(result.coverage.files_scanned, "file"),
                Plural(result.coverage.lookups, "lookup")
            ),
        )]);
        if !result.coverage.scan_complete {
            report.notes([Line::single(
                Role::Dim,
                "Candidate scan incomplete; session clients can continue, or narrow the scope or increase the budget",
            )]);
        }
        report.notes([Line::single(Role::Dim, "Unresolved sites are possible relationships, not confirmed calls. Receiver types, indirect targets, and macro expansion are not inferred.")]);
        report
    }
}
