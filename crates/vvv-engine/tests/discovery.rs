mod common;
use common::Fake;
use std::sync::Arc;
use vvv_engine::{Call, DiscoveryQuery, Engine, Languages, MemoryVfs, Workspace};

#[test]
fn discovery_matches_the_build_and_describes_the_new_requests() {
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(MemoryVfs::new())),
        Languages::new().with(Fake::default()),
    );
    let discovery = DiscoveryQuery::default().execute(&engine);
    assert_eq!(discovery.schemas_available, cfg!(feature = "schema"));
    assert_eq!(
        discovery.commands.iter().any(|c| c.command == "schema"),
        cfg!(feature = "schema")
    );
    assert_eq!(discovery.languages, engine.language_ids());
    let context = discovery
        .commands
        .iter()
        .find(|c| c.command == "context")
        .unwrap();
    assert!(context.read_only);
    assert!(context.parameters.iter().any(|p| p == "budget"));
    assert_eq!(
        discovery.context_defaults,
        vvv_engine::ContextBudget::default()
    );
    assert!(
        discovery
            .commands
            .iter()
            .filter(|c| c.read_only)
            .all(|c| !["rename", "rewrite", "undo"].contains(&c.command.as_str()))
    );
}
#[test]
fn serve_budgets_are_structured_and_never_discard_a_mutation_receipt() {
    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "x".repeat(5000)));
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default()),
    );
    let call: Call = serde_json::from_value(
        serde_json::json!({"id":"budget", "command":"file", "path":"a.p", "max_output_bytes":1024}),
    )
    .unwrap();
    let reply = serde_json::to_value(call.execute(&engine)).unwrap();
    assert_eq!(reply["id"], "budget");
    assert_eq!(reply["code"], "output_limit");
    assert_eq!(reply["output_limit"]["max_bytes"], 1024);
    assert!(reply["output_limit"]["required_bytes"].as_u64().unwrap() > 5000);
    let call: Call = serde_json::from_value(
        serde_json::json!({"command":"batch", "intents":[], "apply":true, "max_output_bytes":1024}),
    )
    .unwrap();
    let reply = serde_json::to_value(call.execute(&engine)).unwrap();
    assert_eq!(reply["code"], "bad_request");
    assert!(
        vvv_engine::Ledger::new(&engine)
            .history()
            .unwrap()
            .entries
            .is_empty()
    );
    let call: Call =
        serde_json::from_value(serde_json::json!({"command":"file", "path":"a.p"})).unwrap();
    assert_eq!(
        serde_json::to_value(call.execute(&engine)).unwrap()["status"],
        "ok"
    );
}

#[test]
fn a_context_call_uses_the_outer_result_budget_without_losing_valid_json() {
    let source = "def Root ".to_owned() + &"é".repeat(5000);
    let symbol = vvv_engine::Symbol::plain(
        vvv_engine::SymbolKind::Function,
        "Root",
        vvv_engine::Span::new(4, 8),
        vvv_engine::Span::new(0, source.len()),
    );
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/package", "ws")
                    .with_file("/ws/a.p", &source),
            ),
        ),
        Languages::new().with(Fake::default().with_symbols(vec![symbol])),
    );
    let call: Call = serde_json::from_value(serde_json::json!({"id":3,"command":"context","origin":{"kind":"position","path":"a.p","position":{"line":0,"column":4}},"max_output_bytes":1024})).unwrap();
    let reply = serde_json::to_value(call.execute(&engine)).unwrap();
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["id"], 3);
    assert!(serde_json::to_vec(&reply["result"]).unwrap().len() <= 1024);
    assert_eq!(reply["result"]["items"][0]["complete"], false);
}
