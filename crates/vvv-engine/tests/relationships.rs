mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_core::{CallKind, CallSite, Facts, NamedImport, Symbol, SymbolKind};
use vvv_engine::{
    Engine, EngineError, Languages, MemoryVfs, NavigationQuery, Position, RelationshipKind,
    RelationshipLimit, RelationshipResolution, RelationshipsQuery, Span, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    source: String,
}
impl Fixture {
    fn new() -> Self {
        let source =
            "def work\ndef caller\nwork()\nalias()\nunknown()\nobj.work()\ncallback()\n".to_owned();
        let span = |name: &str| {
            let start = source.find(name).unwrap();
            Span::new(start, start + name.len())
        };
        let work = Symbol::plain(SymbolKind::Function, "work", span("work"), Span::new(0, 8));
        let caller = Symbol::plain(
            SymbolKind::Function,
            "caller",
            span("caller"),
            Span::new(9, source.len()),
        );
        let mut facts = Facts::new(vec![work, caller.clone()], vec![], vec![]);
        facts.calls_supported = true;
        facts.named_modules = true;
        facts.named_imports.push(NamedImport {
            local: "alias".into(),
            imported: "work".into(),
            module: None,
            name_span: span("alias"),
            alias_span: span("alias"),
            reexport: false,
            type_only: false,
        });
        for name in ["work", "caller", "alias", "unknown", "callback"] {
            for (start, _) in source.match_indices(name) {
                let token = Span::new(start, start + name.len());
                facts.push_token(name, "word", token);
                if !source[..start].ends_with("obj.") {
                    facts.navigation.push(token);
                }
                if source[token.end..].starts_with("()") {
                    facts.calls.push(CallSite {
                        span: Span::new(start, token.end + 2),
                        callee: token,
                        kind: if source[..start].ends_with("obj.") {
                            CallKind::Member
                        } else {
                            CallKind::Direct
                        },
                        owner: Some(caller.name_span),
                    });
                }
            }
        }
        facts.calls.sort_by_key(|c| c.callee.start);
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a.p", &source)
                .with_file("/ws/package", "ws"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default().with_navigation_facts(facts)),
        );
        Self {
            engine,
            vfs,
            source,
        }
    }
    fn query(&self, kind: RelationshipKind) -> RelationshipsQuery {
        RelationshipsQuery::new(
            NavigationQuery::at(
                "a.p",
                Position::new(
                    if kind == RelationshipKind::Callees {
                        1
                    } else {
                        0
                    },
                    4,
                ),
            )
            .origin,
            kind,
        )
    }
}
#[test]
fn aliases_are_confirmed_and_receiver_calls_remain_unresolved() {
    let f = Fixture::new();
    let result = f
        .query(RelationshipKind::Callers)
        .execute(&f.engine)
        .unwrap();
    assert!(result.coverage.scan_complete);
    assert_eq!(result.items.len(), 3);
    assert!(matches!(
        result.items[0].resolution,
        RelationshipResolution::Confirmed { .. }
    ));
    assert_eq!(result.items[1].spelling, "alias");
    assert!(matches!(
        result.items[1].resolution,
        RelationshipResolution::Confirmed { .. }
    ));
    assert!(matches!(
        result.items[2].resolution,
        RelationshipResolution::Unavailable { .. }
    ));
    for item in &result.items {
        assert_eq!(item.site.path.as_path(), Path::new("a.p"));
        assert_eq!(
            &f.source[item.site.span.start..item.site.span.end],
            item.spelling
        );
        assert!(item.caller.is_some());
    }
    let outgoing = f
        .query(RelationshipKind::Callees)
        .execute(&f.engine)
        .unwrap();
    assert_eq!(outgoing.items.len(), 5);
    assert!(outgoing.items.iter().any(|i| i.spelling == "unknown"
        && matches!(i.resolution, RelationshipResolution::Unavailable { .. })));
}
#[test]
fn limits_and_scopes_are_explicit_and_output_is_bounded() {
    let f = Fixture::new();
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_lookups = 1;
    let limited = query.execute(&f.engine).unwrap();
    assert_eq!(
        limited.coverage.stopped_by,
        Some(RelationshipLimit::Lookups)
    );
    assert!(!limited.coverage.scan_complete);
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_items = 1;
    let limited = query.execute(&f.engine).unwrap();
    assert_eq!(limited.coverage.stopped_by, Some(RelationshipLimit::Items));
    assert_eq!(limited.items.len(), 1);
    let mut query = f.query(RelationshipKind::Callers);
    query.scope.paths.push("elsewhere".into());
    let scoped = query.execute(&f.engine).unwrap();
    assert!(scoped.items.is_empty());
    assert!(scoped.coverage.scan_complete);
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_bytes = 1024;
    match query.execute(&f.engine) {
        Ok(reply) => assert!(serde_json::to_vec(&reply).unwrap().len() <= 1024),
        Err(EngineError::OutputLimit {
            max_bytes: 1024, ..
        }) => {}
        other => panic!("{other:?}"),
    }
}
#[test]
fn versioned_relationship_origins_reject_edits_and_invalid_budgets() {
    let f = Fixture::new();
    let nav = NavigationQuery::at("a.p", Position::new(0, 4))
        .execute(&f.engine)
        .unwrap();
    let vvv_engine::NavigationOutcome::Resolved { target, .. } = nav.outcome else {
        panic!()
    };
    let query = RelationshipsQuery::new(
        vvv_engine::NavigationOrigin::Symbol { symbol: target },
        RelationshipKind::References,
    );
    f.vfs
        .write(Path::new("/ws/a.p"), &(f.source + "changed"))
        .unwrap();
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::StaleSource { .. })
    ));
    let mut query = RelationshipsQuery::new(
        NavigationQuery::at("missing.p", Position::new(0, 0)).origin,
        RelationshipKind::Callees,
    );
    query.budget.max_items = 0;
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidBudget)
    ));
}

#[test]
fn ambiguity_and_missing_call_facts_are_not_empty_complete_results() {
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/a.p", "def work")
            .with_file("/ws/b.p", "def work")
            .with_file("/ws/use.p", "use a.p/work\nuse b.p/work\nwork")
            .with_file("/ws/package", "ws"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default()),
    );
    let query = RelationshipsQuery::new(
        NavigationQuery::at("use.p", Position::new(2, 0)).origin,
        RelationshipKind::Callers,
    );
    let ambiguous = query.execute(&engine).unwrap();
    let vvv_engine::ResolutionOutcome::Ambiguous { candidates } = ambiguous.subject else {
        panic!()
    };
    assert_eq!(candidates.len(), 2);
    assert!(!ambiguous.coverage.scan_complete);
    let result = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::Callers,
    )
    .execute(&engine)
    .unwrap();
    assert_eq!(result.coverage.unsupported_files, 3);
    assert!(!result.coverage.scan_complete);
}

#[test]
fn edits_during_scanning_and_cancellation_do_not_publish_results() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    for cancel in [false, true] {
        let base = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a.p", "def work")
                .with_file("/ws/package", "ws"),
        );
        let fault = Arc::new(FaultVfs::over(base));
        let token = vvv_engine::ReadCancellation::default();
        if cancel {
            fault.cancel_on_read(Path::new("/ws/a.p"), 0, token.clone());
        } else {
            fault.arm(
                FaultOperation::Read,
                Path::new("/ws/a.p"),
                0,
                FaultAction::ReadThenReplace("def changed".into()),
            );
        }
        let engine = Engine::new(
            Workspace::new("/ws", fault),
            Languages::new().with(Fake::default()),
        );
        let query = RelationshipsQuery::new(
            NavigationQuery::at("a.p", Position::new(0, 4)).origin,
            RelationshipKind::Callers,
        );
        let call = vvv_engine::Call {
            id: None,
            max_output_bytes: None,
            request: vvv_engine::Request::Relationships(query),
        };
        let reply = call.execute_with_cancellation(&engine, &token);
        let value = serde_json::to_value(reply).unwrap();
        assert_eq!(value["status"], "error", "{value}");
        assert_eq!(
            value["code"],
            if cancel { "cancelled" } else { "stale" },
            "{value}"
        );
    }
}

#[test]
fn byte_fitting_omits_whole_sites_and_records_the_omission() {
    let f = Fixture::new();
    let full = f
        .query(RelationshipKind::Callers)
        .execute(&f.engine)
        .unwrap();
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_bytes = serde_json::to_vec(&full).unwrap().len() - 1;
    let limit = query.budget.max_bytes;
    let fitted = query.execute(&f.engine).unwrap();
    assert!(serde_json::to_vec(&fitted).unwrap().len() <= limit);
    assert_eq!(fitted.coverage.stopped_by, Some(RelationshipLimit::Bytes));
    assert!(fitted.coverage.omitted_items > 0);
    assert_eq!(fitted.items, full.items[..fitted.items.len()]);
    assert_eq!(
        fitted.items.len() + fitted.coverage.omitted_items,
        full.items.len()
    );
}

#[test]
fn ambiguous_incoming_sites_keep_all_candidates() {
    use vvv_core::Language;
    let source = "def work\ndef work\nwork";
    let fake = Fake::default();
    let mut facts = fake.facts(source).unwrap();
    let start = source.rfind("work").unwrap();
    facts.calls_supported = true;
    facts.calls.push(CallSite {
        span: Span::new(start, start + 4),
        callee: Span::new(start, start + 4),
        kind: CallKind::Direct,
        owner: None,
    });
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/a.p", source)
                    .with_file("/ws/package", "ws"),
            ),
        ),
        Languages::new().with(fake.with_navigation_facts(facts)),
    );
    let reply = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::Callers,
    )
    .execute(&engine)
    .unwrap();
    let RelationshipResolution::Ambiguous { candidates } = &reply.items[0].resolution else {
        panic!("{reply:?}")
    };
    assert_eq!(candidates.len(), 2);
    assert_ne!(candidates[0].id, candidates[1].id);
}
