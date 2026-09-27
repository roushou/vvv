//! Several intents as one: the second is planned against what the first
//! leaves, both apply as one transaction, one undo reverses both.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::Query;
use vvv_engine::{
    Apply, BatchIntent, Engine, EngineError, FileQuery, Intent, Languages, MemoryVfs, MoveIntent,
    RelPath, RenameIntent, RewriteIntent, Vfs, Workspace,
};

fn engine_and_vfs() -> (Arc<MemoryVfs>, Engine) {
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/manifest.p", "")
            .with_file("/ws/a/x.p", "def foo\nfoo")
            .with_file("/ws/lib.p", "use a/x.p\nfoo"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(Fake::default()),
    );
    (vfs, engine)
}

fn engine() -> Engine {
    engine_and_vfs().1
}

fn read(engine: &Engine, path: &str) -> String {
    FileQuery {
        path: RelPath::from(path),
    }
    .execute(engine)
    .unwrap()
    .text
}

fn exists(engine: &Engine, path: &str) -> bool {
    FileQuery {
        path: RelPath::from(path),
    }
    .execute(engine)
    .is_ok()
}

#[test]
fn a_rename_can_follow_the_move_of_its_file() {
    let engine = engine();
    let batch = BatchIntent::new([
        Intent::Move(MoveIntent::new("a/x.p", "b/y.p")),
        // Planned against the moved tree: the declaration is in b/y.p now.
        Intent::Rename(RenameIntent::new("foo", "bar").declared_in("b/y.p")),
    ])
    .plan(&engine)
    .unwrap();
    assert_eq!(batch.intents.len(), 2);
    assert!(exists(&engine, "a/x.p"), "nothing real moved yet");

    // The combined preview: pre-batch paths and contents against the end state.
    // Paths compare by component, so this reads the same on Windows, where a
    // joined path spells its separator as `\`.
    let files: Vec<(&Path, Option<&Path>, &str)> = batch
        .preview()
        .iter()
        .map(|f| (f.path.as_path(), f.moved_to.as_deref(), f.after.as_str()))
        .collect();
    assert!(
        files.contains(&(Path::new("a/x.p"), Some(Path::new("b/y.p")), "def bar\nbar")),
        "{files:?}"
    );
    assert!(
        files.contains(&(Path::new("lib.p"), None, "use b/y.p\nbar")),
        "{files:?}"
    );

    let applied = Apply(batch).apply(&engine).unwrap();
    assert!(applied.state == vvv_engine::MutationState::Applied { history_id: 1 });
    assert_eq!(read(&engine, "b/y.p"), "def bar\nbar");
    assert_eq!(read(&engine, "lib.p"), "use b/y.p\nbar");
    assert_eq!(
        vvv_engine::Ledger::new(&engine)
            .history()
            .unwrap()
            .entries
            .len(),
        1,
        "one entry for the whole batch"
    );

    let undone = vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert!(
        matches!(undone.undone.intent, Intent::Batch(BatchIntent { ref intents }) if intents.len() == 2)
    );
    assert_eq!(read(&engine, "a/x.p"), "def foo\nfoo");
    assert_eq!(read(&engine, "lib.p"), "use a/x.p\nfoo");
    assert!(!exists(&engine, "b/y.p"));
}

#[test]
fn a_failing_step_rolls_the_earlier_ones_back() {
    let (vfs, engine) = engine_and_vfs();
    let batch = BatchIntent::new(
        [
            // Touches a/x.p only.
            Intent::Rewrite(RewriteIntent::new(Query::pattern("def"), "fn")),
            // Touches both files.
            Intent::Rewrite(RewriteIntent::new(Query::pattern("foo"), "bar")),
        ]
        .iter()
        .cloned(),
    )
    .plan(&engine)
    .unwrap();
    // Somebody edits lib.p between planning and applying: step one still
    // holds, step two is stale.
    vfs.write(Path::new("/ws/lib.p"), "use a/x.p\nfoo // touched")
        .unwrap();
    let err = Apply(batch).apply(&engine).unwrap_err();
    assert!(matches!(err, EngineError::Apply(_)), "{err}");
    assert_eq!(
        read(&engine, "a/x.p"),
        "def foo\nfoo",
        "step one rolled back"
    );
    assert_eq!(
        read(&engine, "lib.p"),
        "use a/x.p\nfoo // touched",
        "the edit that broke it is kept"
    );
    assert_eq!(
        vvv_engine::Ledger::new(&engine)
            .history()
            .unwrap()
            .entries
            .len(),
        0
    );
}

#[test]
fn batch_recovers_every_attempted_step_after_a_partial_write() {
    let fixture = common::FaultFixture::new(&[("a.p", "one")]);
    let planned = BatchIntent::new([
        Intent::Rewrite(RewriteIntent::new(Query::pattern("one"), "two")),
        Intent::Rewrite(RewriteIntent::new(Query::pattern("two"), "three")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        1,
        common::FaultAction::Partial("cut".into()),
    );
    assert!(matches!(
        Apply(planned).apply(&fixture.engine),
        Err(EngineError::Apply(_))
    ));
    assert_eq!(fixture.read("a.p"), "one");
}

#[test]
fn batch_reports_failure_to_restore_an_earlier_step() {
    let fixture = common::FaultFixture::new(&[("a.p", "one"), ("b.p", "two")]);
    let planned = BatchIntent::new([
        Intent::Rewrite(RewriteIntent::new(Query::pattern("one"), "1")),
        Intent::Rewrite(RewriteIntent::new(Query::pattern("two"), "2")),
    ])
    .plan(&fixture.engine)
    .unwrap();
    fixture.arm(
        common::FaultOperation::Write,
        "b.p",
        0,
        common::FaultAction::Before,
    );
    fixture.arm(
        common::FaultOperation::Write,
        "a.p",
        1,
        common::FaultAction::Before,
    );
    let error = Apply(planned).apply(&fixture.engine).unwrap_err();
    let EngineError::Recovery(recovery) = error else {
        panic!("expected recovery error: {error}")
    };
    assert_eq!(recovery.details.remaining.len(), 1);
    assert_eq!(recovery.details.remaining[0].path, Path::new("a.p"));
    assert_eq!(fixture.read("a.p"), "1");
    assert_eq!(fixture.read("b.p"), "two");
}
