//! Bounded, evidence-bearing context for one exact symbol. No semantic service required.
use crate::graph::Graph;
use crate::{
    ContentId, Engine, EngineError, NavigationOrigin, NavigationOutcome, NavigationQuery,
    Selection, SnapshotId, SourceAnchor, SourceVersion, Span, SymbolRef, UnavailableReason,
};
use serde::{Deserialize, Serialize};

/// Source detail requested for every context item. Bodies remain the default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContextDetail {
    #[default]
    Body,
    Signature,
}

/// Present only for signature requests; absence preserves the body contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ContextSignature {
    Available { requested: SourceAnchor },
    Unsupported,
}

/// The selected source range, shared by one-shot and paged assembly.
struct ContextExtent {
    span: Span,
    signature: Option<ContextSignature>,
}
impl ContextExtent {
    fn capture(
        detail: ContextDetail,
        target: &SymbolRef,
        file: &crate::Candidate,
    ) -> Result<Self, EngineError> {
        let declaration = target.declaration.span;
        if detail == ContextDetail::Body {
            return Ok(Self {
                span: declaration,
                signature: None,
            });
        }
        let span = file
            .facts()?
            .signatures
            .iter()
            .find(|signature| signature.name_span == target.name_span)
            .map(|signature| signature.span)
            .filter(|span| {
                declaration.contains(span)
                    && span.contains(&target.name_span)
                    && file.text().get(span.start..span.end).is_some()
            });
        Ok(match span {
            Some(span) => Self {
                span,
                signature: Some(ContextSignature::Available {
                    requested: SourceAnchor {
                        span,
                        ..target.declaration.clone()
                    },
                }),
            },
            None => Self {
                span: Span::new(declaration.start, declaration.start),
                signature: Some(ContextSignature::Unsupported),
            },
        })
    }
    fn supported(&self) -> bool {
        !matches!(self.signature, Some(ContextSignature::Unsupported))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct ContextBudget {
    /// Maximum compact JSON bytes in the result, excluding the response envelope.
    #[cfg_attr(feature = "schema", schemars(range(min = Self::MIN_BYTES, max = Self::MAX_BYTES)))]
    pub max_bytes: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = Self::MAX_ITEMS)))]
    pub max_items: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = Self::MAX_LOOKUPS)))]
    pub max_lookups: usize,
    /// Maximum source files examined for incoming same-spelling references.
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = Self::MAX_FILES)))]
    pub max_files: usize,
}
impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_bytes: Self::DEFAULT_BYTES,
            max_items: Self::DEFAULT_ITEMS,
            max_lookups: Self::DEFAULT_LOOKUPS,
            max_files: Self::DEFAULT_FILES,
        }
    }
}
impl ContextBudget {
    pub const MIN_BYTES: usize = 1024;
    pub const MAX_BYTES: usize = 1_048_576;
    pub const MAX_ITEMS: usize = 64;
    pub const MAX_LOOKUPS: usize = 512;
    pub const MAX_FILES: usize = 1024;
    pub const DEFAULT_BYTES: usize = 16_384;
    pub const DEFAULT_ITEMS: usize = 12;
    pub const DEFAULT_LOOKUPS: usize = 64;
    pub const DEFAULT_FILES: usize = 64;

    pub const MAXIMUM: Self = Self {
        max_bytes: Self::MAX_BYTES,
        max_items: Self::MAX_ITEMS,
        max_lookups: Self::MAX_LOOKUPS,
        max_files: Self::MAX_FILES,
    };

    pub fn validate(&self) -> Result<(), EngineError> {
        if !(Self::MIN_BYTES..=Self::MAX_BYTES).contains(&self.max_bytes)
            || !(1..=Self::MAX_ITEMS).contains(&self.max_items)
            || !(1..=Self::MAX_LOOKUPS).contains(&self.max_lookups)
            || !(1..=Self::MAX_FILES).contains(&self.max_files)
        {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextQuery {
    pub origin: NavigationOrigin,
    #[serde(default)]
    pub detail: ContextDetail,
    #[serde(default)]
    pub selection: Selection,
    #[serde(default)]
    pub budget: ContextBudget,
    /// Include incoming references with this exact spelling, validated by navigation.
    #[serde(default)]
    pub references: bool,
    /// Include the enclosing declaration at the requested detail as a separate item.
    #[serde(default)]
    pub include_enclosing: bool,
}
impl ContextQuery {
    pub fn new(origin: NavigationOrigin) -> Self {
        Self {
            origin,
            detail: ContextDetail::Body,
            selection: Selection::All,
            budget: ContextBudget::default(),
            references: false,
            include_enclosing: false,
        }
    }
    pub fn execute(self, engine: &Engine) -> Result<ContextReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(&mut *engine.graph()?)
    }
    pub(crate) fn execute_in(self, graph: &mut Graph) -> Result<ContextReply, EngineError> {
        self.budget.validate()?;
        let mut observed = Vec::new();
        let reply = graph.navigate_observed(
            NavigationQuery {
                origin: self.origin,
                selection: self.selection,
            },
            &mut observed,
        )?;
        let mut context = ContextReply {
            snapshot: reply.snapshot.clone(),
            outcome: ContextOutcome::Resolved,
            items: vec![],
            enclosing: None,
            omissions: ContextOmissions::default(),
            references_by_name: self.references,
        };
        let mut snapshots = vec![reply.snapshot];
        match reply.outcome {
            NavigationOutcome::Unavailable { reason } => {
                context.outcome = ContextOutcome::Unavailable { reason }
            }
            NavigationOutcome::Ambiguous { candidates } => {
                context.outcome = ContextOutcome::Ambiguous {
                    candidates: candidates
                        .into_iter()
                        .map(|c| ContextCandidate {
                            id: c.declaration.id,
                            target: c.target,
                        })
                        .collect(),
                };
            }
            NavigationOutcome::Resolved {
                target, preview, ..
            } => {
                let name = preview
                    .declaration
                    .symbol
                    .as_ref()
                    .expect("navigation declaration")
                    .name
                    .clone();
                let enclosing = ContextPending::enclosing(&target, &preview.source.symbols);
                context.enclosing = enclosing.as_ref().map(|item| item.target.clone());
                let file = graph.file(&target.declaration.path)?;
                let selected = ContextExtent::capture(self.detail, &target, &file)?;
                context.add(
                    ContextPending {
                        target: target.clone(),
                        relation: ContextRelation::Definition,
                        via: None,
                    },
                    &file,
                    self.detail,
                    &self.budget,
                )?;
                if self.include_enclosing
                    && let Some(item) = enclosing
                {
                    context.add(item, &file, self.detail, &self.budget)?;
                }
                let mut lookups = 0;
                for anchor in preview.identifiers {
                    graph.check_read()?;
                    if anchor.span == target.name_span || !selected.span.contains(&anchor.span) {
                        continue;
                    }
                    if lookups == self.budget.max_lookups {
                        context.omissions.lookup_limit += 1;
                        continue;
                    }
                    lookups += 1;
                    let reply = graph.navigate_observed(
                        NavigationQuery::occurrence(anchor.clone()),
                        &mut observed,
                    )?;
                    snapshots.push(reply.snapshot);
                    match reply.outcome {
                        NavigationOutcome::Resolved {
                            target: related, ..
                        } => {
                            if let Some(item) = ContextPending::outgoing(&target, related, anchor) {
                                let file = graph.file(&item.target.declaration.path)?;
                                context.add(item, &file, self.detail, &self.budget)?;
                            }
                        }
                        other => context.omissions.resolution(&other),
                    }
                }
                if self.references {
                    let files = graph.files(Some(&target.language));
                    context.omissions.file_limit =
                        files.len().saturating_sub(self.budget.max_files);
                    for file in files.into_iter().take(self.budget.max_files) {
                        observed.push(SourceVersion {
                            path: file.path().into(),
                            content: file.file().content_id(),
                        });
                        for (span, _) in file.facts()?.tokens_named(&name) {
                            if file.path() == target.declaration.path.as_path()
                                && span == target.name_span
                            {
                                continue;
                            }
                            if lookups == self.budget.max_lookups {
                                context.omissions.lookup_limit += 1;
                                continue;
                            }
                            lookups += 1;
                            let anchor = SourceAnchor {
                                path: file.path().into(),
                                content: file.file().content_id(),
                                span,
                            };
                            let reply = graph.navigate_observed(
                                NavigationQuery::occurrence(anchor.clone()),
                                &mut observed,
                            )?;
                            snapshots.push(reply.snapshot);
                            match reply.outcome {
                                NavigationOutcome::Resolved { target: found, .. }
                                    if found == target =>
                                {
                                    if let Some(item) =
                                        ContextPending::incoming(&target, anchor, &file)?
                                    {
                                        context.add(item, &file, self.detail, &self.budget)?;
                                    } else {
                                        context.omissions.no_container += 1;
                                    }
                                }
                                NavigationOutcome::Resolved { .. } => {}
                                other => context.omissions.resolution(&other),
                            }
                        }
                    }
                }
            }
        }
        graph.validate_versions(&observed)?;
        let identity = match self.detail {
            ContextDetail::Body => serde_json::to_string(&(snapshots, observed)),
            ContextDetail::Signature => serde_json::to_string(&(snapshots, observed, self.detail)),
        }
        .expect("context inputs serialize");
        context.snapshot = ContentId::of(&identity).into();
        context.fit(&self.budget)?;
        Ok(context)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextReply {
    pub snapshot: SnapshotId,
    #[serde(flatten)]
    pub outcome: ContextOutcome,
    pub items: Vec<ContextItem>,
    /// Location of the enclosing declaration, without repeating its body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<SymbolRef>,
    pub omissions: ContextOmissions,
    /// Incoming scanning covers this spelling only, not every possible alias.
    pub references_by_name: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ContextOutcome {
    Resolved,
    Ambiguous { candidates: Vec<ContextCandidate> },
    Unavailable { reason: UnavailableReason },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextCandidate {
    pub id: crate::MatchId,
    pub target: SymbolRef,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContextRelation {
    Definition,
    EnclosingDeclaration,
    ReferencedDefinition,
    Reference,
    ReferenceInTestPath,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextItem {
    pub target: SymbolRef,
    pub relation: ContextRelation,
    /// The exact occurrence establishing this relationship; absent for the seed.
    pub via: Option<SourceAnchor>,
    /// Range of the returned text, which can be shorter than the target extent.
    pub excerpt: SourceAnchor,
    pub start: crate::Position,
    pub text: String,
    /// Complete for the requested detail, not necessarily the whole declaration.
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<ContextSignature>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextOmissions {
    pub item_limit: usize,
    pub byte_limit: usize,
    pub lookup_limit: usize,
    pub file_limit: usize,
    pub ambiguous: usize,
    pub unavailable: usize,
    pub no_container: usize,
}
impl ContextOmissions {
    fn resolution(&mut self, outcome: &NavigationOutcome) {
        match outcome {
            NavigationOutcome::Ambiguous { .. } => self.ambiguous += 1,
            NavigationOutcome::Unavailable { .. } => self.unavailable += 1,
            _ => {}
        }
    }
}
impl ContextReply {
    fn add(
        &mut self,
        pending: ContextPending,
        file: &crate::Candidate,
        detail: ContextDetail,
        budget: &ContextBudget,
    ) -> Result<(), EngineError> {
        let ContextPending {
            target,
            relation,
            via,
        } = pending;
        let source = file.text();
        if (self.enclosing.as_ref() == Some(&target)
            && relation != ContextRelation::EnclosingDeclaration)
            || self.items.iter().any(|item| item.target == target)
        {
            return Ok(());
        }
        if self.items.len() >= budget.max_items {
            self.omissions.item_limit += 1;
            return Ok(());
        }
        let selected = ContextExtent::capture(detail, &target, file)?;
        let extent = selected.span;
        let mut end = extent
            .end
            .min(extent.start.saturating_add(budget.max_bytes / 2));
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        let excerpt = SourceAnchor {
            span: Span::new(extent.start, end),
            ..target.declaration.clone()
        };
        let complete = selected.supported() && end == extent.end;
        if end < extent.end {
            self.omissions.byte_limit += 1;
        }
        self.items.push(ContextItem {
            start: vvv_core::text::LineIndex::new(source).position(source, extent.start),
            target,
            relation,
            via,
            text: source[extent.start..end].to_owned(),
            excerpt,
            complete,
            signature: selected.signature,
        });
        Ok(())
    }
    fn fit(&mut self, budget: &ContextBudget) -> Result<(), EngineError> {
        loop {
            let bytes = serde_json::to_vec(self).expect("context serializes").len();
            if bytes <= budget.max_bytes {
                return Ok(());
            }
            if let Some(item) = self.items.last_mut() {
                if !item.text.is_empty() {
                    let mut end = item
                        .text
                        .len()
                        .saturating_sub((bytes - budget.max_bytes).max(1));
                    while !item.text.is_char_boundary(end) {
                        end -= 1;
                    }
                    item.text.truncate(end);
                    item.excerpt.span.end = item.excerpt.span.start + end;
                    if item.complete {
                        self.omissions.byte_limit += 1;
                    }
                    item.complete = false;
                } else {
                    if matches!(item.signature, Some(ContextSignature::Unsupported)) {
                        self.omissions.byte_limit += 1;
                    }
                    self.items.pop();
                }
            } else {
                // Never return a silently shortened ambiguity candidate set.
                return Err(EngineError::OutputLimit {
                    max_bytes: budget.max_bytes,
                    required_bytes: bytes,
                });
            }
        }
    }
}

impl crate::report::Document {
    pub(crate) fn context(reply: &ContextReply) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        match &reply.outcome {
            ContextOutcome::Unavailable { reason } => {
                doc.notes([Line::of(Role::Dim, reason.message())])
            }
            ContextOutcome::Ambiguous { candidates } => {
                doc.notes([Line::of(
                    Role::Dim,
                    "Several definitions match; use --select with a match id",
                )]);
                for candidate in candidates {
                    doc.body([Line::of(Role::Plain, candidate.id.to_string())
                        .and(Role::Plain, " ")
                        .and(Role::Path, candidate.target.declaration.path.to_string())]);
                }
            }
            ContextOutcome::Resolved => {}
        }
        for item in &reply.items {
            let label = match item.relation {
                ContextRelation::Definition => "Definition",
                ContextRelation::EnclosingDeclaration => "Enclosing declaration",
                ContextRelation::ReferencedDefinition => "Referenced definition",
                ContextRelation::Reference => "Confirmed reference",
                ContextRelation::ReferenceInTestPath => "Confirmed reference in a test path",
            };
            doc.body([Line::of(Role::Strong, label)
                .and(Role::Plain, "  ")
                .and(Role::Path, item.excerpt.path.to_string())
                .and(
                    Role::LineNumber,
                    format!(":{}:{}", item.start.line + 1, item.start.column + 1),
                )]);
            doc.body(item.text.lines().map(|s| Line::of(Role::Plain, s)));
            if matches!(item.signature, Some(ContextSignature::Unsupported)) {
                doc.notes([Line::of(Role::Dim, "Signature extraction is not supported for this declaration; request body detail")]);
            } else if !item.complete {
                doc.notes([Line::of(
                    Role::Dim,
                    "Excerpt shortened by the output budget",
                )]);
            }
        }
        let omissions = &reply.omissions;
        if *omissions != ContextOmissions::default() {
            doc.notes([Line::of(Role::Dim, format!("Omitted: {} item limit, {} byte limit, {} lookup limit, {} file limit, {} ambiguous, {} unavailable, {} without a declaration", omissions.item_limit, omissions.byte_limit, omissions.lookup_limit, omissions.file_limit, omissions.ambiguous, omissions.unavailable, omissions.no_container))]);
        }
        doc
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ContextPageQuery {
    pub origin: NavigationOrigin,
    #[serde(default)]
    pub detail: ContextDetail,
    #[serde(default)]
    pub selection: Selection,
    #[serde(default)]
    pub references: bool,
    /// Include the enclosing declaration at the requested detail as a separate item.
    #[serde(default)]
    pub include_enclosing: bool,
    #[serde(default)]
    pub page: crate::PageBudget,
    #[serde(default)]
    pub work: crate::WorkBudget,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename = "context")]
pub struct ContextPage {
    pub snapshot: SnapshotId,
    #[serde(flatten)]
    pub outcome: ContextOutcome,
    pub items: Vec<PagedContextItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<SymbolRef>,
    pub references_by_name: bool,
    pub work: ContextWork,
    pub unresolved: ContextUnresolved,
    pub traversal_complete: bool,
    pub next_cursor: Option<crate::Cursor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PagedContextItem {
    #[serde(flatten)]
    pub item: ContextItem,
    pub expansion: Option<crate::Cursor>,
    /// In signature mode, retrieve the entire declaration from its beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_expansion: Option<crate::Cursor>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextWork {
    pub lookups: usize,
    pub files: usize,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextUnresolved {
    pub ambiguous: usize,
    pub unavailable: usize,
    pub no_container: usize,
}
impl ContextUnresolved {
    fn resolution(&mut self, outcome: &NavigationOutcome) {
        match outcome {
            NavigationOutcome::Ambiguous { .. } => self.ambiguous += 1,
            NavigationOutcome::Unavailable { .. } => self.unavailable += 1,
            _ => {}
        }
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct ContextSeed {
    detail: ContextDetail,
    outcome: ContextOutcome,
    target: Option<SymbolRef>,
    name: String,
    initial: Vec<ContextPending>,
    enclosing: Option<SymbolRef>,
    outgoing: Vec<SourceAnchor>,
    incoming: Vec<crate::RelPath>,
    references: bool,
}
#[derive(Debug, Clone, Serialize)]
struct ContextPending {
    target: SymbolRef,
    relation: ContextRelation,
    via: Option<SourceAnchor>,
}
#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct ContextSession {
    initial: usize,
    outgoing: usize,
    file: usize,
    token: usize,
    seen: Vec<SymbolRef>,
    pending: Option<ContextPending>,
}
impl ContextPageQuery {
    pub fn execute(self, engine: &Engine) -> Result<ContextPage, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<ContextPage, EngineError> {
        self.page.validate()?;
        self.work.validate()?;
        let (mut graph, snapshot) = crate::graph::query_snapshot::QuerySnapshot::capture(engine)?;
        let seed = ContextSeed::capture(&mut graph, &self)?;
        let identity = (
            &self.origin,
            &self.selection,
            self.references,
            self.include_enclosing,
            self.detail,
        );
        let state = ContextSession {
            seen: if self.include_enclosing {
                vec![]
            } else {
                seed.enclosing.clone().into_iter().collect()
            },
            ..Default::default()
        };
        let mut session = super::pagination::PageSession::new(
            engine,
            snapshot,
            &identity,
            crate::query_store::QueryData::Context(seed),
        );
        let result = state.page(engine, &mut graph, &mut session, self.page, self.work);
        match session.finish(engine, result)? {
            crate::PageReply::Context(page) => Ok(page),
            _ => unreachable!("capability returns its own page"),
        }
    }
}
impl ContextSeed {
    fn capture(graph: &mut Graph, query: &ContextPageQuery) -> Result<Self, EngineError> {
        let reply = graph.navigate_observed(
            NavigationQuery {
                origin: query.origin.clone(),
                selection: query.selection.clone(),
            },
            &mut vec![],
        )?;
        let mut seed = Self {
            detail: query.detail,
            outcome: ContextOutcome::Resolved,
            target: None,
            name: String::new(),
            initial: vec![],
            enclosing: None,
            outgoing: vec![],
            incoming: vec![],
            references: query.references,
        };
        match reply.outcome {
            NavigationOutcome::Unavailable { reason } => {
                seed.outcome = ContextOutcome::Unavailable { reason }
            }
            NavigationOutcome::Ambiguous { candidates } => {
                seed.outcome = ContextOutcome::Ambiguous {
                    candidates: candidates
                        .into_iter()
                        .map(|c| ContextCandidate {
                            id: c.declaration.id,
                            target: c.target,
                        })
                        .collect(),
                }
            }
            NavigationOutcome::Resolved {
                target, preview, ..
            } => {
                seed.name = preview
                    .declaration
                    .symbol
                    .as_ref()
                    .expect("navigation declaration")
                    .name
                    .to_string();
                seed.initial.push(ContextPending {
                    target: target.clone(),
                    relation: ContextRelation::Definition,
                    via: None,
                });
                if let Some(item) = ContextPending::enclosing(&target, &preview.source.symbols) {
                    seed.enclosing = Some(item.target.clone());
                    if query.include_enclosing {
                        seed.initial.push(item);
                    }
                }
                let file = graph.file(&target.declaration.path)?;
                let selected = ContextExtent::capture(query.detail, &target, &file)?;
                seed.outgoing = preview
                    .identifiers
                    .into_iter()
                    .filter(|a| a.span != target.name_span && selected.span.contains(&a.span))
                    .collect();
                if query.references {
                    seed.incoming = graph
                        .files(Some(&target.language))
                        .iter()
                        .map(|f| f.path().into())
                        .collect();
                }
                seed.target = Some(target);
            }
        }
        Ok(seed)
    }
}
impl ContextSession {
    fn done(&self, seed: &ContextSeed) -> bool {
        self.pending.is_none()
            && self.initial == seed.initial.len()
            && self.outgoing == seed.outgoing.len()
            && self.file == seed.incoming.len()
    }
    /// Resolve at most the additional work budget. A pending item survives delivery stops.
    fn advance(
        &mut self,
        graph: &mut Graph,
        seed: &ContextSeed,
        budget: &crate::WorkBudget,
        page: &mut ContextPage,
        charged_file: &mut Option<usize>,
    ) -> Result<(), EngineError> {
        while self.pending.is_none() && !self.done(seed) {
            graph.check_read()?;
            if self.initial < seed.initial.len() {
                self.pending = Some(seed.initial[self.initial].clone());
                self.initial += 1;
            } else if self.outgoing < seed.outgoing.len() {
                if page.work.lookups == budget.max_lookups {
                    break;
                }
                let anchor = seed.outgoing[self.outgoing].clone();
                self.outgoing += 1;
                page.work.lookups += 1;
                let reply = graph
                    .navigate_observed(NavigationQuery::occurrence(anchor.clone()), &mut vec![])?;
                match reply.outcome {
                    NavigationOutcome::Resolved { target, .. } => {
                        let original = seed.target.as_ref().expect("resolved context");
                        self.pending = ContextPending::outgoing(original, target, anchor);
                    }
                    other => page.unresolved.resolution(&other),
                }
            } else {
                if *charged_file != Some(self.file) {
                    if page.work.files == budget.max_files {
                        break;
                    }
                    page.work.files += 1;
                    *charged_file = Some(self.file);
                }
                let file = graph.file(&seed.incoming[self.file])?;
                let tokens: Vec<_> = file.facts()?.tokens_named(&seed.name).collect();
                let original = seed.target.as_ref().expect("resolved context");
                if self.token == tokens.len() {
                    self.file += 1;
                    self.token = 0;
                    continue;
                }
                let (span, _) = tokens[self.token];
                if file.path() == original.declaration.path.as_path() && span == original.name_span
                {
                    self.token += 1;
                    continue;
                }
                if page.work.lookups == budget.max_lookups {
                    break;
                }
                self.token += 1;
                page.work.lookups += 1;
                let anchor = SourceAnchor {
                    path: file.path().into(),
                    content: file.file().content_id(),
                    span,
                };
                let reply = graph
                    .navigate_observed(NavigationQuery::occurrence(anchor.clone()), &mut vec![])?;
                match reply.outcome {
                    NavigationOutcome::Resolved { target, .. } if &target == original => {
                        self.pending = ContextPending::incoming(original, anchor, &file)?;
                        if self.pending.is_none() {
                            page.unresolved.no_container += 1;
                        }
                    }
                    other => page.unresolved.resolution(&other),
                }
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|p| self.seen.contains(&p.target))
            {
                self.pending = None;
            }
        }
        Ok(())
    }
    pub(crate) fn page(
        mut self,
        engine: &Engine,
        graph: &mut Graph,
        session: &mut super::pagination::PageSession,
        budget: crate::PageBudget,
        work: crate::WorkBudget,
    ) -> Result<crate::PageReply, EngineError> {
        use crate::{
            PageReply,
            query_store::{Checkpoint, QueryData},
        };
        let root = session.root.clone();
        let QueryData::Context(seed) = &root.data else {
            return Err(EngineError::InvalidCursor);
        };
        let mut page = ContextPage {
            enclosing: seed.enclosing.clone(),
            snapshot: root.identity.clone(),
            outcome: seed.outcome.clone(),
            items: vec![],
            references_by_name: seed.references,
            work: ContextWork::default(),
            unresolved: ContextUnresolved::default(),
            traversal_complete: false,
            next_cursor: None,
        };
        let mut charged_file = None;
        while page.items.len() < budget.max_items {
            engine.check_read()?;
            self.advance(graph, seed, &work, &mut page, &mut charged_file)?;
            let Some(pending) = self.pending.clone() else {
                break;
            };
            let source = graph.file(&pending.target.declaration.path)?;
            let selected = ContextExtent::capture(seed.detail, &pending.target, &source)?;
            let extent = selected.span;
            let body =
                (seed.detail == ContextDetail::Signature).then(|| super::excerpts::Excerpt {
                    target: pending.target.clone(),
                    requested: pending.target.declaration.span,
                    next: pending.target.declaration.span.start,
                });
            let text = &source.text()[extent.start..extent.end];
            let mut end = text.len().min(budget.max_bytes);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let mut after = self.clone();
            after.pending = None;
            after.seen.push(pending.target.clone());
            page.traversal_complete = after.done(seed);
            page.next_cursor = (!page.traversal_complete)
                .then(|| session.token(engine, &Checkpoint::Context(after.clone())));
            let minimum = text.chars().next().map_or(0, char::len_utf8);
            let mut accepted = None;
            loop {
                let excerpt = super::excerpts::Excerpt {
                    target: pending.target.clone(),
                    requested: extent,
                    next: extent.start + end,
                };
                let expansion = (end < text.len())
                    .then(|| session.token(engine, &Checkpoint::Excerpt(excerpt.clone())));
                page.items.push(PagedContextItem {
                    item: ContextItem {
                        target: pending.target.clone(),
                        relation: pending.relation,
                        via: pending.via.clone(),
                        excerpt: SourceAnchor {
                            span: Span::new(extent.start, extent.start + end),
                            ..pending.target.declaration.clone()
                        },
                        start: source.file().source().position(extent.start),
                        text: text[..end].to_owned(),
                        complete: selected.supported() && end == text.len(),
                        signature: selected.signature.clone(),
                    },
                    expansion,
                    body_expansion: body.as_ref().map(|excerpt| {
                        session.token(engine, &Checkpoint::Excerpt(excerpt.clone()))
                    }),
                });
                match budget.check(&PageReply::Context(page.clone())) {
                    Ok(()) => {
                        accepted = Some(excerpt);
                        break;
                    }
                    Err(EngineError::OutputLimit {
                        max_bytes,
                        required_bytes,
                    }) => {
                        page.items.pop();
                        if end == minimum {
                            if page.items.is_empty() {
                                return Err(EngineError::PageOutputLimit {
                                    max_bytes,
                                    required_bytes,
                                    anchor: Some(pending.target.declaration.clone()),
                                });
                            }
                            break;
                        }
                        end = end.saturating_sub(required_bytes - max_bytes).max(minimum);
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            let Some(excerpt) = accepted else {
                break;
            };
            if excerpt.next < extent.end {
                session.retain(Checkpoint::Excerpt(excerpt));
            }
            if let Some(body) = body {
                session.retain(Checkpoint::Excerpt(body));
            }
            self = after;
        }
        page.traversal_complete = self.done(seed);
        let checkpoint = Checkpoint::Context(self);
        page.next_cursor = (!page.traversal_complete).then(|| session.token(engine, &checkpoint));
        let reply = PageReply::Context(page);
        budget.check(&reply)?;
        if reply.next_cursor().is_some() {
            session.retain(checkpoint);
        }
        Ok(reply)
    }
}
impl crate::report::Document {
    pub(crate) fn context_page(page: &ContextPage) -> Self {
        let context = ContextReply {
            enclosing: page.enclosing.clone(),
            snapshot: page.snapshot.clone(),
            outcome: page.outcome.clone(),
            items: page.items.iter().map(|p| p.item.clone()).collect(),
            omissions: ContextOmissions {
                ambiguous: page.unresolved.ambiguous,
                unavailable: page.unresolved.unavailable,
                no_container: page.unresolved.no_container,
                ..Default::default()
            },
            references_by_name: page.references_by_name,
        };
        Self::context(&context)
    }
}

impl ContextPending {
    /// Shared relationship rules for one-shot and paged context assembly.
    fn enclosing(target: &SymbolRef, symbols: &[crate::Symbol]) -> Option<Self> {
        let outer = symbols
            .iter()
            .filter(|s| {
                s.extent != target.declaration.span && s.extent.contains(&target.declaration.span)
            })
            .min_by_key(|s| s.extent.len())?;
        Some(Self {
            target: SymbolRef {
                language: target.language.clone(),
                declaration: SourceAnchor {
                    span: outer.extent,
                    ..target.declaration.clone()
                },
                name_span: outer.name_span,
                kind: outer.kind,
            },
            relation: ContextRelation::EnclosingDeclaration,
            via: Some(target.declaration.clone()),
        })
    }
    fn outgoing(original: &SymbolRef, target: SymbolRef, anchor: SourceAnchor) -> Option<Self> {
        if target.declaration.path == original.declaration.path
            && original.declaration.span.contains(&target.declaration.span)
        {
            return None;
        }
        Some(Self {
            target,
            relation: ContextRelation::ReferencedDefinition,
            via: Some(anchor),
        })
    }
    fn incoming(
        original: &SymbolRef,
        anchor: SourceAnchor,
        file: &crate::Candidate,
    ) -> Result<Option<Self>, EngineError> {
        let Some(outer) = file
            .facts()?
            .symbols
            .iter()
            .filter(|s| s.extent.contains(&anchor.span))
            .min_by_key(|s| s.extent.len())
        else {
            return Ok(None);
        };
        let target = SymbolRef {
            language: original.language.clone(),
            declaration: SourceAnchor {
                span: outer.extent,
                ..anchor.clone()
            },
            name_span: outer.name_span,
            kind: outer.kind,
        };
        let relation = if file
            .path()
            .components()
            .any(|c| c.as_os_str() == "test" || c.as_os_str() == "tests")
        {
            ContextRelation::ReferenceInTestPath
        } else {
            ContextRelation::Reference
        };
        Ok(Some(Self {
            target,
            relation,
            via: Some(anchor),
        }))
    }
}
