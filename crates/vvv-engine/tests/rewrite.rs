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
    FileQuery {
        path: RelPath::from("a.p"),
    }
    .execute(engine)
    .unwrap()
    .text
}

#[test]
fn rewrite_all_matches_with_template() {
    let engine = engine();
    let planned = RewriteIntent::new(Query::pattern("foo"), "bar$NEXT!")
        .plan(&engine)
        .unwrap();
    assert!(!planned.state.is_applied());
    assert_eq!(planned.preview()[0].after, "bar1! bar2!\nbar3!");
    assert_eq!(
        read(&engine),
        "foo:1 foo:2\nfoo:3",
        "preview must not write"
    );

    let rewrite = Apply(planned).apply(&engine).unwrap();
    assert!(rewrite.state == vvv_engine::MutationState::Applied { history_id: 1 });
    assert_eq!(read(&engine), "bar1! bar2!\nbar3!");
}

#[test]
fn selection_narrows_and_rejects_unknown_ids() {
    let engine = engine();
    let matches = vvv_engine::SearchQuery::from(Query::pattern("foo"))
        .execute(&engine)
        .unwrap()
        .matches;
    let intent = RewriteIntent::new(Query::pattern("foo"), "X")
        .selecting(Selection::ids([matches[1].id.clone()]));
    let planned = intent.clone().plan(&engine).unwrap();
    assert_eq!(planned.preview()[0].after, "foo:1 X\nfoo:3");

    let bogus = RewriteIntent::new(Query::pattern("foo"), "X").selecting(Selection::ids([
        vvv_engine::MatchId::derive(&vvv_engine::RelPath::from("z"), Span::new(0, 1), "?"),
    ]));
    assert!(matches!(
        bogus.clone().plan(&engine),
        Err(EngineError::Selection(_))
    ));
}

#[test]
fn unknown_template_variable_names_the_location() {
    let engine = engine();
    let err = RewriteIntent::new(Query::pattern("foo"), "$MISSING")
        .plan(&engine)
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Template { line: 0, .. }),
        "{err}"
    );
}
