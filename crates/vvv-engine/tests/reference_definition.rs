//! Definition previews consume reference evidence without linking a grammar.
mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_engine::{Engine, Languages, MemoryVfs, ReferencesQuery, SymbolKind, Workspace};

#[test]
fn imported_definition_uses_placed_target_and_exact_token_evidence() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/a.p", "def Engine")
                    .with_file("/ws/b.p", "use a.p/Engine\nfield Engine")
                    .with_file("/ws/c.p", "Engine"),
            ),
        ),
        Languages::new().with(Fake::default()),
    );
    let mut references = ReferencesQuery::new("Engine").execute(&engine).unwrap();
    let target = references.declarations[0].clone();
    let mut foreign = target.clone();
    foreign.language = "another".into();
    references.declarations.push(foreign);
    for kind in [SymbolKind::Variant, SymbolKind::Impl] {
        let mut other = target.clone();
        other.symbol.as_mut().unwrap().kind = kind;
        other.address = None;
        references.declarations.push(other);
    }
    let mut resolved = 0;
    for occurrence in &references.occurrences {
        if occurrence.m.path.as_path() == Path::new("b.p") {
            assert_eq!(references.definition_of(&occurrence.m), Some(&target));
            resolved += 1;
        } else if occurrence.m.path.as_path() == Path::new("c.p") {
            assert_eq!(references.definition_of(&occurrence.m), None);
        }
    }
    assert_eq!(resolved, 2, "both the import and field type resolve");
    let mut token = references
        .occurrences
        .iter()
        .find(|o| o.m.path.as_path() == Path::new("b.p"))
        .unwrap()
        .m
        .clone();
    token.language = "another".into();
    assert_eq!(references.definition_of(&token), None);
    token.language = target.language.clone();
    token.span.start += 1;
    assert_eq!(references.definition_of(&token), None);
}

#[test]
fn definition_lookup_judges_each_same_named_target_instead_of_stopping_at_ambiguity() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/a.p", "def Engine")
                    .with_file("/ws/other.p", "def Engine")
                    .with_file(
                        "/ws/b.p",
                        "use a.p/Engine\nfield Engine\nfn(Engine) -> Engine",
                    )
                    .with_file(
                        "/ws/c.p",
                        "use other.p/Engine\nfield Engine\nfn(Engine) -> Engine",
                    )
                    .with_file("/ws/d.p", "Engine"),
            ),
        ),
        Languages::new().with(Fake::default()),
    );
    let query = ReferencesQuery::new("Engine");
    assert!(matches!(
        query.clone().execute(&engine),
        Err(vvv_engine::EngineError::AmbiguousSymbol { .. })
    ));
    let definitions = query.definitions(&engine).unwrap();
    assert_eq!(definitions.candidates.len(), 2);
    let search = vvv_engine::SearchQuery::from(vvv_engine::Query::pattern("Engine"))
        .execute(&engine)
        .unwrap();
    for (consumer, expected) in [("b.p", "a.p"), ("c.p", "other.p")] {
        let uses: Vec<_> = search
            .matches
            .iter()
            .filter(|m| m.path.as_path() == Path::new(consumer))
            .collect();
        assert_eq!(uses.len(), 4, "import, field, parameter and return type");
        for token in uses {
            let definition = definitions
                .definition_of(token)
                .expect("the import chooses a target");
            assert_eq!(definition.path.as_path(), Path::new(expected));
        }
    }
    let unknown = search
        .matches
        .iter()
        .find(|m| m.path.as_path() == Path::new("d.p"))
        .unwrap();
    assert!(definitions.definition_of(unknown).is_none());
}
