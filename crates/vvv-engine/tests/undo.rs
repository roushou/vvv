//! Undo against the shared fake language.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::Query;
use vvv_engine::{
    Apply, Engine, EngineError, FileQuery, HistoryError, HistoryQuery, Intent, Languages,
    MemoryVfs, RelPath, RewriteIntent, UndoLast, Vfs, Workspace,
};

fn engine() -> (Arc<MemoryVfs>, Engine) {
    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "one two one"));
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(Fake::default()),
    );
    (vfs, engine)
}

fn read(engine: &Engine) -> String {
    engine
        .run(FileQuery {
            path: RelPath::from("a.p"),
        })
        .unwrap()
        .text
}

fn rewrite(engine: &Engine, from: &str, to: &str) {
    let intent = RewriteIntent::new(Query::pattern(from), to);
    let planned = engine.run(intent.clone()).unwrap();
    engine.run(Apply(planned)).unwrap();
}

#[test]
fn undo_pops_applies_in_reverse_order() {
    let (_, engine) = engine();
    rewrite(&engine, "one", "1");
    rewrite(&engine, "two", "2");
    assert_eq!(read(&engine), "1 2 1");
    assert_eq!(engine.run(HistoryQuery).unwrap().entries.len(), 2);

    let undone = engine.run(UndoLast).unwrap();
    assert_eq!(
        undone.undone.intent,
        Intent::Rewrite(RewriteIntent::new(Query::pattern("two"), "2"))
    );
    assert_eq!(read(&engine), "1 two 1");

    engine.run(UndoLast).unwrap();
    assert_eq!(read(&engine), "one two one");
    assert!(matches!(
        engine.run(UndoLast),
        Err(EngineError::History(HistoryError::Empty))
    ));
}

#[test]
fn undo_refuses_when_files_changed_and_keeps_the_entry() {
    let (vfs, engine) = engine();
    rewrite(&engine, "one", "1");
    vfs.write(Path::new("/ws/a.p"), "1 two 1 edited").unwrap();
    assert!(matches!(engine.run(UndoLast), Err(EngineError::Apply(_))));
    assert_eq!(
        engine.run(HistoryQuery).unwrap().entries.len(),
        1,
        "entry stays for a later manual fix"
    );
}
