mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_engine::{
    ContextBudget, ContextOutcome, ContextQuery, ContextRelation, Engine, EngineError, Languages,
    MemoryVfs, NavigationQuery, Position, SourceAnchor, Span, Symbol, SymbolKind, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    source: String,
}
impl Fixture {
    fn new(tail: &str) -> Self {
        let source = format!("def Helper\ndef Root Helper {tail}");
        let symbols = vec![
            Symbol::plain(
                SymbolKind::Function,
                "Helper",
                Span::new(4, 10),
                Span::new(0, 10),
            ),
            Symbol::plain(
                SymbolKind::Function,
                "Root",
                Span::new(15, 19),
                Span::new(11, source.len()),
            ),
        ];
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", &source),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default().with_symbols(symbols)),
        );
        Self {
            engine,
            vfs,
            source,
        }
    }
    fn query(&self, root: bool) -> ContextQuery {
        ContextQuery::new(NavigationQuery::at("a.p", Position::new(u32::from(root), 4)).origin)
    }
}
#[test]
fn exact_context_contains_direct_relationships_and_deterministic_versions() {
    let f = Fixture::new("body");
    let reply = f.query(true).execute(&f.engine).unwrap();
    assert!(matches!(reply.outcome, ContextOutcome::Resolved));
    assert_eq!(reply.items.len(), 2);
    assert_eq!(reply.items[0].relation, ContextRelation::Definition);
    assert_eq!(
        reply.items[1].relation,
        ContextRelation::ReferencedDefinition
    );
    assert_eq!(reply.items[1].via.as_ref().unwrap().span, Span::new(20, 26));
    assert_eq!(reply.items[1].text, "def Helper");
    for item in &reply.items {
        assert_eq!(item.excerpt.path.as_path(), Path::new("a.p"));
        assert_eq!(
            item.text,
            f.source[item.excerpt.span.start..item.excerpt.span.end]
        );
    }
    let again = f.query(true).execute(&f.engine).unwrap();
    assert_eq!(reply, again);
}
#[test]
fn incoming_context_is_confirmed_and_explicitly_same_spelling() {
    let f = Fixture::new("body");
    let mut query = f.query(false);
    query.references = true;
    let reply = query.execute(&f.engine).unwrap();
    assert!(reply.references_by_name);
    assert!(reply.items.iter().any(
        |item| item.relation == ContextRelation::Reference && item.text.starts_with("def Root")
    ));
}
#[test]
fn json_budget_counts_escaping_and_preserves_exact_unicode_excerpts() {
    let f = Fixture::new(&"é\"\\\n🙂".repeat(1500));
    for max_bytes in [1024, 1536, 2048, 8192] {
        let mut query = f.query(true);
        query.budget.max_bytes = max_bytes;
        let reply = query.execute(&f.engine).unwrap();
        assert!(serde_json::to_vec(&reply).unwrap().len() <= max_bytes);
        assert!(reply.omissions.byte_limit > 0);
        assert!(!reply.items.is_empty());
        for item in &reply.items {
            assert_eq!(
                item.text,
                f.source[item.excerpt.span.start..item.excerpt.span.end]
            );
        }
    }
}
#[test]
fn work_and_item_limits_report_omissions_and_invalid_limits_are_errors() {
    let f = Fixture::new("one two three");
    let mut query = f.query(true);
    query.budget = ContextBudget {
        max_items: 1,
        max_lookups: 1,
        ..ContextBudget::default()
    };
    let reply = query.execute(&f.engine).unwrap();
    assert_eq!(reply.items.len(), 1);
    assert!(reply.omissions.lookup_limit > 0);
    let mut query = f.query(true);
    query.budget.max_bytes = 0;
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidBudget)
    ));
}
#[test]
fn stale_context_origins_are_rejected_instead_of_relocated_by_name() {
    let f = Fixture::new("body");
    let reply = f.query(true).execute(&f.engine).unwrap();
    let origin = SourceAnchor {
        span: reply.items[0].target.name_span,
        ..reply.items[0].target.declaration.clone()
    };
    f.vfs
        .write(Path::new("/ws/a.p"), &(f.source.clone() + "changed"))
        .unwrap();
    let query = ContextQuery::new(NavigationQuery::occurrence(origin).origin);
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::StaleSource { .. })
    ));
}

#[test]
fn ambiguity_is_never_silently_narrowed_to_fit_a_budget() {
    let source = "def Root\n".repeat(20) + "Root";
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/package", "ws")
                    .with_file("/ws/a.p", &source),
            ),
        ),
        Languages::new().with(Fake::default()),
    );
    let mut query = ContextQuery::new(NavigationQuery::at("a.p", Position::new(20, 0)).origin);
    let reply = query.clone().execute(&engine).unwrap();
    assert!(
        matches!(reply.outcome, ContextOutcome::Ambiguous { candidates } if candidates.len() == 20)
    );
    query.budget.max_bytes = 1024;
    assert!(matches!(
        query.execute(&engine),
        Err(EngineError::OutputLimit { .. })
    ));
}
