//! Bounded, evidence-bearing context for one exact symbol. No semantic service required.
use crate::graph::Graph;
use crate::{
    ContentId, Engine, EngineError, NavigationOrigin, NavigationOutcome, NavigationQuery,
    Selection, SnapshotId, SourceAnchor, SourceVersion, Span, SymbolRef, UnavailableReason,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextBudget {
    /// Maximum compact JSON bytes in the result, excluding the response envelope.
    pub max_bytes: usize,
    pub max_items: usize,
    pub max_lookups: usize,
    /// Maximum source files examined for incoming same-spelling references.
    pub max_files: usize,
}
impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_bytes: 16_384,
            max_items: 12,
            max_lookups: 64,
            max_files: 64,
        }
    }
}
impl ContextBudget {
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(1024..=1_048_576).contains(&self.max_bytes)
            || !(1..=64).contains(&self.max_items)
            || !(1..=512).contains(&self.max_lookups)
            || !(1..=1024).contains(&self.max_files)
        {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextQuery {
    pub origin: NavigationOrigin,
    #[serde(default)]
    pub selection: Selection,
    #[serde(default)]
    pub budget: ContextBudget,
    /// Include incoming references with this exact spelling, validated by navigation.
    #[serde(default)]
    pub references: bool,
}
impl ContextQuery {
    pub fn new(origin: NavigationOrigin) -> Self {
        Self {
            origin,
            selection: Selection::All,
            budget: ContextBudget::default(),
            references: false,
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
                context.add(
                    target.clone(),
                    &preview.source.text,
                    ContextRelation::Definition,
                    None,
                    &self.budget,
                );
                if let Some(outer) = preview
                    .source
                    .symbols
                    .iter()
                    .filter(|s| {
                        s.extent != target.declaration.span
                            && s.extent.contains(&target.declaration.span)
                    })
                    .min_by_key(|s| s.extent.len())
                {
                    let symbol = SymbolRef {
                        language: target.language.clone(),
                        declaration: SourceAnchor {
                            span: outer.extent,
                            ..target.declaration.clone()
                        },
                        name_span: outer.name_span,
                        kind: outer.kind,
                    };
                    context.add(
                        symbol,
                        &preview.source.text,
                        ContextRelation::EnclosingDeclaration,
                        Some(target.declaration.clone()),
                        &self.budget,
                    );
                }
                let mut lookups = 0;
                for anchor in preview.identifiers {
                    if anchor.span == target.name_span {
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
                            target: related,
                            preview,
                            ..
                        } => {
                            if related.declaration.path == target.declaration.path
                                && target.declaration.span.contains(&related.declaration.span)
                            {
                                continue;
                            }
                            context.add(
                                related,
                                &preview.source.text,
                                ContextRelation::ReferencedDefinition,
                                Some(anchor),
                                &self.budget,
                            );
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
                                    if let Some(outer) = file
                                        .facts()?
                                        .symbols
                                        .iter()
                                        .filter(|s| s.extent.contains(&span))
                                        .min_by_key(|s| s.extent.len())
                                    {
                                        let symbol = SymbolRef {
                                            language: target.language.clone(),
                                            declaration: SourceAnchor {
                                                span: outer.extent,
                                                ..anchor.clone()
                                            },
                                            name_span: outer.name_span,
                                            kind: outer.kind,
                                        };
                                        let relation = if file.path().components().any(|c| {
                                            c.as_os_str() == "tests" || c.as_os_str() == "test"
                                        }) {
                                            ContextRelation::ReferenceInTestPath
                                        } else {
                                            ContextRelation::Reference
                                        };
                                        context.add(
                                            symbol,
                                            file.text(),
                                            relation,
                                            Some(anchor),
                                            &self.budget,
                                        );
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
        context.snapshot = ContentId::of(
            &serde_json::to_string(&(snapshots, observed)).expect("context inputs serialize"),
        )
        .into();
        context.fit(&self.budget)?;
        Ok(context)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReply {
    pub snapshot: SnapshotId,
    #[serde(flatten)]
    pub outcome: ContextOutcome,
    pub items: Vec<ContextItem>,
    pub omissions: ContextOmissions,
    /// Incoming scanning covers this spelling only, not every possible alias.
    pub references_by_name: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ContextOutcome {
    Resolved,
    Ambiguous { candidates: Vec<ContextCandidate> },
    Unavailable { reason: UnavailableReason },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCandidate {
    pub id: crate::MatchId,
    pub target: SymbolRef,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRelation {
    Definition,
    EnclosingDeclaration,
    ReferencedDefinition,
    Reference,
    ReferenceInTestPath,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    pub target: SymbolRef,
    pub relation: ContextRelation,
    /// The exact occurrence establishing this relationship; absent for the seed.
    pub via: Option<SourceAnchor>,
    /// Range of the returned text, which can be shorter than the target extent.
    pub excerpt: SourceAnchor,
    pub start: crate::Position,
    pub text: String,
    pub complete: bool,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        target: SymbolRef,
        source: &str,
        relation: ContextRelation,
        via: Option<SourceAnchor>,
        budget: &ContextBudget,
    ) {
        if self.items.iter().any(|item| item.target == target) {
            return;
        }
        if self.items.len() >= budget.max_items {
            self.omissions.item_limit += 1;
            return;
        }
        let extent = target.declaration.span;
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
        let complete = end == extent.end;
        if !complete {
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
        });
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
            if !item.complete {
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
