//! Bounded session-local executable plans and terminal outcomes. Lock after operation exclusion.
use crate::capabilities::plans::review::CapturedReview;
use crate::capabilities::plans::{PendingPlan, RetainedPlan};
use crate::{EngineError, Failure, PlanId, PlanReceipt, PlanReview, PlanStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    collections::BTreeMap,
    hash::BuildHasher,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlanLimits {
    pub max_plans: usize,
    pub max_plan_bytes: usize,
    pub max_total_bytes: usize,
    pub lifetime_seconds: u64,
}
impl Default for PlanLimits {
    fn default() -> Self {
        Self {
            max_plans: 16,
            max_plan_bytes: 16 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            lifetime_seconds: 600,
        }
    }
}
struct Entry {
    plan: RetainedPlan,
    created: Instant,
}
pub(crate) struct PlanStore {
    nonce: String,
    next: u64,
    entries: BTreeMap<PlanId, Entry>,
    limits: PlanLimits,
}
impl Default for PlanStore {
    fn default() -> Self {
        Self {
            nonce: format!(
                "{:016x}",
                std::collections::hash_map::RandomState::new().hash_one("vvv plan store")
            ),
            next: 0,
            entries: BTreeMap::new(),
            limits: PlanLimits::default(),
        }
    }
}
impl PlanStore {
    pub(crate) fn allocate(&mut self) -> PlanId {
        self.next += 1;
        PlanId(format!("p1.{}.{:016x}", self.nonce, self.next))
    }
    fn purge(&mut self, now: Instant) {
        let ttl = Duration::from_secs(self.limits.lifetime_seconds);
        self.entries
            .retain(|_, entry| now.duration_since(entry.created) < ttl);
    }
    pub(crate) fn insert(&mut self, plan: RetainedPlan, now: Instant) -> Result<(), EngineError> {
        self.purge(now);
        // Reclaim discarded tombstones on preparation. Never evict
        // prepared plans or successful receipts to make a new plan fit.
        self.entries
            .retain(|_, entry| !matches!(entry.plan.review.status, PlanStatus::Discarded));
        let total = self
            .entries
            .values()
            .map(|e| e.plan.bytes)
            .fold(0usize, usize::saturating_add);
        if plan.bytes > self.limits.max_plan_bytes
            || total.saturating_add(plan.bytes) > self.limits.max_total_bytes
            || self.entries.len() >= self.limits.max_plans
        {
            return Err(EngineError::PlanRetentionLimit);
        }
        self.entries
            .insert(plan.review.plan_id.clone(), Entry { plan, created: now });
        Ok(())
    }
    pub(crate) fn review(&mut self, id: &PlanId, now: Instant) -> Result<PlanReview, EngineError> {
        Ok(self.entry(id, now)?.plan.review.clone())
    }
    fn entry(&mut self, id: &PlanId, now: Instant) -> Result<&Entry, EngineError> {
        let parts: Vec<_> = id.0.split('.').collect();
        if parts.len() != 3
            || parts[0] != "p1"
            || parts[1].len() != 16
            || parts[2].len() != 16
            || !parts[1..]
                .iter()
                .all(|part| part.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(EngineError::InvalidPlan);
        }
        self.purge(now);
        self.entries.get(id).ok_or(EngineError::PlanExpired)
    }
    pub(crate) fn captured(
        &mut self,
        id: &PlanId,
        now: Instant,
    ) -> Result<Arc<CapturedReview>, EngineError> {
        self.entry(id, now)?
            .plan
            .captured
            .clone()
            .ok_or(EngineError::PlanConsumed)
    }
    pub(crate) fn take(&mut self, id: &PlanId) -> Result<PendingPlan, EngineError> {
        self.entries
            .get_mut(id)
            .and_then(|entry| entry.plan.pending.take())
            .ok_or(EngineError::PlanConsumed)
    }
    pub(crate) fn complete(&mut self, id: &PlanId, result: &Result<PlanReceipt, EngineError>) {
        let entry = self
            .entries
            .get_mut(id)
            .expect("operation exclusion protects active plan");
        if result.is_err() {
            entry.plan.baseline = None;
        }
        entry.plan.review.status = match result {
            Ok(receipt) => PlanStatus::Applied {
                receipt: receipt.clone(),
            },
            Err(error) => PlanStatus::Failed {
                failure: Failure::from(error),
            },
        };
        entry.plan.bytes = crate::query_store::QueryStore::weight(&entry.plan.review)
            .saturating_add(crate::query_store::QueryStore::weight(&entry.plan.baseline))
            .saturating_add(entry.plan.captured.as_ref().map_or(0, |r| r.weight()));
    }
    pub(crate) fn validation(
        &mut self,
        id: &PlanId,
        now: Instant,
    ) -> Result<
        (
            PlanReceipt,
            Arc<crate::capabilities::validation::baseline::ValidationBaseline>,
            u64,
        ),
        EngineError,
    > {
        let review = self.review(id, now)?;
        let PlanStatus::Applied { receipt } = review.status else {
            return Err(EngineError::InvalidValidation);
        };
        let baseline = self.entries[id]
            .plan
            .baseline
            .clone()
            .expect("applied plan retains inputs");
        Ok((
            receipt,
            baseline,
            review.validation.map_or(1, |report| report.run + 1),
        ))
    }
    pub(crate) fn reserve_validation(
        &self,
        id: &PlanId,
        max_bytes: usize,
    ) -> Result<(), EngineError> {
        let plan = &self.entries[id].plan;
        let mut review = plan.review.clone();
        review.validation = None;
        let bytes = crate::query_store::QueryStore::weight(&review)
            .saturating_add(crate::query_store::QueryStore::weight(&plan.baseline))
            .saturating_add(plan.captured.as_ref().map_or(0, |r| r.weight()))
            .saturating_add(max_bytes.saturating_mul(4))
            .saturating_add(4096);
        let total = self
            .entries
            .iter()
            .filter(|(key, _)| *key != id)
            .map(|(_, entry)| entry.plan.bytes)
            .fold(bytes, usize::saturating_add);
        if bytes > self.limits.max_plan_bytes || total > self.limits.max_total_bytes {
            return Err(EngineError::PlanRetentionLimit);
        }
        Ok(())
    }
    pub(crate) fn record_validation(&mut self, id: &PlanId, report: crate::ValidationReport) {
        let entry = self
            .entries
            .get_mut(id)
            .expect("operation exclusion protects active validation");
        entry.plan.review.validation = Some(report);
        entry.plan.bytes = crate::query_store::QueryStore::weight(&entry.plan.review)
            .saturating_add(crate::query_store::QueryStore::weight(&entry.plan.baseline))
            .saturating_add(entry.plan.captured.as_ref().map_or(0, |r| r.weight()));
    }
    pub(crate) fn discard(&mut self, id: &PlanId, now: Instant) -> Result<PlanReview, EngineError> {
        let review = self.review(id, now)?;
        if matches!(review.status, PlanStatus::Prepared { .. }) {
            let entry = self.entries.get_mut(id).expect("review checked existence");
            entry.plan.pending = None;
            entry.plan.captured = None;
            entry.plan.baseline = None;
            entry.plan.review.status = PlanStatus::Discarded;
            entry.plan.bytes = crate::query_store::QueryStore::weight(&entry.plan.review)
                .saturating_add(crate::query_store::QueryStore::weight(&entry.plan.baseline))
                .saturating_add(entry.plan.captured.as_ref().map_or(0, |r| r.weight()));
            return Ok(entry.plan.review.clone());
        }
        // Terminal receipts/failures remain inspectable and retryable until expiry.
        Ok(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Engine, Intent, Languages, MemoryVfs, MutationState, Planned, Rename, RenameIntent,
        Workspace,
    };
    use std::sync::Arc;
    struct Fixture {
        engine: Engine,
        store: PlanStore,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                engine: Engine::new(
                    Workspace::new("/ws", Arc::new(MemoryVfs::new())),
                    Languages::new(),
                ),
                store: PlanStore::default(),
            }
        }
        fn plan(&mut self) -> RetainedPlan {
            let rename = Rename {
                intent: RenameIntent::new("a", "b"),
                state: MutationState::Preview,
                declarations: vec![],
                occurrences: vec![],
                files: vec![],
            };
            let review = PlanReview {
                plan_id: self.store.allocate(),
                validation: None,
                lifetime_seconds: self.store.limits.lifetime_seconds,
                status: PlanStatus::Prepared {
                    preview: crate::PlanPreview::Rename(Arc::new(rename.clone())),
                },
            };
            let planned = Planned::new(
                Intent::Rename(rename.intent.clone()),
                rename,
                vec![],
                vec![],
            );
            let (_, snapshot) =
                crate::graph::query_snapshot::QuerySnapshot::capture(&self.engine).unwrap();
            let baseline = crate::capabilities::validation::baseline::ValidationBaseline::capture(
                &self.engine,
                snapshot,
                planned.preview(),
            )
            .unwrap();
            RetainedPlan::new(review, planned.into_mutation(), baseline, 0)
        }
    }
    #[test]
    fn expiry_is_monotonic_fixed_and_includes_completed_receipts() {
        let mut f = Fixture::new();
        let now = Instant::now();
        let plan = f.plan();
        let id = plan.review.plan_id.clone();
        f.store.insert(plan, now).unwrap();
        let end = now + Duration::from_secs(f.store.limits.lifetime_seconds);
        f.store.review(&id, end - Duration::from_secs(1)).unwrap();
        f.store.complete(
            &id,
            &Ok(PlanReceipt {
                plan_id: id.clone(),
                history_id: 1,
                files: vec![],
            }),
        );
        assert!(matches!(
            f.store.review(&id, end),
            Err(EngineError::PlanExpired)
        ));
        assert!(matches!(
            f.store.captured(&id, end),
            Err(EngineError::PlanExpired)
        ));
        assert!(f.store.entries.is_empty());
    }
    #[test]
    fn individual_and_total_memory_limits_reject_without_evicting_other_plans() {
        let mut f = Fixture::new();
        let now = Instant::now();
        let plan = f.plan();
        let weight = plan.bytes;
        f.store.limits.max_plan_bytes = weight - 1;
        assert!(matches!(
            f.store.insert(plan, now),
            Err(EngineError::PlanRetentionLimit)
        ));
        assert!(f.store.entries.is_empty());
        f.store.limits.max_plan_bytes = weight * 2;
        f.store.limits.max_total_bytes = weight;
        let plan = f.plan();
        let id = plan.review.plan_id.clone();
        f.store.insert(plan, now).unwrap();
        let second = f.plan();
        assert!(matches!(
            f.store.insert(second, now),
            Err(EngineError::PlanRetentionLimit)
        ));
        f.store.review(&id, now).unwrap();
        assert_eq!(f.store.entries.len(), 1);
    }
    #[test]
    fn completed_reviews_remain_charged_and_validation_cannot_overrun_retention() {
        let mut f = Fixture::new();
        let now = Instant::now();
        let plan = f.plan();
        let id = plan.review.plan_id.clone();
        let review_bytes = plan.captured.as_ref().unwrap().weight();
        f.store.insert(plan, now).unwrap();
        f.store.complete(
            &id,
            &Ok(PlanReceipt {
                plan_id: id.clone(),
                history_id: 1,
                files: vec![],
            }),
        );
        assert!(f.store.entries[&id].plan.bytes >= review_bytes);
        f.store.captured(&id, now).unwrap();
        f.store.limits.max_total_bytes = f.store.entries[&id].plan.bytes;
        assert!(matches!(
            f.store.reserve_validation(&id, 4096),
            Err(EngineError::PlanRetentionLimit)
        ));
        f.store.captured(&id, now).unwrap();
    }
}
