mod common;
use common::Fake;
use std::sync::Arc;
use vvv_core::{
    BindingNamespace, Facts, LexicalBinding, ScopeUncertainty, Span, Symbol, SymbolKind,
};
use vvv_engine::{
    Engine, Languages, MemoryVfs, NavigationOutcome, NavigationQuery, Position, Workspace,
};

struct Fixture {
    engine: Engine,
}
impl Fixture {
    fn new(
        kind: SymbolKind,
        namespace: BindingNamespace,
        binding_scope: Span,
        visible_from: usize,
    ) -> Self {
        Self::pattern(kind, namespace, binding_scope, visible_from, true)
    }
    fn pattern(
        kind: SymbolKind,
        namespace: BindingNamespace,
        binding_scope: Span,
        visible_from: usize,
        explicit: bool,
    ) -> Self {
        let mut facts = Facts::default();
        facts.push_token("x", "identifier", Span::new(0, 1));
        facts.lexical.push(LexicalBinding {
            symbol: Symbol::plain(kind, "x", Span::new(0, 1), Span::new(0, 1)),
            scope: binding_scope,
            excluded: vec![],
            visible_from,
            namespace,
            explicit,
        });
        for offset in [2, 6, 10] {
            let span = Span::new(offset, offset + 1);
            facts.push_token("x", "identifier", span);
            facts.lexical_tokens.push(span);
            facts.navigation.push(span);
            if namespace == BindingNamespace::Type {
                facts.navigation_types.push(span);
            }
        }
        facts.scope_uncertainties.push(ScopeUncertainty {
            scope: Span::new(1, 12),
            invocation: Span::new(4, 5),
        });
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", "x x   x   x "),
        );
        Self {
            engine: Engine::new(
                Workspace::new("/ws", vfs),
                Languages::new().with(Fake::default().with_navigation_facts(facts)),
            ),
        }
    }
    fn at(&self, offset: u32) -> NavigationOutcome {
        NavigationQuery::at("a.p", Position::new(0, offset))
            .execute(&self.engine)
            .unwrap()
            .outcome
    }
    fn resolved(&self, offset: u32) {
        assert!(matches!(
            self.at(offset),
            NavigationOutcome::Resolved { .. }
        ));
    }
    fn unsupported(&self, offset: u32) {
        assert!(matches!(
            self.at(offset),
            NavigationOutcome::Unavailable {
                reason: vvv_engine::UnavailableReason::UnsupportedContext
            }
        ));
    }
}

#[test]
fn macro_locals_can_shadow_existing_parameters_after_invocation() {
    let f = Fixture::new(
        SymbolKind::Parameter,
        BindingNamespace::Value,
        Span::new(0, 12),
        0,
    );
    f.unsupported(2);
    f.unsupported(6);
    f.unsupported(10);
}
#[test]
fn later_explicit_locals_and_inner_parameters_take_precedence() {
    let f = Fixture::new(
        SymbolKind::Variable,
        BindingNamespace::Value,
        Span::new(1, 12),
        5,
    );
    f.resolved(6);
    f.resolved(10);
    let f = Fixture::new(
        SymbolKind::Parameter,
        BindingNamespace::Value,
        Span::new(6, 12),
        6,
    );
    f.resolved(6);
    f.resolved(10);
}
#[test]
fn generated_items_can_affect_lookup_before_invocation() {
    let f = Fixture::new(
        SymbolKind::TypeParameter,
        BindingNamespace::Type,
        Span::new(0, 12),
        0,
    );
    f.unsupported(2);
    f.unsupported(6);
    let f = Fixture::new(
        SymbolKind::Function,
        BindingNamespace::Value,
        Span::new(0, 12),
        0,
    );
    f.unsupported(2);
    f.unsupported(6);
}

#[test]
fn equally_ranked_inner_items_remain_selectable_candidates() {
    let mut facts = Facts::default();
    for offset in [0, 2, 6] {
        facts.push_token("x", "identifier", Span::new(offset, offset + 1));
    }
    facts.lexical_tokens.push(Span::new(6, 7));
    for offset in [0, 2] {
        facts.lexical.push(LexicalBinding {
            symbol: Symbol::plain(
                SymbolKind::Function,
                "x",
                Span::new(offset, offset + 1),
                Span::new(offset, offset + 1),
            ),
            scope: Span::new(2, 12),
            excluded: vec![],
            visible_from: 2,
            namespace: BindingNamespace::Value,
            explicit: true,
        });
    }
    facts.scope_uncertainties.push(ScopeUncertainty {
        scope: Span::new(1, 12),
        invocation: Span::new(4, 5),
    });
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/package", "ws")
            .with_file("/ws/a.p", "x x   x     "),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default().with_navigation_facts(facts)),
    );
    let mut query = NavigationQuery::at("a.p", Position::new(0, 6));
    let NavigationOutcome::Ambiguous { candidates } =
        query.clone().execute(&engine).unwrap().outcome
    else {
        panic!("expected both inner items")
    };
    assert_eq!(candidates.len(), 2);
    query.selection =
        vvv_engine::Selection::Ids([candidates[1].declaration.id.clone()].into_iter().collect());
    let NavigationOutcome::Resolved { target, .. } = query.execute(&engine).unwrap().outcome else {
        panic!("explicit selection must resolve")
    };
    assert_eq!(target.name_span, Span::new(2, 3));
}

#[test]
fn same_block_locals_before_the_macro_override_generated_items_but_outer_locals_do_not() {
    let f = Fixture::new(
        SymbolKind::Variable,
        BindingNamespace::Value,
        Span::new(1, 12),
        1,
    );
    f.resolved(2);
    f.unsupported(6);
    let f = Fixture::new(
        SymbolKind::Variable,
        BindingNamespace::Value,
        Span::new(0, 12),
        0,
    );
    f.unsupported(2);
    f.unsupported(6);
}

#[test]
fn immutable_identifier_patterns_may_name_generated_constants() {
    let f = Fixture::pattern(
        SymbolKind::Variable,
        BindingNamespace::Value,
        Span::new(1, 12),
        5,
        false,
    );
    f.unsupported(6);
    let f = Fixture::pattern(
        SymbolKind::Parameter,
        BindingNamespace::Value,
        Span::new(6, 12),
        6,
        false,
    );
    f.unsupported(10);
}
