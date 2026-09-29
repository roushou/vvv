mod common;
#[cfg(any(unix, windows))]
use common::Fake;
use std::sync::Arc;
#[cfg(any(unix, windows))]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(any(unix, windows))]
use vvv_engine::{ApplyPlanQuery, InspectPlanQuery, PlanReceipt, PrepareRenameQuery, RenameIntent};
use vvv_engine::{
    CheckCommand, Engine, EngineError, Languages, MemoryVfs, PlanId, ValidatePlanQuery,
    ValidationBudget, Workspace,
};

#[cfg(any(unix, windows))]
struct Fixture {
    root: PathBuf,
    engine: Engine,
    receipt: PlanReceipt,
}
#[cfg(any(unix, windows))]
impl Fixture {
    fn new(mode: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "vvv-validation 世界-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package"), "ws").unwrap();
        std::fs::write(root.join("a.p"), "def foo\nfoo()").unwrap();
        std::fs::write(root.join("mode"), mode).unwrap();
        std::fs::write(root.join("resource.bin"), [0, 255, 128]).unwrap();
        std::fs::write(root.join(".hidden"), "config").unwrap();
        let engine = Engine::new(
            Workspace::disk(&root).unwrap(),
            Languages::new().with(Fake::default()),
        );
        let review = PrepareRenameQuery {
            intent: RenameIntent::new("foo", "bar"),
            max_bytes: 16384,
            page: None,
        }
        .execute(&engine)
        .unwrap()
        .into_complete()
        .unwrap();
        let receipt = ApplyPlanQuery {
            plan_id: review.plan_id,
        }
        .execute(&engine)
        .unwrap();
        Self {
            root,
            engine,
            receipt,
        }
    }
    fn query(&self) -> ValidatePlanQuery {
        ValidatePlanQuery {
            plan_id: self.receipt.plan_id.clone(),
            checks: vec![CheckCommand {
                name: "fixture check".into(),
                program: std::env::current_exe().unwrap().to_str().unwrap().into(),
                args: vec![
                    "--exact".into(),
                    "validation_command_fixture".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                ],
            }],
            extra_inputs: vec![".hidden".into()],
            budget: ValidationBudget {
                timeout_ms: 2000,
                max_bytes: 4096,
            },
        }
    }
    fn recorded(&self) -> Option<vvv_engine::ValidationReport> {
        InspectPlanQuery {
            plan_id: self.receipt.plan_id.clone(),
            max_bytes: 16384,
            page: None,
        }
        .execute(&self.engine)
        .unwrap()
        .into_complete()
        .unwrap()
        .validation
    }
}
#[cfg(any(unix, windows))]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
#[ignore = "subprocess fixture; invoked explicitly by validation tests"]
fn validation_command_fixture() {
    use std::io::Write;
    if std::env::var_os("VVV_VALIDATION_DESCENDANT").is_some() {
        std::fs::create_dir_all(".vvv").unwrap();
        std::fs::write(".vvv/child-ready", "ready").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::fs::write(".vvv/escaped", "escaped process group").unwrap();
        return;
    }
    let mode = std::fs::read_to_string("mode").unwrap();
    match mode.as_str() {
        "pass" => {
            println!("checked source");
            eprintln!("diagnostic");
        }
        "args" => {
            assert!(std::env::args().any(|argument| argument == "--skip=héllo \"世界\"\\"));
            println!("arguments preserved");
        }
        "fail" => std::process::exit(7),
        "descendant" => {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "validation_command_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("VVV_VALIDATION_DESCENDANT", "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while !std::path::Path::new(".vvv/child-ready").exists()
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if !std::path::Path::new(".vvv/child-ready").exists() {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not start");
            }
            // Deliberately exit with a live descendant to exercise group cleanup.
            std::process::exit(0);
        }
        "flood" => {
            for _ in 0..4096 {
                println!("héllo\\\"世界");
                eprintln!("error 世界");
            }
        }
        "sleep" => {
            std::fs::create_dir_all(".vvv").unwrap();
            std::fs::write(".vvv/check-started", "ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(10));
            std::fs::write(".vvv/escaped", "must not run").unwrap();
        }
        "change" => std::fs::write("a.p", "def changed").unwrap(),
        "hidden" => std::fs::write(".hidden", "changed").unwrap(),
        "ignore" => std::fs::write(".gitignore", "resource.bin\n").unwrap(),
        _ => panic!("unknown fixture mode"),
    }
    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();
}
#[test]
fn virtual_workspaces_never_execute_real_programs() {
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(MemoryVfs::new())),
        Languages::new(),
    );
    assert!(
        !vvv_engine::DiscoveryQuery::default()
            .execute(&engine)
            .validation_available
    );
    let query = ValidatePlanQuery {
        plan_id: serde_json::from_str::<PlanId>("\"bad\"").unwrap(),
        checks: vec![CheckCommand {
            name: "check".into(),
            program: "does-not-exist".into(),
            args: vec![],
        }],
        extra_inputs: vec![],
        budget: ValidationBudget::default(),
    };
    assert!(matches!(
        query.execute(&engine),
        Err(EngineError::ValidationUnavailable)
    ));
}
#[cfg(any(unix, windows))]
#[test]
fn results_are_versioned_retained_and_do_not_change_the_apply_receipt() {
    use vvv_engine::{CheckOutcome, ValidationSourceState};
    for mode in ["pass", "fail", "flood"] {
        let f = Fixture::new(mode);
        let query = f.query();
        let report = query.clone().execute(&f.engine).unwrap();
        assert_eq!(report.passed, mode != "fail");
        assert_eq!(report.source_state, ValidationSourceState::Unchanged);
        assert_eq!(report.before, report.after.clone().unwrap());
        assert_eq!(report.sources, f.receipt.files);
        assert_eq!(report, f.recorded().unwrap());
        assert!(serde_json::to_vec(&report).unwrap().len() <= query.budget.max_bytes);
        if mode == "pass" {
            assert!(report.checks[0].stdout.text.contains("checked source"));
            assert!(report.checks[0].stderr.text.contains("diagnostic"));
        }
        if mode == "fail" {
            assert_eq!(report.checks[0].exit_code, Some(7));
            assert_eq!(report.checks[0].outcome, CheckOutcome::Failed);
        }
        if mode == "flood" {
            assert!(report.checks[0].stdout.truncated);
            assert!(report.checks[0].stderr.truncated);
            assert!(report.checks[0].stdout.bytes_seen > 10000);
        }
        assert_eq!(query.execute(&f.engine).unwrap().run, 2);
        assert_eq!(
            ApplyPlanQuery {
                plan_id: f.receipt.plan_id.clone()
            }
            .execute(&f.engine)
            .unwrap(),
            f.receipt
        );
    }
}
#[cfg(any(unix, windows))]
#[test]
fn stale_sources_and_invalid_requests_never_launch_checks() {
    let f = Fixture::new("sleep");
    let mut query = f.query();
    query.extra_inputs = vec!["../outside".into()];
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidValidation)
    ));
    let mut query = f.query();
    query.checks.clear();
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidValidation)
    ));
    let mut query = f.query();
    query.checks[0].args = vec!["a".repeat(3500)];
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::OutputLimit { .. })
    ));
    assert!(f.recorded().is_none());
    std::fs::write(f.root.join("a.p"), "def edited").unwrap();
    assert!(matches!(
        f.query().execute(&f.engine),
        Err(EngineError::StalePlan)
    ));
    assert!(!f.root.join(".vvv/check-started").exists());
}
#[cfg(any(unix, windows))]
#[test]
fn input_changes_stop_later_checks_and_cannot_produce_a_pass() {
    for mode in ["change", "hidden", "ignore"] {
        let f = Fixture::new(mode);
        let mut query = f.query();
        query.checks.push(query.checks[0].clone());
        let report = query.execute(&f.engine).unwrap();
        assert!(!report.passed);
        assert_eq!(
            report.source_state,
            vvv_engine::ValidationSourceState::Changed
        );
        assert_eq!(report.checks[1].outcome, vvv_engine::CheckOutcome::NotRun);
    }
}
#[cfg(any(unix, windows))]
#[test]
fn spawn_failures_and_timeouts_are_structured_and_retained() {
    let f = Fixture::new("sleep");
    let mut query = f.query();
    query.checks[0].program = f.root.join("absent").to_str().unwrap().into();
    let report = query.execute(&f.engine).unwrap();
    assert_eq!(
        report.checks[0].failure.as_ref().unwrap().operation,
        vvv_engine::CheckOperation::Spawn
    );
    let mut query = f.query();
    query.budget.timeout_ms = 100;
    let start = std::time::Instant::now();
    let report = query.execute(&f.engine).unwrap();
    assert_eq!(report.checks[0].outcome, vvv_engine::CheckOutcome::TimedOut);
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
    assert_eq!(report, f.recorded().unwrap());
    assert!(!f.root.join(".vvv/escaped").exists());
}
#[cfg(any(unix, windows))]
#[test]
fn cancellation_keeps_inspectable_evidence() {
    let f = Fixture::new("sleep");
    let token = vvv_engine::ReadCancellation::default();
    let check_token = token.clone();
    let engine = f.engine.clone();
    let query = f.query();
    let worker = std::thread::spawn(move || {
        vvv_engine::Call {
            id: None,
            max_output_bytes: None,
            request: vvv_engine::Request::ValidatePlan(query),
        }
        .execute_with_cancellation(&engine, &check_token)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !f.root.join(".vvv/check-started").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(f.root.join(".vvv/check-started").exists());
    token.cancel();
    let _reply = worker.join().unwrap();
    let report = f.recorded().unwrap();
    assert_eq!(
        report.checks[0].outcome,
        vvv_engine::CheckOutcome::Cancelled
    );
    assert!(!report.passed);
}

#[cfg(any(unix, windows))]
#[test]
fn successful_leader_exit_cleans_up_descendants_holding_output_pipes() {
    let f = Fixture::new("descendant");
    let report = f.query().execute(&f.engine).unwrap();
    assert!(report.passed, "{report:?}");
    assert!(report.checks[0].stdout.complete);
    std::thread::sleep(std::time::Duration::from_millis(550));
    assert!(!f.root.join(".vvv/escaped").exists());
}

#[cfg(any(unix, windows))]
#[test]
fn rewrite_validation_uses_written_versions_and_retains_review_evidence() {
    let mut f = Fixture::new("pass");
    let review = vvv_engine::PrepareRewriteQuery {
        intent: vvv_engine::RewriteIntent::new(vvv_engine::Query::pattern("bar"), "baz"),
        max_bytes: 16384,
        page: None,
    }
    .execute(&f.engine)
    .unwrap()
    .into_complete()
    .unwrap();
    f.receipt = ApplyPlanQuery {
        plan_id: review.plan_id,
    }
    .execute(&f.engine)
    .unwrap();
    let report = f.query().execute(&f.engine).unwrap();
    assert!(report.passed);
    assert_eq!(report.sources, f.receipt.files);
    assert_eq!(
        report.sources[0].content,
        vvv_engine::ContentId::of("def baz\nbaz()")
    );
    assert_eq!(f.recorded().unwrap(), report);
    assert_eq!(
        ApplyPlanQuery {
            plan_id: f.receipt.plan_id.clone()
        }
        .execute(&f.engine)
        .unwrap(),
        f.receipt
    );
}

#[cfg(windows)]
#[test]
fn validation_executes_inside_an_existing_job() {
    use process_wrap::std::{CommandWrap, JobObject};
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "results_are_versioned_retained_and_do_not_change_the_apply_receipt",
        "--nocapture",
    ]);
    let mut wrapped = CommandWrap::from(command);
    wrapped.wrap(JobObject);
    let mut child = wrapped.spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = child.inner_mut().try_wait().unwrap() {
            child.start_kill().unwrap();
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.start_kill();
            panic!("nested-job fixture timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(any(unix, windows))]
#[test]
fn unicode_paths_and_quoted_arguments_reach_the_native_program() {
    let fixture = Fixture::new("args");
    let mut query = fixture.query();
    query.checks[0].args.push("--skip=héllo \"世界\"\\".into());
    let report = query.execute(&fixture.engine).unwrap();
    assert!(report.passed, "{report:?}");
    assert!(report.checks[0].stdout.text.contains("arguments preserved"));
}
