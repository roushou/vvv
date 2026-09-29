//! Source text and declaration containment from one preview snapshot.
mod common;

use common::Fake;
use std::sync::Arc;
use vvv_engine::{
    Answer, Engine, FileQuery, Languages, MemoryVfs, Request, Span, Symbol, SymbolKind, Workspace,
};

#[test]
fn file_preview_includes_declarations_and_finds_the_nearest_enclosing_enum() {
    let text = "def outer\n    def inner\n        def variant\ndef unrelated";
    let inner_start = text.find("def inner").unwrap();
    let variant_start = text.find("def variant").unwrap();
    let end = text.find("\ndef unrelated").unwrap();
    let outer = Symbol::plain(
        SymbolKind::Enum,
        "outer",
        Span::new(4, 9),
        Span::new(0, end),
    );
    let inner = Symbol::plain(
        SymbolKind::Enum,
        "inner",
        Span::new(inner_start + 4, inner_start + 9),
        Span::new(inner_start, end),
    );
    let variant = Symbol::plain(
        SymbolKind::Variant,
        "variant",
        Span::new(variant_start + 4, end),
        Span::new(variant_start, end),
    );
    let unrelated = Symbol::plain(
        SymbolKind::Enum,
        "unrelated",
        Span::new(end + 5, text.len()),
        Span::new(end + 1, text.len()),
    );
    let symbols = vec![
        outer.clone(),
        unrelated.clone(),
        variant.clone(),
        inner.clone(),
    ];
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(MemoryVfs::new().with_file("/ws/a.p", text))),
        Languages::new().with(Fake::default().with_symbols(symbols.clone())),
    );
    let Answer::File(file) = engine
        .run(Request::File(FileQuery { path: "a.p".into() }))
        .unwrap()
        .into_answer()
    else {
        panic!("expected file preview")
    };
    assert_eq!(file.text, text);
    assert_eq!(file.symbols, symbols);
    assert_eq!(file.enclosing(variant.span, SymbolKind::Enum), Some(&inner));
    assert_eq!(file.enclosing(inner.span, SymbolKind::Enum), Some(&outer));
    assert_eq!(file.enclosing(outer.span, SymbolKind::Enum), None);
    assert_eq!(file.enclosing(unrelated.span, SymbolKind::Enum), None);
    assert_eq!(file.enclosing(variant.span, SymbolKind::Struct), None);
    assert_eq!(file.enclosing(Span::new(0, 0), SymbolKind::Enum), None);
    assert_eq!(
        file.enclosing(Span::new(0, text.len() + 1), SymbolKind::Enum),
        None
    );
    for symbol in &file.symbols {
        assert_eq!(
            &file.text[symbol.name_span.start..symbol.name_span.end],
            symbol.name
        );
    }
    assert!(!file.identifiers.is_empty());
    for anchor in &file.identifiers {
        assert_eq!(anchor.path, file.path);
        assert_eq!(anchor.content, vvv_engine::ContentId::of(text));
        assert!(!file.text[anchor.span.start..anchor.span.end].is_empty());
    }
    let value = serde_json::to_value(&file).unwrap();
    assert_eq!(value["symbols"][0]["kind"], "enum");
    assert_eq!(
        serde_json::from_value::<vvv_engine::File>(value).unwrap(),
        file
    );
}

#[test]
fn unclaimed_file_previews_omit_empty_syntax_data() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/notes.txt", "plain text")),
        ),
        Languages::new().with(Fake::default()),
    );
    let file = FileQuery {
        path: "notes.txt".into(),
    }
    .execute(&engine)
    .unwrap();
    assert!(file.symbols.is_empty());
    assert!(file.highlights.is_empty());
    assert_eq!(
        serde_json::to_value(&file).unwrap(),
        serde_json::json!({"path":"notes.txt", "text":"plain text"})
    );
}
