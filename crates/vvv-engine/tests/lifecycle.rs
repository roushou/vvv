//! Source refresh is demand-driven; operation exclusion spans writes and recovery.
mod common;

use std::path::Path;
use std::time::Duration;

use common::{FaultAction, FaultFixture, FaultOperation};
use vvv_engine::{Apply, Ledger, Query, RenameIntent, Request, Retention, SearchQuery};

#[test]
fn history_undo_and_retained_apply_do_not_refresh_the_source_tree() {
    let fixture = FaultFixture::new(&[("a.p", "def foo\nfoo"), ("unrelated.p", "def other")]);
    let plan = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(FaultOperation::Walk, "", 0, FaultAction::Always);
    fixture.vfs.clear_trace();
    let applied = Apply(plan).apply(&fixture.engine).unwrap();
    let ledger = Ledger::new(&fixture.engine);
    assert_eq!(
        ledger.history().unwrap().entries[0].id,
        applied.history_id()
    );
    fixture.engine.run(Request::History).unwrap();
    fixture.engine.run(Request::Undo).unwrap();
    assert!(ledger.history().unwrap().entries.is_empty());
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(
        !fixture
            .vfs
            .trace()
            .iter()
            .any(|(op, _)| *op == FaultOperation::Walk)
    );
    assert!(
        SearchQuery::from(Query::pattern("foo"))
            .execute(&fixture.engine)
            .is_err()
    );
}

#[test]
fn trusted_sessions_re_read_only_changed_files_after_apply() {
    let fixture = FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "def other")]);
    let engine = fixture
        .engine
        .clone()
        .with_retention(Retention::session().trusting(Duration::from_secs(3600)));
    let plan = RenameIntent::new("foo", "bar").plan(&engine).unwrap();
    Apply(plan).apply(&engine).unwrap();
    fixture.vfs.clear_trace();
    let search = SearchQuery::from(Query::pattern("bar"))
        .execute(&engine)
        .unwrap();
    assert_eq!(search.matches.len(), 2);
    let trace = fixture.vfs.trace();
    let source_reads: Vec<_> = trace
        .iter()
        .filter(|(op, path)| {
            *op == FaultOperation::Read && path.extension().is_some_and(|ext| ext == "p")
        })
        .map(|(_, path)| path.as_path())
        .collect();
    assert_eq!(source_reads, [Path::new("/ws/a.p")]);
    assert!(trace.iter().any(|(op, _)| *op == FaultOperation::Walk));
    fixture.vfs.clear_trace();
    SearchQuery::from(Query::pattern("bar"))
        .execute(&engine)
        .unwrap();
    assert!(
        fixture.vfs.trace().is_empty(),
        "the refreshed session stays trusted"
    );
}

#[test]
fn failed_apply_defers_invalidation_until_the_next_graph_query() {
    let fixture = FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let engine = fixture
        .engine
        .clone()
        .with_retention(Retention::session().trusting(Duration::from_secs(3600)));
    let plan = RenameIntent::new("foo", "bar").plan(&engine).unwrap();
    fixture.arm(
        FaultOperation::Write,
        "a.p",
        0,
        FaultAction::Partial("broken".into()),
    );
    fixture.vfs.clear_trace();
    assert!(Apply(plan).apply(&engine).is_err());
    assert!(
        !fixture
            .vfs
            .trace()
            .iter()
            .any(|(op, _)| *op == FaultOperation::Walk)
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    fixture.vfs.clear_trace();
    let search = SearchQuery::from(Query::pattern("foo"))
        .execute(&engine)
        .unwrap();
    assert_eq!(search.matches.len(), 2);
    assert!(
        fixture
            .vfs
            .trace()
            .iter()
            .any(|(op, _)| *op == FaultOperation::Walk)
    );
}

#[test]
fn one_applied_request_refreshes_the_graph_once() {
    let fixture = FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    fixture
        .engine
        .run(Request::Rename {
            intent: RenameIntent::new("foo", "bar"),
            apply: true,
        })
        .unwrap();
    assert_eq!(
        fixture
            .vfs
            .trace()
            .iter()
            .filter(|(op, _)| *op == FaultOperation::Walk)
            .count(),
        1
    );
}

#[test]
fn external_invalidation_is_shared_and_waits_for_a_graph_request() {
    let fixture = FaultFixture::new(&[("a.p", "def foo")]);
    let engine = fixture
        .engine
        .clone()
        .with_retention(Retention::session().trusting(Duration::from_secs(3600)));
    SearchQuery::from(Query::pattern("foo"))
        .execute(&engine)
        .unwrap();
    fixture
        .vfs
        .base
        .write(Path::new("/ws/a.p"), "def bar")
        .unwrap();
    fixture.vfs.clear_trace();
    engine.clone().touched();
    Ledger::new(&engine).history().unwrap();
    assert!(
        !fixture
            .vfs
            .trace()
            .iter()
            .any(|(op, _)| *op == FaultOperation::Walk)
    );
    let search = SearchQuery::from(Query::pattern("bar"))
        .execute(&engine)
        .unwrap();
    assert_eq!(search.matches.len(), 1);
}
