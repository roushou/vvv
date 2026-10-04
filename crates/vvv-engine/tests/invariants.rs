//! Known failures of source provenance, transactions, namespace revisions,
//! and report sites. Each ignored test states the invariant a repair must restore.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::Fake;
use common::fixture::EngineFixture as Fixture;
use vvv_core::Address;
use vvv_engine::report::{Detailed, Document, Options, View};
use vvv_engine::{
    Answer, Apply, Confidence, DepsQuery, Engine, EngineError, FileQuery, HistoryError, Intent,
    Languages, MemoryVfs, MoveIntent, MutationAnswer, ReferencesQuery, RenameIntent, Retention,
    Vfs, WhereQuery, Workspace,
};

#[test]
fn apply_refuses_a_plan_whose_source_changed() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")])
        .retaining(Retention::session().trusting(Duration::from_secs(3600)));
    ReferencesQuery::new("foo")
        .execute(&fixture.engine)
        .unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "def xyz\nxyz")
        .unwrap();

    // Planning and applying both must reject coordinates from the cached foo
    // snapshot; either boundary may detect that the source changed.
    let result = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .and_then(|planned| Apply(planned).apply(&fixture.engine));
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
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();

    let result = Apply(planned).apply(&fixture.engine);
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
fn applying_a_query_result_is_rejected_before_writes() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")]);
    let _planned = fixture
        .engine
        .run((Intent::Rename(RenameIntent::new("foo", "bar"))).into_request(false))
        .and_then(vvv_engine::Execution::into_preview)
        .unwrap();
    let query = fixture.engine.run(vvv_engine::Request::History).unwrap();
    let rejected = MutationAnswer::try_from(query.into_answer());
    assert!(matches!(rejected, Err(Answer::History(_))));
    // Planned's compile-fail doctests reject substituting query presentation data.
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
    let before = DepsQuery {
        path: "lib.p".into(),
    }
    .execute(&fixture.engine)
    .unwrap();
    assert_eq!(before.module, Some(Address::new("audit", ["lib.p"])));
    assert_eq!(
        before.imports[0].address,
        Some(Address::new("audit", ["a.p", "foo"]))
    );
    for _ in 0..2 {
        FileQuery { path: "a.p".into() }
            .execute(&fixture.engine)
            .unwrap();
    }
    fixture
        .vfs
        .write(Path::new("/ws/package"), "changed")
        .unwrap();

    let after = DepsQuery {
        path: "lib.p".into(),
    }
    .execute(&fixture.engine)
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
    DepsQuery {
        path: "lib.p".into(),
    }
    .execute(&engine)
    .unwrap();
    let first = resolves.load(Ordering::SeqCst);
    assert!(first > 0);
    for _ in 0..3 {
        FileQuery { path: "a.p".into() }.execute(&engine).unwrap();
    }
    DepsQuery {
        path: "lib.p".into(),
    }
    .execute(&engine)
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
    let before = ReferencesQuery::new("foo")
        .execute(&fixture.engine)
        .unwrap();
    assert!(
        before
            .occurrences
            .iter()
            .filter(|o| o.m.path == Path::new("lib.p"))
            .all(|o| o.confidence == Confidence::Resolved)
    );
    for _ in 0..2 {
        FileQuery { path: "a.p".into() }
            .execute(&fixture.engine)
            .unwrap();
    }
    fixture
        .vfs
        .write(Path::new("/ws/package"), "changed")
        .unwrap();
    let after = ReferencesQuery::new("foo")
        .execute(&fixture.engine)
        .unwrap();
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
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/b.p"), "precious new file")
        .unwrap();

    let result = Apply(planned).apply(&fixture.engine);
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
fn declaration_report_rows_retain_their_source_site() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")]);
    let answer = Answer::Where(
        WhereQuery {
            name: "foo".into(),
            from: None,
        }
        .execute(&fixture.engine)
        .unwrap(),
    );
    let report = Document::of(&answer);
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
    vvv_engine::SearchQuery::from(vvv_core::Query::pattern("foo"))
        .execute(&fixture.engine)
        .unwrap();
    fixture.vfs.write(Path::new("/ws/a.p"), "xyz:9").unwrap();
    let result = vvv_engine::RewriteIntent::new(vvv_core::Query::pattern("foo"), "bar$NEXT")
        .plan(&fixture.engine);
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
    let matches = vvv_engine::SearchQuery::from(query.clone())
        .execute(&fixture.engine)
        .unwrap()
        .matches;
    fixture.vfs.write(Path::new("/ws/a.p"), "foo:2").unwrap();
    let result = vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
        matches,
    }
    .plan(&fixture.engine);
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
    let matches = vvv_engine::SearchQuery::from(query.clone())
        .execute(&fixture.engine)
        .unwrap()
        .matches;
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "foo:1\nafter")
        .unwrap();
    let planned = vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
        matches,
    }
    .plan(&fixture.engine)
    .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    assert_eq!(fixture.read("a.p"), "bar1\nafter");
}

#[test]
fn rewrite_of_checks_captures_even_when_the_match_id_is_unchanged() {
    let fixture = Fixture::new(&[("a.p", "foo:1")]);
    let query = vvv_core::Query::pattern("foo");
    let mut matches = vvv_engine::SearchQuery::from(query.clone())
        .execute(&fixture.engine)
        .unwrap()
        .matches;
    let vvv_core::CaptureValue::Single(capture) = matches[0].captures.get_mut("NEXT").unwrap()
    else {
        panic!("the fake supplies a single capture");
    };
    capture.text = "forged".into();
    let result = vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "bar$NEXT"),
        matches,
    }
    .plan(&fixture.engine);
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
    fn entry_path(&self, path: &Path) -> Result<Option<PathBuf>, vvv_engine::VfsError> {
        self.inner.entry_path(path)
    }

    fn same_entry(&self, from: &Path, to: &Path) -> Result<bool, vvv_engine::VfsError> {
        self.inner.same_entry(from, to)
    }

    fn names_alias(&self, from: &Path, to: &Path) -> Result<bool, vvv_engine::VfsError> {
        self.inner.names_alias(from, to)
    }

    fn prepare_parent(&self, path: &Path) -> vvv_engine::ParentCreation {
        self.inner.prepare_parent(path)
    }
    fn remove_file(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.remove_file(path)
    }
    fn create_dir(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.create_dir(path)
    }

    fn remove_empty_dir(&self, path: &Path) -> Result<(), vvv_engine::VfsError> {
        self.inner.remove_empty_dir(path)
    }
    fn move_if_absent(&self, from: &Path, to: &Path) -> Result<(), vvv_engine::MoveError> {
        self.inner.move_if_absent(from, to)
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
    let result = vvv_engine::RewriteIntent::new(vvv_core::Query::pattern("foo"), "bar($$$NEXT)")
        .plan(&engine);
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
    let planned = vvv_engine::RewriteIntent::new(vvv_core::Query::pattern("foo"), "bar($$$NEXT)")
        .plan(&fixture.engine)
        .unwrap();
    assert_eq!(planned.preview()[0].after, "bar(one,  two)");
    Apply(planned).apply(&fixture.engine).unwrap();
    assert_eq!(fixture.read("a.p"), "bar(one,  two)");
}

#[test]
fn moves_refuse_changed_sources_even_without_text_edits() {
    let fixture = Fixture::new(&[("manifest.p", ""), ("a.p", "def local")])
        .retaining(Retention::session().trusting(Duration::from_secs(3600)));
    ReferencesQuery::new("local")
        .execute(&fixture.engine)
        .unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), "def changed")
        .unwrap();
    let result = MoveIntent::new("a.p", "b.p").plan(&fixture.engine);
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
    let matches = vvv_engine::SearchQuery::from(query.clone())
        .execute(&fixture.engine)
        .unwrap()
        .matches;
    let result = vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query.in_language("other"), "bar"),
        matches,
    }
    .plan(&fixture.engine);
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
    let matches = vvv_engine::SearchQuery::from(query.clone())
        .execute(&fixture.engine)
        .unwrap()
        .matches;
    assert!(matches[0].address.is_some());
    let planned = vvv_engine::RewriteOf {
        intent: vvv_engine::RewriteIntent::new(query, "def bar"),
        matches,
    }
    .plan(&fixture.engine)
    .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
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
    let planned = MoveIntent::new("a", "d").plan(&fixture.engine).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/d/z.p"), "precious new file")
        .unwrap();
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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
    let planned = vvv_engine::BatchIntent::new([
        Intent::Rename(RenameIntent::new("foo", "bar")),
        Intent::Move(MoveIntent::new("a.p", "b.p")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/b.p"), "precious new file")
        .unwrap();
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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

    fn apply(&self) -> Result<vvv_engine::Applied<vvv_engine::Rename>, EngineError> {
        let planned = RenameIntent::new("foo", "bar").plan(&self.engine).unwrap();
        Apply(planned).apply(&self.engine)
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
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn apply_restores_every_file_including_the_partially_failed_write() {
    let fixture = common::FaultFixture::new(&[("a.p", "foo"), ("b.p", "foo")]);
    let before = fixture.source_tree();
    let planned = vvv_engine::RewriteIntent::new(vvv_engine::Query::pattern("foo"), "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "b.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.source_tree(), before);
}

#[test]
fn apply_restores_a_write_that_completed_before_returning_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

#[test]
fn recovery_does_not_rewrite_a_file_whose_failed_write_changed_nothing() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.vfs.clear_trace();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        0,
        common::FaultAction::Before,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
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
    let planned = MoveIntent::new("a.p", "dir/b.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "dir/b.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/dir/b.p")));
}

#[test]
fn apply_restores_a_move_that_completed_before_returning_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/b.p")));
}

#[test]
fn recovery_removes_a_destination_left_by_an_incomplete_move() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::Partial(String::new()),
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert!(!fixture.vfs.exists(Path::new("/ws/b.p")));
}

#[test]
fn recovery_reports_a_destination_it_cannot_remove() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
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
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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
    let planned = vvv_engine::RewriteIntent::new(vvv_engine::Query::pattern("foo"), "bar")
        .plan(&fixture.engine)
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
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
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
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
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
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
}

#[test]
fn recovery_reports_both_paths_of_a_move_it_cannot_reverse() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
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
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
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
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.vfs.clear_trace();
    fixture.arm(
        common::FaultOperation::Read,
        ".vvv/history.json",
        0,
        common::FaultAction::Always,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
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
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    vfs.arm(
        common::FaultOperation::PrepareParent,
        &fixture.root.join("nested/deep/b.p"),
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        Apply(planned).apply(&engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
}

#[test]
fn recovery_reports_an_owned_directory_it_cannot_remove() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/b.p").plan(&engine).unwrap();
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
    let error = Apply(planned).apply(&engine).unwrap_err();
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
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join("nested/deep/b.p"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        Apply(planned).apply(&engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
    assert!(!fixture.root.join(".vvv/history.json").exists());
}

#[test]
fn apply_preserves_a_destination_created_during_its_move() {
    let fixture = common::FaultFixture::new(&[
        (
            "a.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::Occupy(
            "def foo
foo"
            .into(),
        ),
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    assert_eq!(error.code(), vvv_engine::ErrorCode::Exists);
    assert_eq!(
        fixture.read("a.p"),
        "def foo
foo"
    );
    assert_eq!(
        fixture.read("b.p"),
        "def foo
foo"
    );
    assert_eq!(fixture.read("manifest.p"), "");
}

#[test]
fn recovery_preserves_a_source_created_during_the_reverse_move() {
    let fixture = common::FaultFixture::new(&[
        (
            "a.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
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
        common::FaultAction::Occupy("foreign".into()),
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("a.p"), "foreign");
    assert_eq!(
        fixture.read("b.p"),
        "def foo
foo"
    );
    assert_eq!(recovery.details.remaining.len(), 2);
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| effect.path == Path::new("a.p"))
    );
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| effect.path == Path::new("b.p"))
    );
}

#[test]
fn case_only_moves_apply_and_undo_the_stored_spelling() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    Apply(planned).apply(&fixture.engine).unwrap();
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    assert_eq!(
        fixture.read("config.p"),
        "def foo
foo"
    );
    assert!(!fixture.files().iter().any(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".vvv-move-")
    }));
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    assert_eq!(fixture.read("manifest.p"), "");
}

#[test]
fn case_only_moves_restore_the_original_spelling_after_a_partial_write() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "Config.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    assert_eq!(
        fixture.read("config.p"),
        "def foo
foo"
    );
    assert_eq!(
        fixture.files(),
        [
            PathBuf::from("/ws/config.p"),
            PathBuf::from("/ws/manifest.p")
        ]
    );
}

#[test]
fn case_only_moves_restore_the_original_after_the_second_leg_fails() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::RenameDestination,
        "Config.p",
        0,
        common::FaultAction::Before,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    assert_eq!(
        fixture.files(),
        [
            PathBuf::from("/ws/config.p"),
            PathBuf::from("/ws/manifest.p")
        ]
    );
}

#[test]
fn case_only_moves_restore_a_second_leg_that_completed_before_failing() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::RenameDestination,
        "Config.p",
        0,
        common::FaultAction::After,
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    assert_eq!(
        fixture.files(),
        [
            PathBuf::from("/ws/config.p"),
            PathBuf::from("/ws/manifest.p")
        ]
    );
}

#[test]
fn case_only_recovery_reports_unrestored_spelling_even_when_contents_match() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "Config.p",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Rename,
        "Config.p",
        0,
        common::FaultAction::Before,
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = &error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    assert_eq!(
        fixture.read("Config.p"),
        "def foo
foo"
    );
    assert!(recovery.details.remaining.iter().any(|effect| matches!(
        &effect.expected, vvv_engine::RecoveryState::File { spelling: Some(spelling), .. } if spelling == Path::new("config.p")
    ) && matches!(&effect.observed, vvv_engine::RecoveryState::File { spelling: Some(spelling), .. } if spelling == Path::new("Config.p"))));
    let wire = serde_json::to_value(vvv_engine::Failure::from(&error)).unwrap();
    assert_eq!(wire["code"], "recovery_failed");
    assert!(
        wire["recovery"]["remaining"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["observed"]["spelling"] == "Config.p")
    );
}

#[test]
fn case_only_moves_preserve_a_destination_created_between_their_legs() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::RenameDestination,
        "Config.p",
        0,
        common::FaultAction::Occupy("foreign".into()),
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("config.p"), "foreign");
    let temporary = fixture
        .files()
        .into_iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".vvv-move-")
        })
        .unwrap();
    assert_eq!(
        fixture.vfs.base.read(&temporary).unwrap(),
        "def foo
foo"
    );
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| Path::new("/ws").join(&effect.path) == temporary)
    );
}

#[test]
fn case_only_moves_compose_in_a_batch_and_undo_to_the_original_spelling() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = vvv_engine::BatchIntent::new([
        Intent::Move(MoveIntent::new("config.p", "Config.p")),
        Intent::Rename(RenameIntent::new("foo", "bar").declared_in("Config.p")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    assert_eq!(
        fixture.read("Config.p"),
        "def bar
bar"
    );
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
    assert_eq!(
        fixture.read("config.p"),
        "def foo
foo"
    );
}

#[test]
fn recovery_of_a_batch_restores_files_before_a_case_only_move() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "a.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = vvv_engine::BatchIntent::new([
        Intent::Move(MoveIntent::new("a.p", "config.p")),
        Intent::Move(MoveIntent::new("config.p", "Config.p")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "Config.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(
        fixture.files(),
        [PathBuf::from("/ws/a.p"), PathBuf::from("/ws/manifest.p")]
    );
    assert_eq!(
        fixture.read("a.p"),
        "def foo
foo"
    );
}

#[test]
fn disk_case_only_moves_apply_and_undo_the_stored_spelling() {
    let fixture = DiskHistoryFixture::new();
    std::fs::write(fixture.root.join("manifest.p"), "").unwrap();
    let source = fixture.root.join("config.p");
    std::fs::rename(fixture.root.join("a.p"), &source).unwrap();
    if !fixture.root.join("Config.p").exists() {
        return; // Case-sensitive host: deterministic Vfs tests cover this on every run.
    }
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    let names: Vec<_> = std::fs::read_dir(&fixture.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(names.contains(&std::ffi::OsString::from("Config.p")));
    assert!(!names.contains(&std::ffi::OsString::from("config.p")));
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    let names: Vec<_> = std::fs::read_dir(&fixture.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(names.contains(&std::ffi::OsString::from("config.p")));
    assert!(!names.contains(&std::ffi::OsString::from("Config.p")));
}

#[test]
fn overlay_preserves_case_aliases_of_files_created_by_staging() {
    let base = Arc::new(common::CaseInsensitiveVfs::new(&[("a.p", "source")]));
    let workspace = Workspace::new("/ws", base.clone()).staged();
    let overlay = workspace.vfs();
    overlay
        .move_if_absent(Path::new("/ws/a.p"), Path::new("/ws/config.p"))
        .unwrap();
    assert_eq!(overlay.read(Path::new("/ws/Config.p")).unwrap(), "source");
    assert!(
        overlay
            .same_entry(Path::new("/ws/config.p"), Path::new("/ws/Config.p"))
            .unwrap()
    );
    assert_eq!(
        overlay
            .entry_path(Path::new("/ws/CONFIG.p"))
            .unwrap()
            .unwrap(),
        Path::new("/ws/config.p")
    );
    overlay.write(Path::new("/ws/Config.p"), "updated").unwrap();
    assert_eq!(
        overlay.walk(Path::new("/ws")).unwrap(),
        [PathBuf::from("/ws/config.p")]
    );
    assert_eq!(overlay.read(Path::new("/ws/config.p")).unwrap(), "updated");
    assert_eq!(base.read(Path::new("/ws/a.p")).unwrap(), "source");
}

#[test]
fn batch_planning_refuses_case_aliases_of_an_occupied_staged_destination() {
    let fixture = common::FaultFixture::case_insensitive(&[
        ("a.p", "def foo"),
        ("b.p", "def bar"),
        ("manifest.p", ""),
    ]);
    let error = vvv_engine::BatchIntent::new([
        Intent::Move(MoveIntent::new("a.p", "config.p")),
        Intent::Move(MoveIntent::new("b.p", "Config.p")),
    ])
    .plan(&fixture.engine)
    .unwrap_err();
    assert_eq!(error.code(), vvv_engine::ErrorCode::Exists);
    assert_eq!(fixture.read("a.p"), "def foo");
    assert_eq!(fixture.read("b.p"), "def bar");
}

#[test]
fn disk_overlay_preserves_case_aliases_of_files_created_by_staging() {
    let fixture = DiskHistoryFixture::new();
    if !fixture.root.join("A.p").exists() {
        return;
    }
    let workspace = Workspace::disk(&fixture.root).unwrap();
    let staged = workspace.staged();
    let from = staged.absolute(Path::new("a.p"));
    let to = staged.absolute(Path::new("config.p"));
    staged.vfs().move_if_absent(&from, &to).unwrap();
    assert_eq!(
        staged
            .vfs()
            .read(&staged.absolute(Path::new("Config.p")))
            .unwrap(),
        "def foo
foo"
    );
    assert_eq!(
        staged
            .vfs()
            .entry_path(&staged.absolute(Path::new("CONFIG.p")))
            .unwrap()
            .unwrap(),
        to
    );
    assert_eq!(
        fixture.source(),
        "def foo
foo"
    );
}

#[test]
fn case_only_moves_restore_every_outcome_of_a_failed_first_leg() {
    for action in [
        common::FaultAction::Before,
        common::FaultAction::After,
        common::FaultAction::Partial(String::new()),
    ] {
        let fixture = common::FaultFixture::case_insensitive(&[
            (
                "config.p",
                "def foo
foo",
            ),
            ("manifest.p", ""),
        ]);
        let planned = MoveIntent::new("config.p", "Config.p")
            .plan(&fixture.engine)
            .unwrap();
        fixture.arm(common::FaultOperation::Rename, "config.p", 0, action);
        let error = Apply(planned).apply(&fixture.engine).unwrap_err();
        assert!(matches!(error, EngineError::Apply(_)), "{error}");
        assert_eq!(
            fixture.stored("Config.p").unwrap(),
            Path::new("/ws/config.p")
        );
        assert_eq!(
            fixture.files(),
            [
                PathBuf::from("/ws/config.p"),
                PathBuf::from("/ws/manifest.p")
            ]
        );
    }
}

#[test]
fn case_only_recovery_reports_a_temporary_file_it_cannot_restore() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::RenameDestination,
        "Config.p",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::RenameDestination,
        "config.p",
        0,
        common::FaultAction::Before,
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    let temporary = fixture
        .files()
        .into_iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".vvv-move-")
        })
        .unwrap();
    assert_eq!(
        fixture.vfs.base.read(&temporary).unwrap(),
        "def foo
foo"
    );
    assert!(fixture.stored("config.p").is_none());
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| Path::new("/ws").join(&effect.path) == temporary
                && effect.expected == vvv_engine::RecoveryState::Absent)
    );
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| effect.path == Path::new("config.p"))
    );
}

#[test]
fn case_only_moves_preserve_an_occupied_temporary_name_and_retry() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "config.p",
        0,
        common::FaultAction::Occupy("foreign".into()),
    );
    Apply(planned).apply(&fixture.engine).unwrap();
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    let foreign = fixture
        .files()
        .into_iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".vvv-move-")
        })
        .unwrap();
    assert_eq!(fixture.vfs.base.read(&foreign).unwrap(), "foreign");
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(fixture.vfs.base.read(&foreign).unwrap(), "foreign");
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
}

#[test]
fn undo_restores_the_source_spelling_observed_at_apply() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    fixture
        .vfs
        .base
        .move_if_absent(Path::new("/ws/config.p"), Path::new("/ws/external.tmp"))
        .unwrap();
    fixture
        .vfs
        .base
        .move_if_absent(Path::new("/ws/external.tmp"), Path::new("/ws/CONFIG.p"))
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/CONFIG.p")
    );
    assert_eq!(
        fixture.read("CONFIG.p"),
        "def foo
foo"
    );
}

#[test]
fn failed_case_only_undo_restores_the_pre_undo_spelling_and_keeps_history() {
    let fixture = common::FaultFixture::case_insensitive(&[
        (
            "config.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "config.p",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::Apply(_)), "{error}");
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    assert_eq!(
        fixture.read("Config.p"),
        "def foo
foo"
    );
    assert_eq!(
        vvv_engine::Ledger::new(&fixture.engine)
            .history()
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(!fixture.files().iter().any(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".vvv-move-")
    }));
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
}

#[test]
fn recovery_preserves_a_destination_whose_acquisition_is_unknown() {
    let fixture = common::FaultFixture::new(&[
        (
            "a.p",
            "def foo
foo",
        ),
        ("manifest.p", ""),
    ]);
    let planned = MoveIntent::new("a.p", "b.p").plan(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Rename,
        "a.p",
        0,
        common::FaultAction::Uncertain,
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(
        fixture.read("a.p"),
        "def foo
foo"
    );
    assert_eq!(
        fixture.read("b.p"),
        "def foo
foo"
    );
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new("b.p"));
}

#[test]
fn failed_first_history_save_restores_files_and_ledger_absence() {
    for action in [
        common::FaultAction::Before,
        common::FaultAction::Partial("cut".into()),
        common::FaultAction::After,
    ] {
        let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
        let planned = RenameIntent::new("foo", "bar")
            .plan(&fixture.engine)
            .unwrap();
        fixture.arm(
            common::FaultOperation::Write,
            ".vvv/history.json",
            0,
            action,
        );
        let error = Apply(planned).apply(&fixture.engine).unwrap_err();
        assert!(matches!(error, EngineError::History(_)), "{error}");
        assert_eq!(fixture.read("a.p"), "def foo\nfoo");
        assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
    }
}

#[test]
fn failed_history_save_restores_the_exact_existing_ledger() {
    for action in [
        common::FaultAction::Before,
        common::FaultAction::Partial("cut".into()),
        common::FaultAction::After,
    ] {
        let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
        let first = RenameIntent::new("foo", "bar")
            .plan(&fixture.engine)
            .unwrap();
        Apply(first).apply(&fixture.engine).unwrap();
        let records: serde_json::Value =
            serde_json::from_str(&fixture.read(".vvv/history.json")).unwrap();
        let original = format!("\n{}\n", serde_json::to_string_pretty(&records).unwrap());
        fixture
            .vfs
            .base
            .write(Path::new("/ws/.vvv/history.json"), &original)
            .unwrap();
        let second = RenameIntent::new("bar", "baz")
            .plan(&fixture.engine)
            .unwrap();
        fixture.arm(
            common::FaultOperation::Write,
            ".vvv/history.json",
            0,
            action,
        );
        let error = Apply(second).apply(&fixture.engine).unwrap_err();
        assert!(matches!(error, EngineError::History(_)), "{error}");
        assert_eq!(fixture.read("a.p"), "def bar\nbar");
        assert_eq!(fixture.read(".vvv/history.json"), original);
        assert_eq!(
            vvv_engine::Ledger::new(&fixture.engine)
                .history()
                .unwrap()
                .entries
                .len(),
            1
        );
    }
}

#[test]
fn failed_batch_history_save_restores_every_step() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "foo")]);
    let batch = vvv_engine::BatchIntent::new([
        Intent::Rename(RenameIntent::new("foo", "bar")),
        Intent::Rename(RenameIntent::new("bar", "baz")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = Apply(batch).apply(&fixture.engine).unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "foo");
    assert!(!fixture.vfs.exists(Path::new("/ws/.vvv/history.json")));
}

#[test]
fn history_save_recovery_reports_an_existing_ledger_it_cannot_restore() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let first = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(first).apply(&fixture.engine).unwrap();
    let second = RenameIntent::new("bar", "baz")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    let error = Apply(second).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = &error else {
        panic!("expected recovery error: {error}")
    };
    assert!(matches!(recovery.cause.as_ref(), EngineError::History(_)));
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(fixture.read(".vvv/history.json"), "cut");
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(
        recovery.details.remaining[0].path,
        Path::new(".vvv/history.json")
    );
    let wire = serde_json::to_value(vvv_engine::Failure::from(&error)).unwrap();
    assert_eq!(wire["code"], "recovery_failed");
    assert_eq!(
        wire["recovery"]["remaining"][0]["path"],
        ".vvv/history.json"
    );
}

#[test]
fn history_save_recovery_reports_a_new_ledger_it_cannot_remove() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::RemoveFile,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(
        recovery.details.remaining[0].path,
        Path::new(".vvv/history.json")
    );
    assert_eq!(
        recovery.details.remaining[0].expected,
        vvv_engine::RecoveryState::Absent
    );
    assert_eq!(
        recovery.details.failures[0].operation,
        vvv_engine::RecoveryOperation::RemoveFile
    );
}

#[test]
fn history_save_recovery_reports_every_unrestored_file_and_ledger() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "foo")]);
    let first = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(first).apply(&fixture.engine).unwrap();
    let second = RenameIntent::new("bar", "baz")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        1,
        common::FaultAction::Before,
    );
    let error = Apply(second).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("a.p"), "def baz\nbaz");
    assert_eq!(fixture.read("b.p"), "bar");
    assert_eq!(fixture.read(".vvv/history.json"), "cut");
    let paths: Vec<_> = recovery
        .details
        .remaining
        .iter()
        .map(|effect| effect.path.as_path())
        .collect();
    assert_eq!(paths, [Path::new(".vvv/history.json"), Path::new("a.p")]);
}

#[test]
fn history_save_recovery_verifies_a_restoration_that_returned_an_error() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let first = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(first).apply(&fixture.engine).unwrap();
    let original = fixture.read(".vvv/history.json");
    let second = RenameIntent::new("bar", "baz")
        .plan(&fixture.engine)
        .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::After,
    );
    let error = Apply(second).apply(&fixture.engine).unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(fixture.read(".vvv/history.json"), original);
}

#[test]
fn failed_history_save_removes_owned_ledger_and_move_directories() {
    let fixture = DiskHistoryFixture::new();
    std::fs::remove_dir(fixture.root.join(".vvv")).unwrap();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = Apply(planned).apply(&engine).unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("manifest.p")).unwrap(),
        ""
    );
    assert!(!fixture.root.join("nested").exists());
    assert!(!fixture.root.join(".vvv").exists());
}

#[test]
fn history_save_recovery_reports_an_owned_ledger_directory_it_cannot_remove() {
    let fixture = DiskHistoryFixture::new();
    std::fs::remove_dir(fixture.root.join(".vvv")).unwrap();
    let (vfs, engine) = fixture.fault_engine();
    let planned = RenameIntent::new("foo", "bar").plan(&engine).unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    vfs.arm(
        common::FaultOperation::RemoveDirectory,
        &fixture.root.join(".vvv"),
        0,
        common::FaultAction::Before,
    );
    let error = Apply(planned).apply(&engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join(".vvv/history.json").exists());
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new(".vvv"));
    assert_eq!(
        recovery.details.remaining[0].observed,
        vvv_engine::RecoveryState::Directory
    );
}

#[test]
fn undo_history_save_failure_restores_files_and_exact_ledger() {
    for action in [
        common::FaultAction::Before,
        common::FaultAction::Partial("cut".into()),
        common::FaultAction::After,
    ] {
        let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
        let planned = RenameIntent::new("foo", "bar")
            .plan(&fixture.engine)
            .unwrap();
        Apply(planned).apply(&fixture.engine).unwrap();
        let records: serde_json::Value =
            serde_json::from_str(&fixture.read(".vvv/history.json")).unwrap();
        let original = format!("\n{}\n", serde_json::to_string_pretty(&records).unwrap());
        fixture
            .vfs
            .base
            .write(Path::new("/ws/.vvv/history.json"), &original)
            .unwrap();
        fixture.arm(
            common::FaultOperation::Write,
            ".vvv/history.json",
            0,
            action,
        );
        let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
        assert!(matches!(error, EngineError::History(_)), "{error}");
        assert_eq!(fixture.read("a.p"), "def bar\nbar");
        assert_eq!(fixture.read(".vvv/history.json"), original);
        assert_eq!(
            vvv_engine::Ledger::new(&fixture.engine)
                .history()
                .unwrap()
                .entries
                .len(),
            1
        );
        vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
        assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    }
}

#[test]
fn undo_of_a_batch_compensates_a_failed_history_save() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "foo")]);
    let batch = vvv_engine::BatchIntent::new([
        Intent::Rename(RenameIntent::new("foo", "bar")),
        Intent::Rename(RenameIntent::new("bar", "baz")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    Apply(batch).apply(&fixture.engine).unwrap();
    let original = fixture.read(".vvv/history.json");
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def baz\nbaz");
    assert_eq!(fixture.read("b.p"), "baz");
    assert_eq!(fixture.read(".vvv/history.json"), original);
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "foo");
}

#[test]
fn undo_history_save_failure_restores_the_applied_case_spelling() {
    let fixture =
        common::FaultFixture::case_insensitive(&[("config.p", "def foo\nfoo"), ("manifest.p", "")]);
    let planned = MoveIntent::new("config.p", "Config.p")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    let original = fixture.read(".vvv/history.json");
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(
        fixture.stored("config.p").unwrap(),
        Path::new("/ws/Config.p")
    );
    assert_eq!(fixture.read("Config.p"), "def foo\nfoo");
    assert_eq!(fixture.read("manifest.p"), "moved\n");
    assert_eq!(fixture.read(".vvv/history.json"), original);
    assert!(!fixture.files().iter().any(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".vvv-move-")
    }));
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(
        fixture.stored("Config.p").unwrap(),
        Path::new("/ws/config.p")
    );
}

#[test]
fn undo_recovery_reports_a_ledger_it_cannot_restore() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert!(matches!(recovery.cause.as_ref(), EngineError::History(_)));
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(fixture.read(".vvv/history.json"), "cut");
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(
        recovery.details.remaining[0].path,
        Path::new(".vvv/history.json")
    );
}

#[test]
fn undo_recovery_reports_a_file_it_cannot_return_to_the_applied_state() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    let original = fixture.read(".vvv/history.json");
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        1,
        common::FaultAction::Before,
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read(".vvv/history.json"), original);
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new("a.p"));
}

#[test]
fn undo_recovery_reports_every_unrestored_file_and_ledger() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo"), ("b.p", "foo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::After,
    );
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        1,
        common::FaultAction::Before,
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    let EngineError::Recovery(recovery) = &error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read("b.p"), "bar");
    assert_eq!(fixture.read(".vvv/history.json"), "[]");
    let paths: Vec<_> = recovery
        .details
        .remaining
        .iter()
        .map(|effect| effect.path.as_path())
        .collect();
    assert_eq!(paths, [Path::new(".vvv/history.json"), Path::new("a.p")]);
    let wire = serde_json::to_value(vvv_engine::Failure::from(&error)).unwrap();
    assert_eq!(wire["code"], "recovery_failed");
    assert_eq!(wire["recovery"]["remaining"].as_array().unwrap().len(), 2);
}

#[test]
fn undo_compensation_uses_the_retained_ledger_when_a_reload_fails() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    let original = fixture.read(".vvv/history.json");
    fixture.arm(
        common::FaultOperation::Write,
        ".vvv/history.json",
        0,
        common::FaultAction::Partial("cut".into()),
    );
    fixture.arm(
        common::FaultOperation::Read,
        ".vvv/history.json",
        1,
        common::FaultAction::Before,
    );
    let error = vvv_engine::Ledger::new(&fixture.engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(fixture.read(".vvv/history.json"), original);
}

#[test]
fn undo_uses_one_validated_history_snapshot() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    fixture.vfs.clear_trace();
    fixture.arm(
        common::FaultOperation::Read,
        ".vvv/history.json",
        1,
        common::FaultAction::Always,
    );
    vvv_engine::Ledger::new(&fixture.engine).undo().unwrap();
    assert_eq!(fixture.read("a.p"), "def foo\nfoo");
    assert_eq!(fixture.read(".vvv/history.json"), "[]");
    assert_eq!(
        fixture
            .vfs
            .trace()
            .iter()
            .filter(
                |(operation, path)| *operation == common::FaultOperation::Read
                    && path == Path::new("/ws/.vvv/history.json")
            )
            .count(),
        1
    );
}

#[test]
fn undo_validates_history_before_attempting_restoration() {
    let fixture = common::FaultFixture::new(&[("a.p", "def foo\nfoo")]);
    let planned = RenameIntent::new("foo", "bar")
        .plan(&fixture.engine)
        .unwrap();
    Apply(planned).apply(&fixture.engine).unwrap();
    fixture
        .vfs
        .base
        .write(Path::new("/ws/.vvv/history.json"), "corrupt")
        .unwrap();
    fixture.vfs.clear_trace();
    assert!(matches!(
        vvv_engine::Ledger::new(&fixture.engine).undo(),
        Err(EngineError::History(_))
    ));
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(fixture.read(".vvv/history.json"), "corrupt");
    assert!(
        fixture
            .vfs
            .trace()
            .iter()
            .all(|(operation, _)| *operation == common::FaultOperation::Read)
    );
}

#[test]
fn undo_removes_only_owned_empty_directories() {
    let fixture = DiskHistoryFixture::new();
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    let (_, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested/deep").exists());
    assert!(fixture.root.join("nested").is_dir());
    assert!(fixture.root.join(".vvv").is_dir());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        "[]"
    );
}

#[test]
fn undo_preserves_owned_directories_that_contain_other_files() {
    let fixture = DiskHistoryFixture::new();
    let (_, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    std::fs::write(fixture.root.join("nested/deep/foreign.txt"), "foreign").unwrap();
    vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("nested/deep/foreign.txt")).unwrap(),
        "foreign"
    );
    assert!(!fixture.root.join("nested/deep/b.p").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        "[]"
    );
}

#[test]
fn undo_of_a_batch_cleans_owned_directories_across_all_steps() {
    let fixture = DiskHistoryFixture::new();
    let (_, engine) = fixture.fault_engine();
    let batch = vvv_engine::BatchIntent::new([
        Intent::Move(MoveIntent::new("a.p", "one/b.p")),
        Intent::Move(MoveIntent::new("one/b.p", "two/deep/c.p")),
    ])
    .plan(&engine)
    .unwrap();
    Apply(batch).apply(&engine).unwrap();
    vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("one").exists());
    assert!(!fixture.root.join("two").exists());
}

#[test]
fn undo_directory_cleanup_failure_restores_the_applied_state() {
    for action in [common::FaultAction::Before, common::FaultAction::After] {
        let fixture = DiskHistoryFixture::new();
        let (vfs, engine) = fixture.fault_engine();
        let planned = MoveIntent::new("a.p", "nested/deep/b.p")
            .plan(&engine)
            .unwrap();
        Apply(planned).apply(&engine).unwrap();
        let original = std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap();
        let manifest = std::fs::read_to_string(fixture.root.join("manifest.p")).unwrap();
        vfs.arm(
            common::FaultOperation::RemoveDirectory,
            &fixture.root.join("nested/deep"),
            0,
            action,
        );
        let error = vvv_engine::Ledger::new(&engine).undo().unwrap_err();
        assert!(matches!(error, EngineError::Apply(_)), "{error}");
        assert!(!fixture.root.join("a.p").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("nested/deep/b.p")).unwrap(),
            "def foo\nfoo"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("manifest.p")).unwrap(),
            manifest
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
            original
        );
        vvv_engine::Ledger::new(&engine).undo().unwrap();
        assert_eq!(fixture.source(), "def foo\nfoo");
        assert!(!fixture.root.join("nested").exists());
    }
}

#[test]
fn undo_history_save_failure_recreates_removed_owned_directories() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    let original = std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap();
    let manifest = std::fs::read_to_string(fixture.root.join("manifest.p")).unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    let error = vvv_engine::Ledger::new(&engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert!(!fixture.root.join("a.p").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("nested/deep/b.p")).unwrap(),
        "def foo\nfoo"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("manifest.p")).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        original
    );
    vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
}

#[test]
fn undo_recovery_reports_every_directory_and_file_it_cannot_restore() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    let original = std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Partial("cut".into()),
    );
    vfs.arm(
        common::FaultOperation::CreateDirectory,
        &fixture.root.join("nested"),
        0,
        common::FaultAction::Before,
    );
    let error = vvv_engine::Ledger::new(&engine).undo().unwrap_err();
    let EngineError::Recovery(recovery) = &error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert!(!fixture.root.join("nested").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        original
    );
    let paths: Vec<_> = recovery
        .details
        .remaining
        .iter()
        .map(|effect| effect.path.as_path())
        .collect();
    assert_eq!(
        paths,
        [
            Path::new("a.p"),
            Path::new("nested"),
            Path::new("nested/deep"),
            Path::new("nested/deep/b.p")
        ]
    );
    assert!(recovery.details.failures.iter().any(|issue| issue.operation
        == vvv_engine::RecoveryOperation::RestoreDirectory
        && issue.path == Path::new("nested")));
    let wire = serde_json::to_value(vvv_engine::Failure::from(&error)).unwrap();
    assert!(
        wire["recovery"]["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["operation"] == "restore_directory")
    );
}

#[test]
fn undo_recovery_verifies_directory_recreation_that_returned_an_error() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    let original = std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Before,
    );
    vfs.arm(
        common::FaultOperation::CreateDirectory,
        &fixture.root.join("nested"),
        0,
        common::FaultAction::After,
    );
    let error = vvv_engine::Ledger::new(&engine).undo().unwrap_err();
    assert!(matches!(error, EngineError::History(_)), "{error}");
    assert!(!fixture.root.join("a.p").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("nested/deep/b.p")).unwrap(),
        "def foo\nfoo"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        original
    );
}

#[test]
fn undo_recovery_preserves_a_file_created_at_a_removed_directory_path() {
    let fixture = DiskHistoryFixture::new();
    let (vfs, engine) = fixture.fault_engine();
    let planned = MoveIntent::new("a.p", "nested/deep/b.p")
        .plan(&engine)
        .unwrap();
    Apply(planned).apply(&engine).unwrap();
    let original = std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap();
    vfs.arm(
        common::FaultOperation::Write,
        &fixture.root.join(".vvv/history.json"),
        0,
        common::FaultAction::Before,
    );
    vfs.arm(
        common::FaultOperation::CreateDirectory,
        &fixture.root.join("nested"),
        0,
        common::FaultAction::Occupy("foreign".into()),
    );
    let error = vvv_engine::Ledger::new(&engine).undo().unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("nested")).unwrap(),
        "foreign"
    );
    assert_eq!(fixture.source(), "def foo\nfoo");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".vvv/history.json")).unwrap(),
        original
    );
    assert!(
        recovery
            .details
            .remaining
            .iter()
            .any(|effect| effect.path == Path::new("nested")
                && effect.expected == vvv_engine::RecoveryState::Directory
                && matches!(effect.observed, vvv_engine::RecoveryState::File { .. }))
    );
    assert!(
        recovery
            .details
            .failures
            .iter()
            .any(|issue| issue.path == Path::new("nested")
                && issue.operation == vvv_engine::RecoveryOperation::RestoreDirectory)
    );
}

#[test]
fn every_mutation_intent_keeps_its_result_variant_through_apply() {
    use vvv_engine::{BatchIntent, MoveSymbolIntent, Query, RewriteIntent};

    let rename = RenameIntent::new("helper", "renamed");
    let intents = [
        Intent::Rewrite(RewriteIntent::new(Query::pattern("helper"), "renamed")),
        Intent::Rename(rename.clone()),
        Intent::Move(MoveIntent::new("a/x.p", "c/z.p")),
        Intent::MoveSymbol(MoveSymbolIntent::new("helper", "a/x.p", "b/y.p")),
        Intent::Batch(BatchIntent::new([Intent::Rename(rename)])),
    ];
    for intent in intents {
        let fixture = Fixture::new(&[
            ("manifest.p", ""),
            ("a/x.p", "def helper\ndef other\nhelper"),
            ("b/y.p", "use a/x.p/helper\nhelper"),
            ("lib.p", "use a/x.p/helper\nhelper"),
        ]);
        let planned = fixture
            .engine
            .run(intent.clone().into_request(false))
            .unwrap()
            .into_preview()
            .unwrap();
        assert_eq!(fixture.read("a/x.p"), "def helper\ndef other\nhelper");
        let preview = planned.clone().into_inner();
        let value = serde_json::to_value(&preview).unwrap();
        assert_eq!(value["applied"], false);
        assert!(value.get("history_id").is_none());
        let answer = Answer::from(preview);
        let round_trip = MutationAnswer::try_from(answer).unwrap();
        assert_eq!(serde_json::to_value(&round_trip).unwrap(), value);
        // Exercise the public payload deserializers, including their flattened state.
        let decode = |wire| -> Result<serde_json::Value, serde_json::Error> {
            match &round_trip {
                MutationAnswer::Rewrite(_) => serde_json::from_value::<vvv_engine::Rewrite>(wire)
                    .map(|r| serde_json::to_value(r).unwrap()),
                MutationAnswer::Rename(_) => serde_json::from_value::<vvv_engine::Rename>(wire)
                    .map(|r| serde_json::to_value(r).unwrap()),
                MutationAnswer::Move(_) => serde_json::from_value::<vvv_engine::Move>(wire)
                    .map(|r| serde_json::to_value(r).unwrap()),
                MutationAnswer::MoveSymbol(_) => {
                    serde_json::from_value::<vvv_engine::MoveSymbol>(wire)
                        .map(|r| serde_json::to_value(r).unwrap())
                }
                MutationAnswer::Batch(_) => serde_json::from_value::<vvv_engine::Batch>(wire)
                    .map(|r| serde_json::to_value(r).unwrap()),
            }
        };
        assert_eq!(decode(value.clone()).unwrap(), value);
        let mut nullable_preview = value.clone();
        nullable_preview["history_id"] = serde_json::Value::Null;
        assert_eq!(decode(nullable_preview).unwrap(), value);
        for (is_applied, id, message) in [
            (
                false,
                Some(serde_json::json!(1)),
                "a preview cannot have a history_id",
            ),
            (true, None, "an applied mutation requires a history_id"),
            (
                true,
                Some(serde_json::Value::Null),
                "an applied mutation requires a history_id",
            ),
        ] {
            let mut invalid = value.clone();
            invalid["applied"] = serde_json::json!(is_applied);
            if let Some(id) = id {
                invalid["history_id"] = id;
            }
            let error = decode(invalid).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }

        let applied = Apply(planned).apply(&fixture.engine).unwrap();
        assert_eq!(applied.history_id(), 1);
        let mut expected_applied = value;
        expected_applied["applied"] = serde_json::json!(true);
        expected_applied["history_id"] = serde_json::json!(1);
        assert_eq!(serde_json::to_value(&applied).unwrap(), expected_applied);
        assert_eq!(decode(expected_applied.clone()).unwrap(), expected_applied);
        assert_eq!(
            applied.history_id(),
            applied.into_inner().history_id().unwrap()
        );
        assert_eq!(
            vvv_engine::Ledger::new(&fixture.engine)
                .history()
                .unwrap()
                .entries[0]
                .intent,
            intent
        );
    }
}

#[test]
fn detached_presentation_changes_cannot_change_the_plan_or_its_history_intent() {
    let fixture = Fixture::new(&[("a.p", "def foo\nfoo")]);
    let intent = RenameIntent::new("foo", "bar");
    let planned = intent.clone().plan(&fixture.engine).unwrap();
    let mut presentation = (*planned).clone();
    presentation.intent.to = "presentation only".into();
    presentation.files.clear();
    Apply(planned.into_mutation())
        .apply(&fixture.engine)
        .unwrap();
    assert_eq!(fixture.read("a.p"), "def bar\nbar");
    assert_eq!(
        vvv_engine::Ledger::new(&fixture.engine)
            .history()
            .unwrap()
            .entries[0]
            .intent,
        Intent::Rename(intent),
    );
}
