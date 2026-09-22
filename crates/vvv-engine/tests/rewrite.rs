//! Rewrite against the shared fake language (`$NEXT` captures the word after a hit).

mod common;

use std::sync::Arc;

use common::Fake;
use vvv_core::{Query, Span};
use vvv_engine::{
    Apply, Engine, EngineError, FileQuery, Languages, MemoryVfs, RelPath, RewriteIntent, Selection,
    Workspace,
};

fn engine() -> Engine {
    let vfs = MemoryVfs::new().with_file("/ws/a.p", "foo:1 foo:2\nfoo:3");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

fn read(engine: &Engine) -> String {
    engine
        .run(FileQuery {
            path: RelPath::from("a.p"),
        })
        .unwrap()
        .text
}

#[test]
fn rewrite_all_matches_with_template() {
    let engine = engine();
    let planned = engine
        .run(RewriteIntent::new(Query::pattern("foo"), "bar$NEXT!"))
        .unwrap();
    assert!(!planned.applied);
    assert_eq!(planned.preview()[0].after, "bar1! bar2!\nbar3!");
    assert_eq!(
        read(&engine),
        "foo:1 foo:2\nfoo:3",
        "preview must not write"
    );

    let rewrite = engine.run(Apply(planned)).unwrap();
    assert!(rewrite.applied && rewrite.history_id == Some(1));
    assert_eq!(read(&engine), "bar1! bar2!\nbar3!");
}

#[test]
fn selection_narrows_and_rejects_unknown_ids() {
    let engine = engine();
    let matches = engine.run(Query::pattern("foo")).unwrap().matches;
    let intent = RewriteIntent::new(Query::pattern("foo"), "X")
        .selecting(Selection::ids([matches[1].id.clone()]));
    let planned = engine.run(intent.clone()).unwrap();
    assert_eq!(planned.preview()[0].after, "foo:1 X\nfoo:3");

    let bogus = RewriteIntent::new(Query::pattern("foo"), "X").selecting(Selection::ids([
        vvv_engine::MatchId::derive(&vvv_engine::RelPath::from("z"), Span::new(0, 1), "?"),
    ]));
    assert!(matches!(
        engine.run(bogus.clone()),
        Err(EngineError::Selection(_))
    ));
}

#[test]
fn unknown_template_variable_names_the_location() {
    let engine = engine();
    let err = engine
        .run(RewriteIntent::new(Query::pattern("foo"), "$MISSING"))
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Template { line: 0, .. }),
        "{err}"
    );
}
