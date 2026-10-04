//! Plugin evidence is checked before coordinates can authorize edits.

mod common;

use common::Fake;
use common::fixture::EngineFixture;
use vvv_core::{Facts, FactsError, RawMatch, SearchError, Span, SpanError};
use vvv_engine::{EngineError, ErrorCode, FileQuery, Query, RewriteIntent};

#[test]
fn invalid_plugin_matches_are_typed_conflicts_and_never_write() {
    for span in [Span { start: 2, end: 1 }, Span::new(1, 2), Span::new(0, 3)] {
        let fixture = EngineFixture::with_language(
            &[("a.p", "é")],
            Fake::default().with_matches(vec![RawMatch::plain(span, "word", "é")]),
        );
        let before = fixture.source_tree();
        let error = RewriteIntent::new(Query::pattern("é"), "replacement")
            .plan(&fixture.engine)
            .unwrap_err();
        assert!(matches!(
            &error,
            EngineError::Search {
                source: SearchError::Span(_),
                ..
            }
        ));
        assert_eq!(error.code(), ErrorCode::Conflict);
        assert_eq!(fixture.source_tree(), before);
    }
}

#[test]
fn invalid_plugin_facts_are_rejected_before_source_coordinates_are_read() {
    let mut facts = Facts::default();
    facts.push_token("é", "word", Span::new(1, 2));
    let fixture = EngineFixture::with_language(
        &[("a.p", "é")],
        Fake::default().with_navigation_facts(facts),
    );
    let error = FileQuery { path: "a.p".into() }
        .execute(&fixture.engine)
        .unwrap_err();
    assert!(matches!(
        &error,
        EngineError::Search {
            source: SearchError::Facts(FactsError::Span(SpanError::CharBoundary { .. })),
            ..
        }
    ));
    assert_eq!(error.code(), ErrorCode::Conflict);
}
