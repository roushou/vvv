//! Search against the shared fake language: no parser is linked in.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::{Position, Query};
use vvv_engine::{Engine, Languages, MemoryVfs, Workspace};

fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.p", "foo bar\nfoo")
        .with_file("/ws/b.p", "nothing")
        .with_file("/ws/c.txt", "foo but unclaimed extension");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

#[test]
fn matches_carry_relative_paths_and_positions() {
    let found = engine().run(Query::pattern("foo")).unwrap().matches;
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].path, Path::new("a.p"));
    assert_eq!(found[0].start, Position::new(0, 0));
    assert_eq!(found[1].start, Position::new(1, 0));
    assert_ne!(found[0].id, found[1].id);
}

#[test]
fn unknown_language_filter_yields_nothing() {
    let found = engine()
        .run(Query::pattern("foo").in_language("rust"))
        .unwrap();
    assert!(found.matches.is_empty() && found.skipped.is_empty());
}

/// A language whose grammar cannot compile the query is skipped and named;
/// the other languages' matches are complete and the search succeeds.
#[test]
fn a_language_that_declines_the_query_is_skipped_not_fatal() {
    use vvv_core::{Facts, Language, LanguageId, Query, RawMatch, SearchError, Semantics};

    struct Picky;
    impl Language for Picky {
        fn id(&self) -> LanguageId {
            "picky".into()
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["q"]
        }
        fn semantics(&self) -> &'static Semantics {
            Fake::default().semantics()
        }
        fn paths(&self) -> vvv_core::PathSyntax {
            Fake::default().paths()
        }
        fn facts(&self, _: &str) -> Result<Facts, SearchError> {
            Ok(Facts::default())
        }
        fn accepts(&self, query: &Query) -> Result<(), SearchError> {
            Err(SearchError::Pattern(format!(
                "picky cannot parse `{}`",
                query.pattern_str().unwrap_or_default()
            )))
        }
        fn find(&self, _: &str, _: &Query) -> Result<Vec<RawMatch>, SearchError> {
            panic!("a declining language must not be asked to search")
        }
    }

    let vfs = MemoryVfs::new()
        .with_file("/ws/a.p", "foo")
        .with_file("/ws/b.q", "foo");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()).with(Picky),
    );
    let search = engine.run(Query::pattern("foo")).unwrap();
    assert_eq!(
        search.matches.len(),
        1,
        "the fake's match; the picky file was not searched"
    );
    assert_eq!(search.skipped.len(), 1);
    assert_eq!(search.skipped[0].language, "picky".into());
    assert!(search.skipped[0].reason.contains("cannot parse `foo`"));

    let only_fake = engine
        .run(Query::pattern("foo").in_language("fake"))
        .unwrap();
    assert!(
        only_fake.skipped.is_empty(),
        "a language filter asks nobody else"
    );
}
