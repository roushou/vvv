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
    let error = result.unwrap_err();
    assert_eq!(error.code(), vvv_engine::ErrorCode::Exists);
    assert!(
        matches!(&error, EngineError::Apply(vvv_engine::ApplyError::DestinationExists { path }) if path == Path::new("b.p"))
    );
    let failure = serde_json::to_value(vvv_engine::Failure::from(&error)).unwrap();
    assert_eq!(failure["code"], "exists");
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

#[test]
fn rewrite_refuses_coordinates_from_a_changed_source() {
    let fixture = Fixture::new(&[("a.p", "foo:1")])
        .retaining(Retention::session().trusting(Duration::from_secs(3600)));
    fixture.engine.run(vvv_core::Query::pattern("foo")).unwrap();
    fixture.vfs.write(Path::new("/ws/a.p"), "xyz:9").unwrap();
    let result = fixture.engine.run(vvv_engine::RewriteIntent::new(
        vvv_core::Query::pattern("foo"),
        "bar$NEXT",
    ));
    assert!(matches!(
        result,
        Err(EngineError::Apply(vvv_engine::ApplyError::Stale { .. }))
    ));
    assert_eq!(fixture.read("a.p"), "xyz:9");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn rewrite_of_refuses_matches_that_no_longer_describe_the_source() {
    let fixture = Fixture::new(&[("a.p", "foo:1")]);
    let query = vvv_core::Query::pattern("foo");
    let matches = fixture.engine.run(query.clone()).unwrap().matches;
    fixture.vfs.write(Path::new("/ws/a.p"), "foo:2").unwrap();
    let result = fixture.engine.run(vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
        matches,
    });
    assert!(matches!(
        result,
        Err(EngineError::Apply(vvv_engine::ApplyError::Stale { .. }))
    ));
    assert_eq!(fixture.read("a.p"), "foo:2");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn rewrite_of_accepts_unchanged_matches_after_unrelated_source_edits() {
    let fixture = Fixture::new(&[("a.p", "foo:1\nbefore")]);
    let query = vvv_core::Query::pattern("foo");
    let matches = fixture.engine.run(query.clone()).unwrap().matches;
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "foo:1\nafter")
        .unwrap();
    let planned = fixture
        .engine
        .run(vvv_engine::RewriteOf {
            intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
            matches,
        })
        .unwrap();
    fixture.engine.run(Apply(planned)).unwrap();
    assert_eq!(fixture.read("a.p"), "bar1\nafter");
}

#[test]
fn rewrite_of_checks_captures_even_when_the_match_id_is_unchanged() {
    let fixture = Fixture::new(&[("a.p", "foo:1")]);
    let query = vvv_core::Query::pattern("foo");
    let mut matches = fixture.engine.run(query.clone()).unwrap().matches;
    let vvv_core::CaptureValue::Single(capture) = matches[0].captures.get_mut("NEXT").unwrap()
    else {
        panic!("the fake supplies a single capture");
    };
    capture.text = "forged".into();
    let result = fixture.engine.run(vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
        matches,
    });
    assert!(matches!(
        result,
        Err(EngineError::Apply(vvv_engine::ApplyError::Stale { .. }))
    ));
    assert_eq!(fixture.read("a.p"), "foo:1");
}

/// Simulates an external write immediately after the engine obtains a snapshot.
struct ChangingRead {
    inner: Arc<MemoryVfs>,
    path: PathBuf,
    replacement: String,
    pending: std::sync::atomic::AtomicBool,
}

impl ChangingRead {
    fn new(inner: Arc<MemoryVfs>, path: PathBuf, replacement: &str) -> Self {
        Self {
            inner,
            path,
            replacement: replacement.into(),
            pending: true.into(),
        }
    }
}

impl Vfs for ChangingRead {
    fn read(&self, path: &Path) -> Result<String, vvv_engine::VfsError> {
        let text = self.inner.read(path)?;
        if path == self.path && self.pending.swap(false, Ordering::SeqCst) {
            self.inner.write(path, &self.replacement)?;
        }
        Ok(text)
    }
    fn stamp(&self, path: &Path) -> Result<vvv_engine::Stamp, vvv_engine::VfsError> {
        self.inner.stamp(path)
    }
    fn write(&self, path: &Path, contents: &str) -> Result<(), vvv_engine::VfsError> {
        self.inner.write(path, contents)
    }
    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }
    fn entry_kind(
        &self,
        path: &Path,
    ) -> Result<Option<vvv_engine::EntryKind>, vvv_engine::VfsError> {
        self.inner.entry_kind(path)
    }
    fn prepare_parent(&self, path: &Path) -> vvv_engine::ParentCreation {
        self.inner.prepare_parent(path)
    }
    fn remove_file(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.remove_file(path)
    }
    fn remove_empty_dir(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.remove_empty_dir(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.rename(from, to)
    }
    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, vvv_engine::VfsError> {
        self.inner.walk(root)
    }
}

#[test]
fn rewrite_sequence_captures_use_the_matched_source() {
    let inner = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "foo:[one, two]"));
    let vfs = Arc::new(ChangingRead::new(
        inner.clone(),
        PathBuf::from("/ws/a.p"),
        "x",
    ));
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default()),
    );
    let result = engine.run(vvv_engine::RewriteIntent::new(
        vvv_core::Query::pattern("foo"),
        "bar($$$NEXT)",
    ));
    assert!(matches!(
        result,
        Err(EngineError::Apply(vvv_engine::ApplyError::Stale { .. }))
    ));
    assert_eq!(inner.read(Path::new("/ws/a.p")).unwrap(), "x");
    assert!(!inner.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn rewrite_sequence_captures_preserve_matched_punctuation() {
    let fixture = Fixture::new(&[("a.p", "foo:[one,  two]")]);
    let planned = fixture
        .engine
        .run(vvv_engine::RewriteIntent::new(
            vvv_core::Query::pattern("foo"),
            "bar($$$NEXT)",
        ))
        .unwrap();
    assert_eq!(planned.preview()[0].after, "bar(one,  two)");
    fixture.engine.run(Apply(planned)).unwrap();
    assert_eq!(fixture.read("a.p"), "bar(one,  two)");
}

#[test]
fn moves_refuse_changed_sources_even_without_text_edits() {
    let fixture = Fixture::new(&[("manifest.p", ""), ("a.p", "def local")])
        .retaining(Retention::session().trusting(Duration::from_secs(3600)));
    fixture.engine.run(ReferencesQuery::new("local")).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "def changed")
        .unwrap();
    let result = fixture.engine.run(MoveIntent::new("a.p", "b.p"));
    assert!(
        matches!(result, Err(EngineError::Apply(vvv_engine::ApplyError::Stale { path })) if path == Path::new("a.p"))
    );
    assert_eq!(fixture.read("a.p"), "def changed");
    assert_eq!(fixture.read("manifest.p"), "");
    assert!(!fixture.vfs.exists(Path::new("/ws/b.p")));
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn rewrite_of_respects_the_query_language() {
    let fixture = Fixture::new(&[("a.p", "foo:1")]);
    let query = vvv_core::Query::pattern("foo");
    let matches = fixture.engine.run(query.clone()).unwrap().matches;
    let result = fixture.engine.run(vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query.in_language("other"), "bar"),
        matches,
    });
    assert!(matches!(
        result,
        Err(EngineError::Apply(vvv_engine::ApplyError::Stale { .. }))
    ));
    assert_eq!(fixture.read("a.p"), "foo:1");
}

#[test]
fn rewrite_of_accepts_reported_declaration_addresses() {
    let fixture = Fixture::new(&[("a.p", "def foo")]);
    let query = vvv_core::Query::named("foo");
    let matches = fixture.engine.run(query.clone()).unwrap().matches;
    assert!(matches[0].address.is_some());
    let planned = fixture
        .engine
        .run(vvv_engine::RewriteOf {
            intent: vvv_engine::RewriteIntent::new(query, "def bar"),
            matches,
        })
        .unwrap();
    fixture.engine.run(Apply(planned)).unwrap();
    assert_eq!(fixture.read("a.p"), "def bar");
}

#[test]
fn directory_moves_check_all_destinations_before_writes() {
    let fixture = Fixture::new(&[
        ("manifest.p", ""),
        ("a/a.p", "def first"),
        ("a/z.p", "def last"),
        ("lib.p", "use a/a.p/first\nfirst"),
    ]);
    let planned = fixture.engine.run(MoveIntent::new("a", "d")).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/d/z.p"), "precious new file")
        .unwrap();
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    assert!(
        matches!(error, EngineError::Apply(vvv_engine::ApplyError::DestinationExists { path }) if path == Path::new("d/z.p"))
    );
    assert_eq!(fixture.read("a/a.p"), "def first");
    assert_eq!(fixture.read("a/z.p"), "def last");
    assert_eq!(fixture.read("d/z.p"), "precious new file");
    assert_eq!(fixture.read("lib.p"), "use a/a.p/first\nfirst");
    assert_eq!(fixture.read("manifest.p"), "");
    assert!(!fixture.vfs.exists(Path::new("/ws/d/a.p")));
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn a_batch_restores_earlier_steps_when_a_move_destination_becomes_occupied() {
    let fixture = Fixture::new(&[("manifest.p", ""), ("a.p", "def foo\nfoo")]);
    let planned = fixture
        .engine
        .run(vvv_engine::BatchIntent::new([
            Intent::Rename(RenameIntent::new("foo", "bar")),
            Intent::Move(MoveIntent::new("a.p", "b.p")),
        ]))
        .unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/b.p"), "precious new file")
        .unwrap();
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    assert!(
        matches!(error, EngineError::Apply(vvv_engine::ApplyError::DestinationExists { path }) if path == Path::new("b.p"))
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "precious new file");
    assert_eq!(fixture.read("manifest.p"), "");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

struct DiskHistoryFixture {
    root: PathBuf,
    engine: Engine,
}

impl DiskHistoryFixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "vvv-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".vvv")).unwrap();
        std::fs::write(root.join("a.p"), "def foo\nfoo").unwrap();
        let engine = Engine::new(
            Workspace::disk(&root).unwrap(),
            Languages::new().with(Fake::default()),
        );
        Self { root, engine }
    }

    fn apply(&self) -> Result<vvv_engine::Rename, EngineError> {
        let planned = self.engine.run(RenameIntent::new("foo", "bar")).unwrap();
        self.engine.run(Apply(planned))
    }

    fn source(&self) -> String {
        std::fs::read_to_string(self.root.join("a.p")).unwrap()
    }
}

impl Drop for DiskHistoryFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn apply_preserves_files_when_history_cannot_be_read() {
    let fixture = DiskHistoryFixture::new();
    std::fs::create_dir(fixture.root.join(".vvv/history.json")).unwrap();
    assert!(matches!(
        fixture.apply(),
        Err(EngineError::History(HistoryError::Vfs(_)))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(fixture.root.join(".vvv/history.json").is_dir());
}

#[test]
fn apply_preserves_files_when_history_is_not_utf8() {
    let fixture = DiskHistoryFixture::new();
    std::fs::write(fixture.root.join(".vvv/history.json"), [0xff]).unwrap();
    assert!(matches!(
        fixture.apply(),
        Err(EngineError::History(HistoryError::Vfs(
            vvv_engine::VfsError::InvalidUtf8 { .. }
        )))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert_eq!(
        std::fs::read(fixture.root.join(".vvv/history.json")).unwrap(),
        [0xff]
    );
}

#[test]
fn apply_restores_a_file_after_a_partial_write_failure() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn apply_restores_every_file_including_the_partially_failed_write() {
    let fixture = common::FaultFixture::new(&[("a.p", "foo"), ("b.p", "foo")]);
    let planned = fixture
        .engine
        .run(vvv_engine::RewriteIntent::new(
            vvv_engine::Query::pattern("foo"),
            "bar",
        ))
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "b.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "foo");
    assert_eq!(fixture.read("b.p"), "foo");
}

#[test]
fn apply_restores_a_write_that_completed_before_returning_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

#[test]
fn recovery_does_not_rewrite_a_file_whose_failed_write_changed_nothing() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.vfs.clear_trace();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Before,
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(
        fixture
            .vfs
            .trace()
            .iter()
            .filter(|(operation, _)| *operation == common::FaultOperation::Write)
            .count(),
        1
    );
}

#[test]
fn apply_restores_a_moved_file_after_its_write_fails() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = fixture
        .engine
        .run(MoveIntent::new("a.p", "dir/b.p"))
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "dir/b.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/dir/b.p")));
}

#[test]
fn apply_restores_a_move_that_completed_before_returning_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = fixture.engine.run(MoveIntent::new("a.p", "b.p")).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/b.p")));
}

#[test]
fn recovery_removes_a_destination_left_by_an_incomplete_move() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = fixture.engine.run(MoveIntent::new("a.p", "b.p")).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::Partial(String::new()),
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/b.p")));
}

#[test]
fn recovery_reports_a_destination_it_cannot_remove() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = fixture.engine.run(MoveIntent::new("a.p", "b.p")).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::Partial(String::new()),
    );
    fixture.arm(
        common::FaultOperation::RemoveFile,
        "b.p",
        0,
        common::FaultAction::Before,
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(
        recovery.details.failures[0].operation,
        vvv_engine::RecoveryOperation::RemoveFile
    );
    assert_eq!(recovery.details.failures[0].path, Path::new("b.p"));
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new("b.p"));
    assert_eq!(
        recovery.details.remaining[0].expected,
        vvv_engine::RecoveryState::Absent
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "def foo\nfoo");
}

#[test]
fn recovery_continues_and_reports_every_unrestored_effect() {
    let fixture = common::FaultFixture::new(&[("a.p", "foo"), ("b.p", "foo"), ("c.p", "foo")]);
    let planned = fixture
        .engine
        .run(vvv_engine::RewriteIntent::new(
            vvv_engine::Query::pattern("foo"),
            "bar",
        ))
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "c.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        "c.p",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Write,
        "b.p",
        1,
        common::FaultAction::Before,
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    assert_eq!(error.code(), vvv_engine::ErrorCode::RecoveryFailed);
    let failure = vvv_engine::Failure::from(&error);
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(recovery.details.failures.len(), 2);
    assert_eq!(
        recovery
            .details
            .remaining
            .iter()
            .map(|effect| effect.path.as_ref())
            .collect::<Vec<&Path>>(),
        [Path::new("b.p"), Path::new("c.p")]
    );
    assert!(recovery.details.unverified.is_empty());
    assert_eq!(
        fixture.read("a.p"),
        "foo",
        "independent restoration continues"
    );
    assert_eq!(fixture.read("b.p"), "bar");
    assert_eq!(fixture.read("c.p"), "cut");
    let json = serde_json::to_value(&failure).unwrap();
    assert_eq!(json["code"], "recovery_failed");
    assert_eq!(json["recovery"]["cause"]["code"], "io");
    assert_eq!(json["recovery"]["remaining"][0]["path"], "b.p");
    assert_eq!(
        serde_json::from_value::<vvv_engine::Failure>(json).unwrap(),
        failure
    );
    assert!(failure.message.contains("b.p") && failure.message.contains("c.p"));
}

#[test]
fn recovery_identifies_paths_it_cannot_verify() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Inspect,
        "a.p",
        1,
        common::FaultAction::Always,
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert!(recovery.details.remaining.is_empty());
    assert_eq!(recovery.details.unverified.len(), 1);
    assert_eq!(recovery.details.unverified[0].path, Path::new("a.p"));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

#[test]
fn recovery_verifies_restoration_even_when_the_restore_write_returns_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::After,
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

#[test]
fn recovery_reports_both_paths_of_a_move_it_cannot_reverse() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = fixture.engine.run(MoveIntent::new("a.p", "b.p")).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "b.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Rename,
        "b.p",
        0,
        common::FaultAction::Before,
    );
    let error = fixture.engine.run(Apply(planned)).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(
        recovery
            .details
            .remaining
            .iter()
            .map(|effect| effect.path.as_ref())
            .collect::<Vec<&Path>>(),
        [Path::new("a.p"), Path::new("b.p")]
    );
    assert_eq!(
        recovery.details.remaining[0].observed,
        vvv_engine::RecoveryState::Absent
    );
    assert!(!fixture.vfs.exists(Path::new("/ws/a.p")));
    assert_eq!(fixture.read("b.p"), "def foo\nfoo");
}

#[test]
fn history_validation_precedes_every_mutation_attempt() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = fixture.engine.run(RenameIntent::new("foo", "bar")).unwrap();
    fixture.vfs.clear_trace();
    fixture.arm(
        common::FaultOperation::Read,
        ".vvv/history.json",
        0,
        common::FaultAction::Always,
    );
    assert!(matches!(
        fixture.engine.run(Apply(planned)),
        Err(EngineError::History(_))
    ));
    assert!(
        fixture
            .vfs
            .trace()
            .iter()
            .all(|(operation, _)| *operation == common::FaultOperation::Read)
    );
    assert!(
        fixture
            .vfs
            .trace()
            .iter()
            .any(|(_, path)| path == Path::new("/ws/.vvv/history.json"))
    );
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

impl DiskHistoryFixture {
    fn fault_engine(&self) -> (Arc<common::FaultVfs>, Engine) {
        std::fs::write(self.root.join("manifest.p"), "").unwrap();
        let vfs = Arc::new(common::FaultVfs::over(Arc::new(vvv_engine::DiskVfs::new())));
        let engine = Engine::new(
            Workspace::new(&self.root, vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        (vfs, engine)
    }
}

#[test]
fn failed_mutations_remove_owned_empty_parent_directories() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = engine
        .run(MoveIntent::new("a.p", "nested/deep/b.p"))
        .unwrap();
    vfs.arm(
        common::FaultOperation::PrepareParent,
        &fixture.root.join("nested/deep/b.p"),
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
}

#[test]
fn recovery_reports_an_owned_directory_it_cannot_remove() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = engine.run(MoveIntent::new("a.p", "nested/b.p")).unwrap();
    vfs.arm(
        common::FaultOperation::PrepareParent,
        &fixture.root.join("nested/b.p"),
        0,
        common::FaultAction::After,
    );
    vfs.arm(
        common::FaultOperation::RemoveDirectory,
        &fixture.root.join("nested"),
        0,
        common::FaultAction::Before,
    );
    let error = engine.run(Apply(planned)).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new("nested"));
    assert_eq!(
        recovery.details.remaining[0].expected,
        vvv_engine::RecoveryState::Absent
    );
    assert_eq!(
        recovery.details.remaining[0].observed,
        vvv_engine::RecoveryState::Directory
    );
    assert_eq!(fixture.source(), "def foo\nfoo");
}

#[test]
fn recovery_restores_disk_files_and_cleans_move_parent_directories() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = engine
        .run(MoveIntent::new("a.p", "nested/deep/b.p"))
        .unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join("nested/deep/b.p"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        engine.run(Apply(planned)),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
    assert!(!fixture.root.join(".vvv/history.json").exists());
}
