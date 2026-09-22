//! The wire round trip: a request read from JSON runs as the command it
//! names, and its answer prints as that command's result; a mutation with
//! `apply` writes and records one undo.

mod common;

use std::sync::Arc;

use common::Fake;
use vvv_engine::{
    Answer, Engine, EngineError, ErrorCode, Failure, FileQuery, Languages, MemoryVfs, RelPath,
    Request, Retention, Workspace,
};

fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.p", "def foo\nfoo")
        .with_file("/ws/b.p", "use a.p/foo\nfoo");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
    .with_retention(Retention::session())
}

fn request(json: &str) -> Request {
    serde_json::from_str(json).unwrap()
}

fn read(engine: &Engine, path: &str) -> String {
    engine
        .run(FileQuery {
            path: RelPath::from(path),
        })
        .unwrap()
        .text
}

#[test]
fn a_request_is_the_command_it_names() {
    let engine = engine();
    let answer = engine
        .run(request(r#"{"command": "search", "name": "foo"}"#))
        .unwrap();
    let Answer::Search(search) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(search.matches.len(), 1);
    let answer = engine
        .run(request(r#"{"command": "references", "name": "foo"}"#))
        .unwrap();
    let Answer::References(refs) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(refs.occurrences.len(), 4);
    let answer = engine
        .run(request(r#"{"command": "deps", "path": "b.p"}"#))
        .unwrap();
    let value = serde_json::to_value(&answer).unwrap();
    assert_eq!(
        value["path"], "b.p",
        "an answer prints as the result itself"
    );
    assert_eq!(value["imports"][0]["path"], "a.p/foo");
    assert!(matches!(
        engine.run(request(r#"{"command": "history"}"#)).unwrap(),
        Answer::History(_)
    ));
}

#[test]
fn a_mutation_previews_unless_it_applies_and_then_records_one_undo() {
    let engine = engine();
    let preview = request(r#"{"command": "rename", "name": "foo", "to": "bar"}"#);
    let Answer::Rename(rename) = engine.run(preview).unwrap() else {
        panic!()
    };
    assert!(!rename.applied);
    assert_eq!(
        read(&engine, "a.p"),
        "def foo\nfoo",
        "a preview writes nothing"
    );

    let apply = request(r#"{"command": "rename", "name": "foo", "to": "bar", "apply": true}"#);
    let Answer::Rename(rename) = engine.run(apply).unwrap() else {
        panic!()
    };
    assert!(rename.applied && rename.history_id.is_some());
    assert_eq!(read(&engine, "a.p"), "def bar\nbar");

    let Answer::Undo(undo) = engine.run(request(r#"{"command": "undo"}"#)).unwrap() else {
        panic!()
    };
    assert_eq!(undo.restored.len(), 2);
    assert_eq!(read(&engine, "a.p"), "def foo\nfoo");
}

#[test]
fn a_request_serialises_as_its_intent_plus_apply() {
    let request = request(r#"{"command": "rename", "name": "foo", "to": "bar", "apply": true}"#);
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["command"], "rename");
    assert_eq!(value["apply"], true);
    assert_eq!(value["name"], "foo");
    // An intent printed by `--json` is a request without `apply`.
    let intent = serde_json::json!({"command": "move", "from": "a.p", "to": "c.p"});
    assert!(matches!(
        serde_json::from_value::<Request>(intent).unwrap(),
        Request::Move { apply: false, .. }
    ));
}

#[test]
fn errors_have_codes_and_hints() {
    let engine = engine();
    let error = engine
        .run(request(
            r#"{"command": "rename", "name": "nope", "to": "x"}"#,
        ))
        .unwrap_err();
    let failure = Failure::from(&error);
    assert_eq!(failure.code, ErrorCode::NoSuchSymbol);
    assert!(failure.hint.unwrap().contains("vvv search --name nope"));
    assert_eq!(
        serde_json::to_value(ErrorCode::AmbiguousSymbol).unwrap(),
        "ambiguous_symbol"
    );
    let empty = engine.run(request(r#"{"command": "search"}"#)).unwrap_err();
    assert!(matches!(empty, EngineError::Query(_)));
    assert_eq!(Failure::from(&empty).code, ErrorCode::BadQuery);
    let undo = engine.run(request(r#"{"command": "undo"}"#)).unwrap_err();
    assert_eq!(Failure::from(&undo).code, ErrorCode::NoHistory);
}
