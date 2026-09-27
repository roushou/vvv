//! Typed capability entry points preserve concrete outputs alongside wire routing.
mod common;

use std::sync::Arc;

use common::Fake;
use vvv_engine::{
    Apply, Batch, BatchIntent, Engine, History, Languages, Ledger, MemoryVfs, Planned, Query,
    References, ReferencesQuery, Rename, RenameIntent, Rewrite, RewriteIntent, Search, SearchQuery,
    Undo, Workspace,
};

#[test]
fn typed_queries_preserve_their_concrete_answers() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def foo\nfoo")),
        ),
        Languages::new().with(Fake::default()),
    );
    let search: Search = SearchQuery::from(Query::pattern("foo"))
        .execute(&engine)
        .unwrap();
    assert_eq!(search.matches.len(), 2);
    let references: References = ReferencesQuery::new("foo").execute(&engine).unwrap();
    assert_eq!(references.occurrences.len(), 2);
    assert_eq!(
        serde_json::to_value(&references).unwrap(),
        serde_json::to_value(engine.run(ReferencesQuery::new("foo")).unwrap()).unwrap()
    );
}

#[test]
fn typed_planning_and_application_keep_the_history_completion() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def foo\nfoo")),
        ),
        Languages::new().with(Fake::default()),
    );
    let plan: Planned<Rename> = RenameIntent::new("foo", "bar").plan(&engine).unwrap();
    let ledger = Ledger::new(&engine);
    let before: History = ledger.history().unwrap();
    assert!(before.entries.is_empty());
    let applied = Apply(plan).apply(&engine).unwrap();
    let history: History = ledger.history().unwrap();
    assert_eq!(history.entries[0].id, applied.history_id());
    let undone: Undo = ledger.undo().unwrap();
    assert_eq!(undone.undone.id, applied.history_id());
    assert!(ledger.history().unwrap().entries.is_empty());
}

#[test]
fn typed_rewrite_and_batch_plans_remain_read_only() {
    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def foo\nfoo"));
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(Fake::default()),
    );
    let rewrite: Planned<Rewrite> = RewriteIntent::new(Query::pattern("foo"), "bar")
        .plan(&engine)
        .unwrap();
    assert!(!rewrite.files.is_empty());
    let batch: Planned<Batch> = BatchIntent {
        intents: vec![RenameIntent::new("foo", "bar").into()],
    }
    .plan(&engine)
    .unwrap();
    assert!(!batch.files.is_empty());
    assert_eq!(
        vvv_engine::Vfs::read(&*vfs, std::path::Path::new("/ws/a.p")).unwrap(),
        "def foo\nfoo"
    );
}
