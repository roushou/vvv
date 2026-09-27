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
    FileQuery {
        path: RelPath::from(path),
    }
    .execute(engine)
    .unwrap()
    .text
}

#[test]
fn a_request_is_the_command_it_names() {
    let engine = engine();
    let answer = engine
        .run(request(r#"{"command": "search", "name": "foo"}"#))
        .unwrap()
        .into_answer();
    let Answer::Search(search) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(search.matches.len(), 1);
    let answer = engine
        .run(request(r#"{"command": "references", "name": "foo"}"#))
        .unwrap()
        .into_answer();
    let Answer::References(refs) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(refs.occurrences.len(), 4);
    let answer = engine
        .run(request(r#"{"command": "deps", "path": "b.p"}"#))
        .unwrap()
        .into_answer();
    let value = serde_json::to_value(&answer).unwrap();
    assert_eq!(
        value["path"], "b.p",
        "an answer prints as the result itself"
    );
    assert_eq!(value["imports"][0]["path"], "a.p/foo");
    assert!(matches!(
        engine
            .run(request(r#"{"command": "history"}"#))
            .unwrap()
            .into_answer(),
        Answer::History(_)
    ));
}

#[test]
fn a_mutation_previews_unless_it_applies_and_then_records_one_undo() {
    let engine = engine();
    let preview = request(r#"{"command": "rename", "name": "foo", "to": "bar"}"#);
    let Answer::Rename(rename) = engine.run(preview).unwrap().into_answer() else {
        panic!()
    };
    assert!(!rename.state.is_applied());
    assert_eq!(
        read(&engine, "a.p"),
        "def foo\nfoo",
        "a preview writes nothing"
    );

    let apply = request(r#"{"command": "rename", "name": "foo", "to": "bar", "apply": true}"#);
    let Answer::Rename(rename) = engine.run(apply).unwrap().into_answer() else {
        panic!()
    };
    assert!(rename.state.is_applied() && rename.state.history_id().is_some());
    assert_eq!(read(&engine, "a.p"), "def bar\nbar");

    let Answer::Undo(undo) = engine
        .run(request(r#"{"command": "undo"}"#))
        .unwrap()
        .into_answer()
    else {
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

#[test]
fn rename_composes_a_flat_references_query_on_the_wire() {
    use vvv_engine::{RenameIntent, Selection, SymbolKind};

    for fields in 0..8 {
        for selection in [
            Selection::All,
            Selection::ids([]),
            Selection::ordinals([1, 3]),
        ] {
            let mut intent = RenameIntent::new("foo", "bar").selecting(selection);
            if fields & 1 != 0 {
                intent = intent.of_symbol(SymbolKind::Function);
            }
            if fields & 2 != 0 {
                intent = intent.in_language("fake");
            }
            if fields & 4 != 0 {
                intent = intent.declared_in("a.p");
            }
            let value = serde_json::to_value(&intent).unwrap();
            assert_eq!(value["name"], "foo");
            assert_eq!(value["to"], "bar");
            assert!(value.get("references").is_none());
            assert_eq!(value.get("symbol").is_some(), fields & 1 != 0);
            assert_eq!(value.get("language").is_some(), fields & 2 != 0);
            assert_eq!(value.get("declared_in").is_some(), fields & 4 != 0);
            assert_eq!(
                serde_json::from_value::<RenameIntent>(value).unwrap(),
                intent
            );
            let request = Request::Rename {
                intent,
                apply: true,
            };
            let value = serde_json::to_value(&request).unwrap();
            let decoded: Request = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        }
    }
}

#[test]
fn dispatch_preserves_an_executable_preview_and_its_applied_completion() {
    use vvv_engine::{Apply, ExecutionKind, Ledger, MutationAnswer};

    let engine = engine();
    let execution = engine
        .run(request(r#"{"command":"rename","name":"foo","to":"bar"}"#))
        .unwrap();
    assert_eq!(execution.kind(), ExecutionKind::Preview);
    let planned = execution.into_preview().unwrap();
    assert!(matches!(&*planned, MutationAnswer::Rename(_)));
    assert_eq!(read(&engine, "a.p"), "def foo\nfoo");
    let applied = Apply(planned).apply(&engine).unwrap();
    assert_eq!(
        Ledger::new(&engine).history().unwrap().entries[0].id,
        applied.history_id()
    );
    assert!(applied.into_inner().history_id().is_some());
    let execution = engine
        .run(request(
            r#"{"command":"rename","name":"bar","to":"baz","apply":true}"#,
        ))
        .unwrap();
    assert_eq!(execution.kind(), ExecutionKind::Applied);
    let applied = execution.into_applied().unwrap();
    assert_eq!(applied.history_id(), 2);
    assert!(applied.into_inner().history_id().is_some());
}

#[test]
fn execution_kind_mismatches_are_structured_rejections() {
    use vvv_engine::{ExecutionKind, Ledger};

    let engine = engine();
    let completed = engine
        .run(request(r#"{"command":"search","name":"foo"}"#))
        .unwrap();
    let error = completed.into_preview().unwrap_err();
    assert!(matches!(
        error,
        EngineError::ExecutionKind {
            expected: ExecutionKind::Preview,
            actual: ExecutionKind::Completed
        }
    ));
    assert_eq!(Failure::from(&error).code, ErrorCode::BadRequest);
    let preview = engine
        .run(request(r#"{"command":"rename","name":"foo","to":"bar"}"#))
        .unwrap();
    let error = preview.into_applied().unwrap_err();
    assert!(matches!(
        error,
        EngineError::ExecutionKind {
            expected: ExecutionKind::Applied,
            actual: ExecutionKind::Preview
        }
    ));
    assert_eq!(read(&engine, "a.p"), "def foo\nfoo");
    assert!(Ledger::new(&engine).history().unwrap().entries.is_empty());
}

#[test]
fn intent_conversion_changes_only_execution_policy_on_the_wire() {
    use vvv_engine::Intent;

    for json in [
        r#"{"command":"rename","name":"foo","to":"bar","declared_in":"a.p"}"#,
        r#"{"command":"move","from":"a.p","to":"c.p"}"#,
        r#"{"command":"move_symbol","name":"foo","from":"a.p","to":"b.p"}"#,
        r#"{"command":"rewrite","query":{"pattern":"foo"},"template":"bar"}"#,
        r#"{"command":"batch","intents":[]}"#,
    ] {
        let intent: Intent = serde_json::from_str(json).unwrap();
        let mut expected = serde_json::to_value(&intent).unwrap();
        for apply in [false, true] {
            expected["apply"] = serde_json::json!(apply);
            let request = intent.clone().into_request(apply);
            assert_eq!(serde_json::to_value(&request).unwrap(), expected);
            let decoded: Request = serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(decoded, request);
        }
    }
}
