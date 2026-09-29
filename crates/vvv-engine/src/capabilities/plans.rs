//! Session-owned, reviewable rename and rewrite plans. Executable edits never cross the wire.
pub(crate) mod review;
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{
    ContentId, Engine, EngineError, Failure, MutationAnswer, Planned, RenameIntent, RewriteIntent,
    SourceVersion,
};
use review::CapturedReview;
pub use review::{
    PlanPreview, PlanReviewCursor, PlanReviewItem, PlanReviewKind, PlanReviewPage, PlanReviewReply,
    ReviewMutation, ReviewSection, ReviewTotals,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

/// Opaque handle scoped to an engine session, not a serializable edit plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PlanId(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PrepareRenameQuery {
    pub intent: RenameIntent,
    #[serde(default = "PrepareRenameQuery::default_max_bytes")]
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::PageBudget>,
}
impl PrepareRenameQuery {
    pub fn default_max_bytes() -> usize {
        16_384
    }
    pub fn execute(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let budget = PlanReviewReply::budget(self.max_bytes, self.page.as_ref())?;
        let revision = engine.query_revision();
        let (mut graph, snapshot) = QuerySnapshot::capture(engine)?;
        if let Some(oracle) = engine.oracle() {
            graph = graph.with_oracle(oracle);
        }
        let planned = self.intent.plan_in(&mut graph, engine.workspace())?;
        RetainedPlan::publish(
            engine,
            planned.into_mutation(),
            snapshot,
            revision,
            budget,
            self.page.is_some(),
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PrepareRewriteQuery {
    pub intent: RewriteIntent,
    #[serde(default = "PrepareRenameQuery::default_max_bytes")]
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::PageBudget>,
}
impl PrepareRewriteQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let budget = PlanReviewReply::budget(self.max_bytes, self.page.as_ref())?;
        let revision = engine.query_revision();
        let (mut graph, snapshot) = QuerySnapshot::capture(engine)?;
        let planned = self.intent.plan_in(&mut graph, engine.workspace())?;
        RetainedPlan::publish(
            engine,
            planned.into_mutation(),
            snapshot,
            revision,
            budget,
            self.page.is_some(),
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InspectPlanQuery {
    pub plan_id: PlanId,
    #[serde(default = "PrepareRenameQuery::default_max_bytes")]
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::PageBudget>,
}
impl InspectPlanQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReviewReply, EngineError> {
        let budget = PlanReviewReply::budget(self.max_bytes, self.page.as_ref())?;
        let review = engine.plans().review(&self.plan_id, Instant::now())?;
        let result = if self.page.is_some() && matches!(review.status, PlanStatus::Prepared { .. })
        {
            let captured = engine.plans().captured(&self.plan_id, Instant::now())?;
            PlanReviewReply::Page(captured.page(engine, &self.plan_id, 0, 0, budget.clone())?)
        } else {
            PlanReviewReply::Complete(review)
        };
        budget.check(&result)?;
        Ok(result)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReviewPlanQuery {
    pub cursor: PlanReviewCursor,
    #[serde(default)]
    pub page: crate::PageBudget,
}
impl ReviewPlanQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReviewPage, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReviewPage, EngineError> {
        self.page.validate()?;
        let (id, record, offset) = self.cursor.position()?;
        let captured = engine.plans().captured(&id, Instant::now())?;
        captured.page(engine, &id, record, offset, self.page)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ApplyPlanQuery {
    pub plan_id: PlanId,
}
impl ApplyPlanQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReceipt, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReceipt, EngineError> {
        let review = engine.plans().review(&self.plan_id, Instant::now())?;
        match review.status {
            PlanStatus::Applied { receipt } => return Ok(receipt),
            PlanStatus::Prepared { .. } => {}
            _ => return Err(EngineError::PlanConsumed),
        }
        // Move the executable plan out under operation exclusion. Store locks never
        // cover filesystem work. A failed attempt cannot run the same edits again.
        let pending = engine.plans().take(&self.plan_id)?;
        let result = pending.apply(engine, self.plan_id.clone());
        engine.plans().complete(&self.plan_id, &result);
        result
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DiscardPlanQuery {
    pub plan_id: PlanId,
}
impl DiscardPlanQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        engine.publish_read(|| engine.plans().discard(&self.plan_id, Instant::now()))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlanReview {
    /// Latest explicitly requested validation; does not change the apply receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<crate::ValidationReport>,
    pub plan_id: PlanId,
    /// Fixed lifetime from preparation; inspection/retry does not extend it.
    pub lifetime_seconds: u64,
    #[serde(flatten)]
    pub status: PlanStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum PlanStatus {
    Prepared { preview: PlanPreview },
    Applied { receipt: PlanReceipt },
    Failed { failure: Failure },
    Discarded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlanReceipt {
    pub plan_id: PlanId,
    /// Committed history entry, used by the existing history/undo lifecycle.
    pub history_id: u64,
    /// Contents written by the transaction. A replay reports this historical result.
    pub files: Vec<SourceVersion>,
}

pub(crate) struct RetainedPlan {
    pub review: PlanReview,
    pub pending: Option<PendingPlan>,
    pub captured: Option<Arc<CapturedReview>>,
    pub baseline: Option<QuerySnapshot>,
    pub bytes: usize,
}
pub(crate) struct PendingPlan {
    planned: Planned<MutationAnswer>,
    snapshot: QuerySnapshot,
    revision: u64,
}
impl RetainedPlan {
    fn publish(
        engine: &Engine,
        planned: Planned<MutationAnswer>,
        snapshot: QuerySnapshot,
        revision: u64,
        budget: crate::PageBudget,
        paged: bool,
    ) -> Result<PlanReviewReply, EngineError> {
        snapshot.validate(engine)?;
        if revision != engine.query_revision() {
            return Err(EngineError::StalePlan);
        }
        let preview = PlanPreview::from_mutation(&planned)?;
        let review = PlanReview {
            plan_id: engine.plans().allocate(),
            validation: None,
            lifetime_seconds: crate::PlanLimits::default().lifetime_seconds,
            status: PlanStatus::Prepared { preview },
        };
        let retained = Self::new(review.clone(), planned, snapshot, revision);
        let reply = if paged {
            PlanReviewReply::Page(retained.captured.as_ref().expect("prepared review").page(
                engine,
                &review.plan_id,
                0,
                0,
                budget.clone(),
            )?)
        } else {
            PlanReviewReply::Complete(review)
        };
        budget.check(&reply)?;
        retained
            .baseline
            .as_ref()
            .expect("prepared baseline")
            .validate(engine)?;
        if revision != engine.query_revision() {
            return Err(EngineError::StalePlan);
        }
        engine.publish_read(|| engine.plans().insert(retained, Instant::now()))?;
        Ok(reply)
    }
    pub(crate) fn new(
        review: PlanReview,
        planned: Planned<MutationAnswer>,
        snapshot: QuerySnapshot,
        revision: u64,
    ) -> Self {
        // Charge source copies, replacement strings, preview metadata, snapshot maps,
        // and allocation overhead conservatively. No graph is retained.
        let source_bytes = planned
            .preview()
            .iter()
            .map(|file| file.before.len().saturating_add(file.after.len()))
            .fold(0usize, usize::saturating_add);
        let bytes = source_bytes
            .saturating_mul(4)
            .saturating_add(crate::query_store::QueryStore::weight(&review))
            .saturating_add(crate::query_store::QueryStore::weight(&snapshot).saturating_mul(2));
        let captured = match &review.status {
            PlanStatus::Prepared { preview } => {
                Some(Arc::new(CapturedReview::new(preview.clone())))
            }
            _ => None,
        };
        let bytes = bytes.saturating_add(captured.as_ref().map_or(0, |c| c.weight()));
        Self {
            review,
            captured,
            baseline: Some(snapshot.clone()),
            pending: Some(PendingPlan {
                planned,
                snapshot,
                revision,
            }),
            bytes,
        }
    }
}
impl PendingPlan {
    fn apply(self, engine: &Engine, plan_id: PlanId) -> Result<PlanReceipt, EngineError> {
        if self.revision != engine.query_revision() {
            return Err(EngineError::StalePlan);
        }
        self.snapshot
            .validate(engine)
            .map_err(|error| match error {
                EngineError::StaleQuery | EngineError::StaleSource { .. } => EngineError::StalePlan,
                other => other,
            })?;
        let files = self
            .planned
            .preview()
            .iter()
            .map(|file| SourceVersion {
                path: file.moved_to.clone().unwrap_or_else(|| file.path.clone()),
                content: ContentId::of(&file.after),
            })
            .collect();
        let mut receipt = PlanReceipt {
            plan_id,
            history_id: u64::MAX,
            files,
        };
        // Reserve enough wire space before writes, including the longest history id.
        crate::PageBudget::MAXIMUM.check(&receipt)?;
        let applied = crate::Apply(self.planned).apply_in(engine)?;
        receipt.history_id = applied.history_id();
        Ok(receipt)
    }
}
impl crate::report::Document {
    pub(crate) fn plan_review(review: &PlanReview) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = match &review.status {
            PlanStatus::Prepared { preview } => match preview {
                PlanPreview::Rename(r) => Self::rename(r),
                PlanPreview::Rewrite(r) => Self::rewrite(r),
            },
            PlanStatus::Applied { receipt } => Self::plan_receipt(receipt),
            PlanStatus::Failed { failure } => Self::error(failure),
            PlanStatus::Discarded => Self::new(),
        };
        if let Some(validation) = &review.validation {
            let result = Self::validation(validation);
            let (body, notes) = result.parts();
            for block in body {
                doc.block_body(block.clone());
            }
            for block in notes {
                doc.block_note(block.clone());
            }
        }
        doc.notes([Line::of(Role::Plain, "Plan: ").and(Role::Plain, &review.plan_id.0)]);
        doc
    }
    pub(crate) fn plan_receipt(receipt: &PlanReceipt) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        doc.body([Line::of(Role::Strong, "Applied reviewed plan")
            .and(Role::Plain, format!(" (history {})", receipt.history_id))]);
        for file in &receipt.files {
            doc.body([Line::of(Role::Path, file.path.to_string())]);
        }
        doc
    }
}
