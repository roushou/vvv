//! Session-owned, reviewable rename plans. Executable edits never cross the wire.
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{
    ContentId, Engine, EngineError, Failure, Planned, Rename, RenameIntent, SourceVersion,
};
use serde::{Deserialize, Serialize};
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
}
impl PrepareRenameQuery {
    pub fn default_max_bytes() -> usize {
        16_384
    }
    pub fn execute(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        let budget = crate::PageBudget {
            max_bytes: self.max_bytes,
            max_items: 1,
        };
        budget.validate()?;
        let revision = engine.query_revision();
        let (mut graph, snapshot) = QuerySnapshot::capture(engine)?;
        if let Some(oracle) = engine.oracle() {
            graph = graph.with_oracle(oracle);
        }
        let planned = self.intent.plan_in(&mut graph, engine.workspace())?;
        snapshot.validate(engine)?;
        if revision != engine.query_revision() {
            return Err(EngineError::StalePlan);
        }
        let id = engine.plans().allocate();
        let review = PlanReview {
            plan_id: id,
            validation: None,
            lifetime_seconds: crate::PlanLimits::default().lifetime_seconds,
            status: PlanStatus::Prepared {
                preview: (*planned).clone(),
            },
        };
        // Never publish a handle whose complete review could not be delivered.
        budget.check(&review)?;
        let retained = RetainedPlan::new(review.clone(), planned, snapshot, revision);
        engine.publish_read(|| engine.plans().insert(retained, Instant::now()))?;
        Ok(review)
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
}
impl InspectPlanQuery {
    pub fn execute(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PlanReview, EngineError> {
        let budget = crate::PageBudget {
            max_bytes: self.max_bytes,
            max_items: 1,
        };
        budget.validate()?;
        let review = engine.plans().review(&self.plan_id, Instant::now())?;
        budget.check(&review)?;
        Ok(review)
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
    Prepared { preview: Rename },
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
    pub baseline: Option<QuerySnapshot>,
    pub bytes: usize,
}
pub(crate) struct PendingPlan {
    planned: Planned<Rename>,
    snapshot: QuerySnapshot,
    revision: u64,
}
impl RetainedPlan {
    pub(crate) fn new(
        review: PlanReview,
        planned: Planned<Rename>,
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
        Self {
            review,
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
            PlanStatus::Prepared { preview } => Self::rename(preview),
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
