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
        Engine::new(
            Workspace::new(
                "/ws",
                Arc::new(MemoryVfs::new().with_file("/ws/a.p", &self.source)),
            ),
            Languages::new().with(Fake::default().with_navigation_facts(self.facts.clone())),
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
