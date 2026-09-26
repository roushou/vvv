//! Known failures of source provenance, transactions, namespace revisions,
//! and report sites. Each ignored test states the invariant a repair must restore.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::Fake;
use vvv_core::Address;
use vvv_engine::report::{Detailed, Document, Options, View};
use vvv_engine::{
    Answer, Apply, Confidence, DepsQuery, Engine, EngineError, FileQuery, History, HistoryError,
    Intent, Languages, MemoryVfs, MoveIntent, ReferencesQuery, RenameIntent, Retention, Vfs,
    WhereQuery, Workspace,
};

struct Fixture {
    vfs: Arc<MemoryVfs>,
    engine: Engine,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let vfs = Arc::new(files.iter().fold(MemoryVfs::new(), |vfs, (path, text)| {
            vfs.with_file(Path::new("/ws").join(path), *text)
        }));
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        Self { vfs, engine }
    }

    fn retaining(mut self, retention: Retention) -> Self {
        self.engine = self.engine.with_retention(retention);
        self
    }

    fn read(&self, path: &str) -> String {
        self.vfs.read(&Path::new("/ws").join(path)).unwrap()
    }
}

#[test]
#[ignore = "known bug: plans fingerprint current text while retaining cached source coordinates"]
fn apply_refuses_a_plan_whose_source_changed() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")])
        .retaining(Retention::session().trusting(Duration::from_secs(3600)));
    fixture.engine.run(ReferencesQuery::new("foo")).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "def xyz\nxyz")
        .unwrap();

    // Planning and applying both must reject coordinates from the cached foo
    // snapshot; either boundary may detect that the source changed.
    let result = fixture
        .engine
        .run(RenameIntent::new("foo", "bar"))
        .and_then(|planned| fixture.engine.run(Apply(planned)));
    assert!(
        result.is_err(),
        "a stale source must be refused: {result:?}"
    );
    assert_eq!(fixture.read("a.p"), "def xyz\nxyz");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
#[ignore = "known bug: apply writes files before discovering corrupt history"]
fn failed_apply_preserves_files_when_history_is_unreadable() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "use a.p/foo\nfoo")]);
    fixture
        .vfs
        .write(Path::new("/ws/.vvv/history.json"), "invalid")
        .unwrap();
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();

    let result = fixture.engine.run(Apply(planned));
    assert!(matches!(
        result,
        Err(EngineError::History(HistoryError::Corrupt(_)))
    ));
    assert_eq!(
        fixture.read("a.p"),
        "def foo\nfoo",
        "a failed history commit must preserve the declaring file"
    );
    assert_eq!(fixture.read("b.p"), "use a.p/foo\nfoo");
    assert_eq!(fixture.read(".vvv/history.json"), "invalid");
}

#[test]
#[ignore = "known bug: a query substituted into Planned panics after its mutation writes"]
fn applying_a_query_result_is_rejected_before_writes() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture
        .engine
        .run(Intent::Rename(RenameIntent::new("foo", "bar")))
        .unwrap()
        .map(|_| {
            Answer::History(History {
                entries: Vec::new(),
            })
        });

    // This must be an error, not a panic or a successful write. A future
    // mutation-only result type can make the invalid substitution unrepresentable.
    let result = fixture.engine.run(Apply(planned));
    assert!(
        result.is_err(),
        "a query result cannot authorize a mutation"
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn namespace_edges_follow_project_changes_after_read_only_queries() {
    let fixture = Fixture::new(&[
        ("package", "audit"),
        ("lib.p", "use a.p/foo"),
        ("a.p", "def foo"),
    ])
    .retaining(Retention::session());
    let before = fixture
        .engine
        .run(DepsQuery {
            path: "lib.p".into(),
        })
        .unwrap();
    assert_eq!(before.module, Some(Address::new("audit", ["lib.p"])));
    assert_eq!(
        before.imports[0].address,
        Some(Address::new("audit", ["a.p", "foo"]))
    );
    for _ in 0..2 {
        fixture
            .engine
            .run(FileQuery { path: "a.p".into() })
            .unwrap();
    }
    fixture
        .vfs
        .write(Path::new("/ws/package"), "changed")
        .unwrap();

    let after = fixture
        .engine
        .run(DepsQuery {
            path: "lib.p".into(),
        })
        .unwrap();
    assert_eq!(after.module, Some(Address::new("changed", ["lib.p"])));
    assert_eq!(
        after.imports[0].address,
        Some(Address::new("changed", ["a.p", "foo"])),
        "cached edges must belong to the rebuilt namespace"
    );
}

#[test]
fn unchanged_projects_reuse_fragments_across_projectless_queries() {
    let resolves = Arc::new(AtomicUsize::new(0));
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/package", "audit")
            .with_file("/ws/lib.p", "use a.p/foo")
            .with_file("/ws/a.p", "def foo"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new()
            .with(common::Counting::new(Default::default()).resolving(resolves.clone())),
    )
    .with_retention(Retention::session());
    engine
        .run(DepsQuery {
            path: "lib.p".into(),
        })
        .unwrap();
    let first = resolves.load(Ordering::SeqCst);
    assert!(first > 0);
    for _ in 0..3 {
        engine.run(FileQuery { path: "a.p".into() }).unwrap();
    }
    engine
        .run(DepsQuery {
            path: "lib.p".into(),
        })
        .unwrap();
    assert_eq!(resolves.load(Ordering::SeqCst), first);
}

#[test]
fn namespace_scopes_follow_project_changes_after_read_only_queries() {
    let fixture = Fixture::new(&[
        ("package", "audit"),
        ("lib.p", "use a.p/foo\nfoo"),
        ("a.p", "def foo"),
    ])
    .retaining(Retention::session());
    let before = fixture.engine.run(ReferencesQuery::new("foo")).unwrap();
    assert!(
        before
            .occurrences
            .iter()
            .filter(|o| o.m.path == Path::new("lib.p"))
            .all(|o| o.confidence == Confidence::Resolved)
    );
    for _ in 0..2 {
        fixture
            .engine
            .run(FileQuery { path: "a.p".into() })
            .unwrap();
    }
    fixture
        .vfs
        .write(Path::new("/ws/package"), "changed")
        .unwrap();
    let after = fixture.engine.run(ReferencesQuery::new("foo")).unwrap();
    let consumers: Vec<_> = after
        .occurrences
        .iter()
        .filter(|o| o.m.path == Path::new("lib.p"))
        .collect();
    assert!(!consumers.is_empty());
    assert!(
        consumers
            .iter()
            .all(|o| o.confidence == Confidence::Resolved),
        "{consumers:?}"
    );
}

#[test]
#[ignore = "known bug: apply does not recheck move destination occupancy"]
fn apply_preserves_a_destination_created_after_planning() {
    let fixture = Fixture::new(&[("manifest.p", ""), ("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(MoveIntent::new("a.p", "b.p")).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/b.p"), "precious new file")
        .unwrap();

    let result = fixture.engine.run(Apply(planned));
    assert!(
        result.is_err(),
        "an occupied destination must be refused: {result:?}"
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "precious new file");
    assert_eq!(fixture.read("manifest.p"), "");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
#[ignore = "known bug: step 7 — where report rows discard the declaration source site"]
fn declaration_report_rows_retain_their_source_site() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")]);
    let answer = Answer::Where(
        fixture
            .engine
            .run(WhereQuery {
                name: "foo".into(),
                from: None,
            })
            .unwrap(),
    );
    let report = Document::of(&answer, Options::default());
    let presentation = Detailed.present(&report, Options::default(), usize::MAX);
    let sites: Vec<_> = presentation
        .body
        .into_iter()
        .filter_map(|row| row.source)
        .map(|source| (source.path.to_path_buf(), source.line))
        .collect();
    assert_eq!(sites, [(PathBuf::from("a.p"), 0)]);
}
