mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_core::{
    Facts, ImportBinding, ImportRef, ModuleDeclaration, ModuleScope, PathSyntax, Span, Symbol,
    SymbolKind,
};
use vvv_engine::{
    Engine, Languages, MemoryVfs, NavigationOutcome, NavigationQuery, Position, Retention,
    Workspace,
};

struct Fixture {
    source: String,
    facts: Facts,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let mut facts = Facts::default();
        for (start, token) in source.split_inclusive('\n').scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, line.trim_end()))
        }) {
            facts.push_token(token, "word", Span::new(start, start + token.len()));
            facts.navigation.push(Span::new(start, start + token.len()));
        }
        Self {
            source: source.into(),
            facts,
        }
    }
    fn span(&self, line: usize) -> Span {
        let start: usize = self
            .source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum();
        Span::new(start, start + self.source.lines().nth(line).unwrap().len())
    }
    fn scope(&mut self, path: &[&str], start: usize, end: usize) -> usize {
        let span = Span::new(self.span(start).start, self.span(end).end);
        self.facts.module_scopes.push(ModuleScope {
            path: path.iter().map(|name| (*name).into()).collect(),
            span,
            declaration: None,
            declarations: vec![],
            imports: vec![],
        });
        self.facts.module_scopes.len() - 1
    }
    fn declaration(&mut self, scope: usize, line: usize) {
        let span = self.span(line);
        self.facts.symbols.push(Symbol::plain(
            SymbolKind::Function,
            self.source.lines().nth(line).unwrap(),
            span,
            span,
        ));
        self.facts.module_scopes[scope]
            .declarations
            .push(ModuleDeclaration {
                name_span: span,
                restriction: None,
            });
    }
    fn import(&mut self, scope: usize, line: usize, path: &str, alias: &str) {
        let span = self.span(line);
        self.facts.imports.push(ImportRef {
            span,
            path: PathSyntax::Scoped.parse(path),
            alias: Some(alias.into()),
            glob: false,
            declares: true,
            reexport: false,
            group: None,
        });
        self.facts.module_scopes[scope].imports.push(ImportBinding {
            span,
            visibility: None,
            restriction: None,
        });
    }
    fn engine(&self) -> Engine {
        const SEMANTICS: vvv_core::Semantics = vvv_core::Semantics {
            import_scopes_names: false,
            addressable: &[
                SymbolKind::Function,
                SymbolKind::Const,
                SymbolKind::TypeAlias,
            ],
            visibility: &[],
            default_visibility: vvv_core::ReachKind::Declaring,
        };
        Engine::new(
            Workspace::new(
                "/ws",
                Arc::new(MemoryVfs::new().with_file("/ws/a.p", &self.source)),
            ),
            Languages::new().with(
                Fake::default()
                    .with_semantics(&SEMANTICS)
                    .with_navigation_facts(self.facts.clone()),
            ),
        )
        .with_retention(Retention::session())
    }
    fn target(&self, line: u32) -> Span {
        let reply = NavigationQuery::at("a.p", Position::new(line, 0))
            .execute(&self.engine())
            .unwrap();
        let NavigationOutcome::Resolved { target, .. } = reply.outcome else {
            panic!("{reply:?}")
        };
        assert_eq!(target.declaration.path.as_path(), Path::new("a.p"));
        target.name_span
    }
}

#[test]
fn explicit_module_ownership_resolves_nested_names_without_outer_fallback() {
    let mut f = Fixture::new("Foo\ninner\nFoo\nFoo\ndeep\nFoo\nFoo\nMissing\nFoo\n");
    let root = f.scope(&[], 0, 8);
    let inner = f.scope(&["inner"], 2, 7);
    let deep = f.scope(&["inner", "deep"], 5, 6);
    f.declaration(root, 0);
    f.declaration(inner, 2);
    f.declaration(deep, 5);
    assert_eq!(f.target(3), f.span(2));
    assert_eq!(f.target(6), f.span(5));
    assert_eq!(f.target(8), f.span(0));
    let reply = NavigationQuery::at("a.p", Position::new(7, 0))
        .execute(&f.engine())
        .unwrap();
    assert!(matches!(
        reply.outcome,
        NavigationOutcome::Unavailable { .. }
    ));
}

#[test]
fn imports_are_scoped_and_super_is_relative_to_the_inline_owner() {
    let mut f = Fixture::new("Foo\ninner\nAlias\nAlias\nAlias\n");
    let root = f.scope(&[], 0, 4);
    let inner = f.scope(&["inner"], 2, 3);
    f.declaration(root, 0);
    f.import(inner, 2, "super::Foo", "Alias");
    assert_eq!(f.target(3), f.span(0));
    let reply = NavigationQuery::at("a.p", Position::new(4, 0))
        .execute(&f.engine())
        .unwrap();
    assert!(matches!(
        reply.outcome,
        NavigationOutcome::Unavailable { .. }
    ));
}

#[test]
fn competing_module_declarations_remain_selectable_and_import_cycles_terminate() {
    let mut f = Fixture::new("Foo\nFoo\nFoo\nleft\nright\nleft\n");
    let root = f.scope(&[], 0, 5);
    f.declaration(root, 0);
    f.declaration(root, 1);
    f.import(root, 3, "right", "left");
    f.import(root, 4, "left", "right");
    let engine = f.engine();
    let reply = NavigationQuery::at("a.p", Position::new(2, 0))
        .execute(&engine)
        .unwrap();
    let NavigationOutcome::Ambiguous { candidates } = reply.outcome else {
        panic!("{reply:?}")
    };
    assert_eq!(candidates.len(), 2);
    let reply = NavigationQuery::at("a.p", Position::new(5, 0))
        .execute(&engine)
        .unwrap();
    assert!(matches!(
        reply.outcome,
        NavigationOutcome::Unavailable {
            reason: vvv_engine::UnavailableReason::CyclicImports
        }
    ));
}

#[test]
fn long_acyclic_import_chains_return_a_bounded_failure() {
    let source = (0..140)
        .map(|index| format!("Alias{index}\n"))
        .collect::<String>()
        + "Alias0\n";
    let mut fixture = Fixture::new(&source);
    let root = fixture.scope(&[], 0, 140);
    for index in 0..139 {
        fixture.import(
            root,
            index,
            &format!("Alias{}", index + 1),
            &format!("Alias{index}"),
        );
    }
    fixture.declaration(root, 139);
    let error = NavigationQuery::at("a.p", Position::new(140, 0))
        .execute(&fixture.engine())
        .unwrap_err();
    assert!(matches!(error, vvv_engine::EngineError::NavigationLimit));
}

impl Fixture {
    fn local_import(&mut self, start: usize, end: usize, line: usize, path: &str, alias: &str) {
        self.facts.lexical_tokens = self.facts.navigation.clone();
        self.import(0, line, path, alias);
        let binding = self.facts.module_scopes[0].imports.pop().unwrap();
        let span = Span::new(self.span(start).start, self.span(end).end);
        if let Some(scope) = self.facts.import_scopes.iter_mut().find(|s| s.span == span) {
            scope.imports.push(binding);
        } else {
            self.facts.import_scopes.push(vvv_core::ImportScope {
                span,
                imports: vec![binding],
                aliases: vec![],
            });
        }
    }
    fn local(&mut self, start: usize, end: usize, line: usize, kind: SymbolKind, after: bool) {
        let span = self.span(line);
        self.facts.lexical.push(vvv_core::LexicalBinding {
            symbol: Symbol::plain(kind, self.source.lines().nth(line).unwrap(), span, span),
            scope: Span::new(self.span(start).start, self.span(end).end),
            excluded: vec![],
            visible_from: if after {
                span.end
            } else {
                self.span(start).start
            },
            namespace: vvv_core::BindingNamespace::Value,
            explicit: true,
        });
        self.facts.lexical_tokens = self.facts.navigation.clone();
    }
}

#[test]
fn local_imports_are_hoisted_bounded_and_restore_outer_bindings() {
    let mut f = Fixture::new("Foo\nBar\nalias\nalias\nalias\nalias\nalias\nalias\n");
    let root = f.scope(&[], 0, 7);
    f.declaration(root, 0);
    f.declaration(root, 1);
    f.local_import(2, 6, 3, "self::Foo", "alias");
    f.local_import(4, 5, 4, "self::Bar", "alias");
    assert_eq!(f.target(2), f.span(0));
    assert_eq!(f.target(5), f.span(1));
    assert_eq!(f.target(6), f.span(0));
    let reply = NavigationQuery::at("a.p", Position::new(7, 0))
        .execute(&f.engine())
        .unwrap();
    assert!(matches!(
        reply.outcome,
        NavigationOutcome::Unavailable {
            reason: vvv_engine::UnavailableReason::Unresolved
        }
    ));
}

#[test]
fn local_imports_shadow_outer_locals_but_same_block_variables_shadow_imports() {
    let mut f = Fixture::new("Foo\nalias\nalias\nalias\nalias\nalias\n");
    let root = f.scope(&[], 0, 5);
    f.declaration(root, 0);
    f.local(1, 5, 1, SymbolKind::Variable, true);
    f.local_import(2, 5, 2, "self::Foo", "alias");
    assert_eq!(f.target(3), f.span(0));
    f.local(2, 5, 4, SymbolKind::Variable, true);
    assert_eq!(f.target(5), f.span(4));
}

#[test]
fn local_imports_and_peer_items_preserve_ambiguity_and_selection() {
    let mut f = Fixture::new("Foo\nBar\nalias\nalias\nalias\nalias\n");
    let root = f.scope(&[], 0, 5);
    f.declaration(root, 0);
    f.declaration(root, 1);
    f.local_import(2, 5, 2, "self::Foo", "alias");
    f.local_import(2, 5, 3, "self::Bar", "alias");
    f.local(2, 5, 4, SymbolKind::Function, false);
    let reply = NavigationQuery::at("a.p", Position::new(5, 0))
        .execute(&f.engine())
        .unwrap();
    let NavigationOutcome::Ambiguous { candidates } = reply.outcome else {
        panic!("{reply:?}")
    };
    assert_eq!(candidates.len(), 3);
    let mut query = NavigationQuery::at("a.p", Position::new(5, 0));
    query.selection = vvv_engine::Selection::ids([candidates[1].declaration.id.clone()]);
    assert!(matches!(
        query.execute(&f.engine()).unwrap().outcome,
        NavigationOutcome::Resolved { .. }
    ));
}

#[test]
fn local_import_cycles_are_guarded_without_outer_fallback() {
    let mut f = Fixture::new("first\nsecond\nfirst\n");
    f.scope(&[], 0, 2);
    f.local_import(0, 2, 0, "second", "first");
    f.local_import(0, 2, 1, "first", "second");
    let reply = NavigationQuery::at("a.p", Position::new(2, 0))
        .execute(&f.engine())
        .unwrap();
    assert!(matches!(
        reply.outcome,
        NavigationOutcome::Unavailable {
            reason: vvv_engine::UnavailableReason::CyclicImports
        }
    ));
}

#[test]
fn imported_constants_do_not_become_immutable_local_pattern_bindings() {
    let mut f = Fixture::new("UNIT\nUNIT\nUNIT\nUNIT\n");
    let root = f.scope(&[], 0, 3);
    f.declaration(root, 0);
    f.facts.symbols[0].kind = SymbolKind::Const;
    f.local_import(1, 3, 1, "self::UNIT", "UNIT");
    f.local(1, 3, 2, SymbolKind::Variable, true);
    f.facts.lexical[0].explicit = false;
    assert_eq!(f.target(2), f.span(0));
    assert_eq!(f.target(3), f.span(0));
}

#[test]
fn type_only_imports_do_not_shadow_value_parameters() {
    let mut f = Fixture::new("Alias\nvalue\nvalue\nvalue\n");
    let root = f.scope(&[], 0, 3);
    f.declaration(root, 0);
    f.facts.symbols[0].kind = SymbolKind::TypeAlias;
    f.local(1, 3, 1, SymbolKind::Parameter, false);
    f.local_import(2, 3, 2, "self::Alias", "value");
    assert_eq!(f.target(3), f.span(1));
}

#[test]
fn long_local_import_chains_are_bounded_but_proven_locals_need_no_traversal() {
    let source = (0..140)
        .map(|index| format!("Alias{index}\n"))
        .collect::<String>()
        + "Alias0\n";
    let mut f = Fixture::new(&source);
    let root = f.scope(&[], 0, 140);
    f.declaration(root, 139);
    for index in 0..139 {
        f.local_import(
            0,
            140,
            index,
            &format!("Alias{}", index + 1),
            &format!("Alias{index}"),
        );
    }
    let query = NavigationQuery::at("a.p", Position::new(140, 0));
    assert!(matches!(
        query.execute(&f.engine()),
        Err(vvv_engine::EngineError::NavigationLimit)
    ));
    f.local(0, 140, 0, SymbolKind::Variable, true);
    assert_eq!(f.target(140), f.span(0));
}
