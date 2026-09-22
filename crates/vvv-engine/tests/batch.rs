//! Several intents as one: the second is planned against what the first
//! leaves, both apply as one transaction, one undo reverses both.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::Query;
use vvv_engine::{
    Apply, BatchIntent, Engine, EngineError, FileQuery, HistoryQuery, Intent, Languages, MemoryVfs,
    MoveIntent, RelPath, RenameIntent, RewriteIntent, UndoLast, Vfs, Workspace,
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
    engine
        .run(FileQuery {
            path: RelPath::from(path),
        })
        .unwrap()
        .text
}

fn exists(engine: &Engine, path: &str) -> bool {
    engine
        .run(FileQuery {
            path: RelPath::from(path),
        })
        .is_ok()
}

#[test]
fn a_rename_can_follow_the_move_of_its_file() {
    let engine = engine();
    let batch = engine
        .run(BatchIntent::new([
            Intent::Move(MoveIntent::new("a/x.p", "b/y.p")),
            // Planned against the moved tree: the declaration is in b/y.p now.
            Intent::Rename(RenameIntent::new("foo", "bar").declared_in("b/y.p")),
        ]))
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

    let applied = engine.run(Apply(batch)).unwrap();
    assert!(applied.applied && applied.history_id == Some(1));
    assert_eq!(read(&engine, "b/y.p"), "def bar\nbar");
    assert_eq!(read(&engine, "lib.p"), "use b/y.p\nbar");
    assert_eq!(
        engine.run(HistoryQuery).unwrap().entries.len(),
        1,
        "one entry for the whole batch"
    );

    let undone = engine.run(UndoLast).unwrap();
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
    let batch = engine
        .run(BatchIntent::new(
            [
                // Touches a/x.p only.
                Intent::Rewrite(RewriteIntent::new(Query::pattern("def"), "fn")),
                // Touches both files.
                Intent::Rewrite(RewriteIntent::new(Query::pattern("foo"), "bar")),
            ]
            .iter()
            .cloned(),
        ))
        .unwrap();
    // Somebody edits lib.p between planning and applying: step one still
    // holds, step two is stale.
    vfs.write(Path::new("/ws/lib.p"), "use a/x.p\nfoo // touched")
        .unwrap();
    let err = engine.run(Apply(batch)).unwrap_err();
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
    assert_eq!(engine.run(HistoryQuery).unwrap().entries.len(), 0);
}
