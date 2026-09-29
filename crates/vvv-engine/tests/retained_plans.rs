mod common;
use common::{Fake, FaultAction, FaultFixture, FaultOperation};
use std::{path::Path, sync::Arc};
use vvv_engine::{
    ApplyPlanQuery, Call, ContentId, DiscardPlanQuery, Engine, EngineError, InspectPlanQuery,
    Languages, Ledger, MemoryVfs, PlanId, PlanStatus, PrepareRenameQuery, RenameIntent, Vfs,
    Workspace,
};
struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new() -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", "def foo\nfoo()")
                .with_file("/ws/b.p", "call foo"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        Self { engine, vfs }
    }
    fn prepare(&self) -> vvv_engine::PlanReview {
        PrepareRenameQuery {
            intent: RenameIntent::new("foo", "bar"),
            max_bytes: 16384,
            page: None,
        }
        .execute(&self.engine)
        .unwrap()
        .into_complete()
        .unwrap()
    }
    fn inspect(&self, plan_id: PlanId) -> vvv_engine::PlanReview {
        InspectPlanQuery {
            plan_id,
            max_bytes: 16384,
            page: None,
        }
        .execute(&self.engine)
        .unwrap()
        .into_complete()
        .unwrap()
    }
}
#[test]
fn exact_preview_survives_wire_round_trip_and_apply_is_retryable_even_after_undo() {
    let f = Fixture::new();
    let expected = RenameIntent::new("foo", "bar").plan(&f.engine).unwrap();
    let review = f.prepare();
    let PlanStatus::Prepared { preview } = &review.status else {
        panic!()
    };
    assert_eq!(
        serde_json::to_value(preview.files()).unwrap(),
        serde_json::to_value(&expected.files).unwrap()
    );
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), "def foo\nfoo()");
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
    assert_eq!(
        serde_json::to_value(&review).unwrap(),
        serde_json::to_value(f.inspect(review.plan_id.clone())).unwrap()
    );
    let plan_id = serde_json::from_value(serde_json::to_value(&review.plan_id).unwrap()).unwrap();
    let apply = ApplyPlanQuery { plan_id };
    let receipt = apply.clone().execute(&f.engine.clone()).unwrap();
    for file in expected.preview() {
        assert_eq!(
            f.vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
            file.after
        );
        assert!(
            receipt.files.iter().any(|version| version.path == file.path
                && version.content == ContentId::of(&file.after))
        );
    }
    assert_eq!(receipt, apply.clone().execute(&f.engine).unwrap());
    assert_eq!(Ledger::new(&f.engine).history().unwrap().entries.len(), 1);
    assert!(matches!(
        f.inspect(review.plan_id).status,
        PlanStatus::Applied { .. }
    ));
    let undo = Ledger::new(&f.engine).undo().unwrap();
    assert_eq!(undo.undone.id, receipt.history_id);
    assert_eq!(receipt, apply.execute(&f.engine).unwrap());
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), "def foo\nfoo()");
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
}
#[test]
fn changed_inputs_and_new_occurrences_require_a_new_review() {
    for (path, text) in [
        ("a.p", "def foo\nuser edit"),
        ("new.p", "foo"),
        ("package", "changed-package"),
        ("b.p", "call foo\nfoo"),
    ] {
        let f = Fixture::new();
        let review = f.prepare();
        f.vfs.write(&Path::new("/ws").join(path), text).unwrap();
        let apply = ApplyPlanQuery {
            plan_id: review.plan_id.clone(),
        };
        assert!(matches!(
            apply.clone().execute(&f.engine),
            Err(EngineError::StalePlan)
        ));
        assert!(
            matches!(f.inspect(review.plan_id).status, PlanStatus::Failed { failure } if failure.code == vvv_engine::ErrorCode::Stale)
        );
        assert!(matches!(
            apply.execute(&f.engine),
            Err(EngineError::PlanConsumed)
        ));
        assert_eq!(f.vfs.read(&Path::new("/ws").join(path)).unwrap(), text);
        assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
    }
}
#[test]
fn foreign_malformed_discarded_and_concurrent_handles_are_safe() {
    let f = Fixture::new();
    let review = f.prepare();
    let apply = ApplyPlanQuery {
        plan_id: review.plan_id.clone(),
    };
    assert!(matches!(
        apply.clone().execute(&Fixture::new().engine),
        Err(EngineError::PlanExpired)
    ));
    let malformed = serde_json::from_value(serde_json::json!("bad")).unwrap();
    assert!(matches!(
        ApplyPlanQuery { plan_id: malformed }.execute(&f.engine),
        Err(EngineError::InvalidPlan)
    ));
    let discard = DiscardPlanQuery {
        plan_id: review.plan_id.clone(),
    };
    assert!(matches!(
        discard.clone().execute(&f.engine).unwrap().status,
        PlanStatus::Discarded
    ));
    assert!(matches!(
        discard.execute(&f.engine).unwrap().status,
        PlanStatus::Discarded
    ));
    assert!(matches!(
        apply.execute(&f.engine),
        Err(EngineError::PlanConsumed)
    ));
    let review = f.prepare();
    let workers = (0..4)
        .map(|_| {
            let engine = f.engine.clone();
            let plan_id = review.plan_id.clone();
            std::thread::spawn(move || ApplyPlanQuery { plan_id }.execute(&engine).unwrap())
        })
        .collect::<Vec<_>>();
    let receipts = workers
        .into_iter()
        .map(|w| w.join().unwrap())
        .collect::<Vec<_>>();
    assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
    assert_eq!(Ledger::new(&f.engine).history().unwrap().entries.len(), 1);
}
#[test]
fn failed_apply_rolls_back_records_failure_and_cannot_be_retried_as_a_write() {
    let f = FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "foo")]);
    let review = PrepareRenameQuery {
        intent: RenameIntent::new("foo", "bar"),
        max_bytes: 16384,
        page: None,
    }
    .execute(&f.engine)
    .unwrap()
    .into_complete()
    .unwrap();
    f.arm(FaultOperation::Write, "b.p", 0, FaultAction::After);
    let apply = ApplyPlanQuery {
        plan_id: review.plan_id.clone(),
    };
    assert!(apply.clone().execute(&f.engine).is_err());
    assert_eq!(f.read("a.p"), "def foo\nfoo");
    assert_eq!(f.read("b.p"), "foo");
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
    assert!(matches!(
        InspectPlanQuery {
            plan_id: review.plan_id,
            max_bytes: 16384,
            page: None,
        }
        .execute(&f.engine)
        .unwrap()
        .into_complete()
        .unwrap()
        .status,
        PlanStatus::Failed { .. }
    ));
    f.vfs.clear_trace();
    assert!(matches!(
        apply.execute(&f.engine),
        Err(EngineError::PlanConsumed)
    ));
    assert!(
        !f.vfs
            .trace()
            .iter()
            .any(|(operation, _)| *operation == FaultOperation::Write)
    );
}
#[test]
fn output_and_cancellation_failures_publish_no_plans_and_mutation_budgets_write_nothing() {
    let f = Fixture::new();
    let request = serde_json::json!({"command":"prepare_rename","intent":{"name":"foo","to":"bar"},"max_output_bytes":1024});
    for _ in 0..20 {
        let call: Call = serde_json::from_value(request.clone()).unwrap();
        let reply = serde_json::to_value(call.execute(&f.engine)).unwrap();
        assert_eq!(reply["code"], "output_limit");
    }
    let cancelled = vvv_engine::ReadCancellation::default();
    cancelled.cancel();
    let mut request = request;
    request["max_output_bytes"] = 16384.into();
    let call: Call = serde_json::from_value(request).unwrap();
    assert_eq!(
        serde_json::to_value(call.execute_with_cancellation(&f.engine, &cancelled)).unwrap()["code"],
        "cancelled"
    );
    let review = f.prepare();
    let call: Call = serde_json::from_value(serde_json::json!({"command":"apply_plan","plan_id":review.plan_id,"max_output_bytes":1024})).unwrap();
    assert_eq!(
        serde_json::to_value(call.execute(&f.engine)).unwrap()["code"],
        "bad_request"
    );
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), "def foo\nfoo()");
    assert!(matches!(
        f.inspect(review.plan_id).status,
        PlanStatus::Prepared { .. }
    ));
}
#[test]
fn capacity_never_silently_evicts_a_reviewed_plan_and_discard_frees_space() {
    let f = Fixture::new();
    let plans = (0..vvv_engine::PlanLimits::default().max_plans)
        .map(|_| f.prepare())
        .collect::<Vec<_>>();
    assert!(matches!(
        PrepareRenameQuery {
            intent: RenameIntent::new("foo", "bar"),
            max_bytes: 16384,
            page: None,
        }
        .execute(&f.engine),
        Err(EngineError::PlanRetentionLimit)
    ));
    assert!(matches!(
        f.inspect(plans[0].plan_id.clone()).status,
        PlanStatus::Prepared { .. }
    ));
    DiscardPlanQuery {
        plan_id: plans[0].plan_id.clone(),
    }
    .execute(&f.engine)
    .unwrap();
    f.prepare();
    assert!(matches!(
        f.inspect(plans[1].plan_id.clone()).status,
        PlanStatus::Prepared { .. }
    ));
}
