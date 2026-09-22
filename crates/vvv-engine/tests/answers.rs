//! The read-only questions: what a file declares, where a name lives, who
//! depends on whom, what sits at a position. All answered from the same
//! facts a rename uses, with the fake language.

mod common;

use std::sync::Arc;

use common::Fake;
use vvv_core::{Address, Position};
use vvv_engine::{
    Confidence, DepsQuery, Engine, EngineError, ExplainQuery, Languages, MemoryVfs, OutlineQuery,
    Reach, ReferencesQuery, RelPath, WhereQuery, Workspace,
};

fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/lib.p", "use a/x.p\nuse b/y.p/*\ndef main\nmain x")
        .with_file("/ws/a/x.p", "def foo\nfoo foo\n\ndef bar\nbar")
        .with_file("/ws/b/y.p", "use a/x.p\nfoo")
        .with_file("/ws/notes.txt", "foo");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

#[test]
fn outline_lists_declarations_with_addresses_and_reach() {
    let outline = engine()
        .run(OutlineQuery {
            path: RelPath::from("a/x.p"),
        })
        .unwrap();
    assert_eq!(outline.path, RelPath::from("a/x.p"));
    let names: Vec<(&str, Option<&Address>, Option<&Reach>)> = outline
        .items
        .iter()
        .map(|i| (i.symbol.name.as_str(), i.address.as_ref(), i.reach.as_ref()))
        .collect();
    let module = Address::new("ws", ["a", "x.p"]);
    assert_eq!(
        names,
        [
            (
                "foo",
                Some(&module.join("foo")),
                Some(&Reach::Within(module.clone()))
            ),
            (
                "bar",
                Some(&module.join("bar")),
                Some(&Reach::Within(module.clone()))
            ),
        ]
    );
    assert_eq!(
        outline.items[1].start,
        Position::new(3, 0),
        "extent starts at the item"
    );
    assert!(matches!(
        engine().run(OutlineQuery {
            path: RelPath::from("notes.txt")
        }),
        Err(EngineError::NoLanguage(_))
    ));
}

#[test]
fn references_are_renames_evidence_without_a_plan() {
    let refs = engine().run(ReferencesQuery::new("foo")).unwrap();
    assert_eq!(refs.declarations.len(), 1);
    let by_file: Vec<(String, Confidence)> = refs
        .occurrences
        .iter()
        .map(|o| (o.m.path.display().to_string(), o.confidence))
        .collect();
    assert_eq!(
        by_file,
        [
            ("a/x.p".to_owned(), Confidence::Resolved),
            ("a/x.p".to_owned(), Confidence::Resolved),
            ("a/x.p".to_owned(), Confidence::Resolved),
            // `use a/x.p` scopes the file's name, not `foo` (Rust-like); only
            // a glob would resolve it.
            ("b/y.p".to_owned(), Confidence::Unresolved),
        ]
    );
}

#[test]
fn locate_names_the_site_and_the_import_to_write() {
    let found = engine()
        .run(WhereQuery {
            name: "foo".to_owned(),
            from: Some(RelPath::from("lib.p")),
        })
        .unwrap();
    assert_eq!(found.sites.len(), 1);
    let site = &found.sites[0];
    assert_eq!(site.declaration.path, RelPath::from("a/x.p"));
    assert_eq!(site.address, Some(Address::new("ws", ["a", "x.p", "foo"])));
    assert_eq!(site.import.as_deref(), Some("use a/x.p/foo"));
    let without_from = engine()
        .run(WhereQuery {
            name: "foo".to_owned(),
            from: None,
        })
        .unwrap();
    assert_eq!(without_from.sites[0].import, None, "no file to write it in");
}

#[test]
fn deps_go_both_ways() {
    let deps = engine()
        .run(DepsQuery {
            path: RelPath::from("a/x.p"),
        })
        .unwrap();
    assert!(deps.imports.is_empty(), "x.p imports nothing");
    let importers: Vec<(String, String)> = deps
        .importers
        .iter()
        .map(|i| (i.path.display().to_string(), i.import.path.to_string()))
        .collect();
    assert_eq!(
        importers,
        [
            ("b/y.p".to_owned(), "a/x.p".to_owned()),
            ("lib.p".to_owned(), "a/x.p".to_owned()),
        ]
    );

    let lib = engine()
        .run(DepsQuery {
            path: RelPath::from("lib.p"),
        })
        .unwrap();
    let imports: Vec<(String, Option<RelPath>)> = lib
        .imports
        .iter()
        .map(|d| (d.import.path.to_string(), d.file.clone()))
        .collect();
    assert_eq!(
        imports,
        [
            ("a/x.p".to_owned(), Some(RelPath::from("a/x.p"))),
            ("b/y.p".to_owned(), Some(RelPath::from("b/y.p"))),
        ]
    );
}

#[test]
fn explain_finds_the_enclosing_declaration_and_its_importers() {
    // Offset 12 in "def foo\nfoo foo\n\ndef bar\nbar" is the second `foo`.
    let explained = engine()
        .run(ExplainQuery {
            path: "a/x.p".into(),
            position: Position::new(1, 4),
        })
        .unwrap();
    assert_eq!(
        explained.symbol.as_ref().map(|s| s.name.as_str()),
        None,
        "a plain token has no enclosing extent"
    );
    let on_decl = engine()
        .run(ExplainQuery {
            path: "a/x.p".into(),
            position: Position::new(0, 5),
        })
        .unwrap();
    assert_eq!(
        on_decl.symbol.as_ref().map(|s| s.name.as_str()),
        Some("foo")
    );
    assert_eq!(
        on_decl.address,
        Some(Address::new("ws", ["a", "x.p", "foo"]))
    );
    assert_eq!(on_decl.module, Some(Address::new("ws", ["a", "x.p"])));
    assert_eq!(
        on_decl.importers,
        [RelPath::from("b/y.p"), RelPath::from("lib.p")]
    );
    assert!(matches!(
        engine().run(ExplainQuery {
            path: "a/x.p".into(),
            position: Position::new(40, 0),
        }),
        Err(EngineError::NoSuchPosition { .. })
    ));
}
