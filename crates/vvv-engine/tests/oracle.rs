//! The oracle seam: where the scope says `?`, an oracle the host provides
//! may say what a token is — and its answer counts only as a declaration
//! the graph knows.

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::{Oracle, Referent, Span};
use vvv_engine::{
    Confidence, Engine, Languages, MemoryVfs, Reason, ReferencesQuery, RelPath, Retention,
    Workspace,
};

/// Answers from a table: (file, token span) → where the declaration is.
struct Table(HashMap<(RelPath, Span), Referent>);

impl Oracle for Table {
    fn refers(&self, file: &Path, span: Span) -> Option<Referent> {
        self.0.get(&(RelPath::from(file), span)).cloned()
    }
}

/// `foo` is declared in `a.p` and `b.p`; `c.p` names it bare, three times,
/// with nothing imported: syntax alone says `?` for each.
fn engine(oracle: Option<Table>) -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.p", "def foo")
        .with_file("/ws/b.p", "def foo")
        .with_file("/ws/c.p", "foo foo foo");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
    .with_retention(Retention::session());
    match oracle {
        Some(table) => engine.with_oracle(Arc::new(table)),
        None => engine,
    }
}

fn verdicts(engine: &Engine) -> Vec<(String, Confidence, Reason)> {
    engine
        .run(ReferencesQuery::new("foo").declared_in("a.p"))
        .unwrap()
        .occurrences
        .iter()
        .filter(|o| o.m.path == Path::new("c.p"))
        .map(|o| (o.m.span.start.to_string(), o.confidence, o.reason))
        .collect()
}

#[test]
fn without_an_oracle_syntax_has_the_last_word() {
    let got = verdicts(&engine(None));
    assert!(
        got.iter()
            .all(|(_, c, r)| *c == Confidence::Unresolved && *r == Reason::Unresolved)
    );
    assert_eq!(got.len(), 3);
}

#[test]
fn an_oracle_settles_what_syntax_could_not_and_only_that() {
    let c = RelPath::from("c.p");
    let at = |start: usize| (c.clone(), Span::new(start, start + 3));
    let declared_in = |file: &str| Referent {
        path: RelPath::from(file),
        name_span: Span::new(4, 7),
    };
    let table = Table(HashMap::from([
        // The first is a.p's foo, the second b.p's, the third points at
        // nothing the graph declares.
        (at(0), declared_in("a.p")),
        (at(4), declared_in("b.p")),
        (
            at(8),
            Referent {
                path: RelPath::from("a.p"),
                name_span: Span::new(0, 3),
            },
        ),
    ]));
    let got = verdicts(&engine(Some(table)));
    assert_eq!(
        got,
        [
            ("0".to_owned(), Confidence::Resolved, Reason::Oracle),
            ("8".to_owned(), Confidence::Unresolved, Reason::Unresolved),
            ("4".to_owned(), Confidence::Other, Reason::OracleOther),
        ],
        "sorted as a preview lists them: ✓, then ?, then ✗"
    );
}

/// The oracle is never asked about what syntax already placed.
#[test]
fn an_oracle_is_not_asked_about_the_resolved() {
    struct Loud;
    impl Oracle for Loud {
        fn refers(&self, file: &Path, span: Span) -> Option<Referent> {
            panic!("asked about {}:{:?}", file.display(), span)
        }
    }
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.p", "def foo\nfoo")
        .with_file("/ws/b.p", "use a.p/foo\nfoo");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
    .with_oracle(Arc::new(Loud));
    let refs = engine.run(ReferencesQuery::new("foo")).unwrap();
    assert!(
        refs.occurrences
            .iter()
            .all(|o| o.confidence == Confidence::Resolved)
    );
}
