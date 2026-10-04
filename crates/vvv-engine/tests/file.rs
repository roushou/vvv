//! Source text and declaration containment from one preview snapshot.
mod common;

use common::Fake;
use std::sync::Arc;
use vvv_engine::{
    Answer, Engine, FileQuery, Languages, MemoryVfs, Request, Span, Symbol, SymbolKind, Workspace,
};

#[test]
fn workspace_inventory_is_sorted_relative_and_includes_unclaimed_files() {
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/z.p", "def Z")
                    .with_file("/ws/docs/readme.md", "read me")
                    .with_file("/ws/a.p", "def A"),
            ),
        ),
        Languages::new().with(Fake::default()),
    );
    let Answer::WorkspaceFiles(files) = engine
        .run(Request::WorkspaceFiles(
            vvv_engine::WorkspaceFilesQuery::default(),
        ))
        .unwrap()
        .into_answer()
    else {
        panic!()
    };
    assert_eq!(
        files.paths,
        vec![
            vvv_engine::RelPath::from("a.p"),
            "docs/readme.md".into(),
            "z.p".into()
        ]
    );
    for path in &files.paths {
        assert!(!path.is_absolute());
    }
    assert_eq!(
        serde_json::to_value(&files).unwrap(),
        serde_json::json!({"paths": ["a.p", "docs/readme.md", "z.p"]})
    );
    assert_eq!(
        vvv_engine::WorkspaceFilesQuery::default()
            .execute(&engine)
            .unwrap(),
        files
    );
    assert!(Request::WorkspaceFiles(vvv_engine::WorkspaceFilesQuery::default()).is_read_only());
}

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

#[test]
fn repeated_previews_reuse_parsing_but_read_external_edits_and_deletions_even_with_trust() {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use vvv_engine::{ContentId, Retention, Vfs};
    let parses = Arc::new(AtomicUsize::new(0));
    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def Original"));
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(common::Counting::new(parses.clone())),
    )
    .with_retention(Retention::session().trusting(Duration::from_secs(3_600)));
    let read = || FileQuery { path: "a.p".into() }.execute(&engine).unwrap();
    let original = read();
    let Answer::File(repeated) = engine
        .clone()
        .run(Request::File(FileQuery { path: "a.p".into() }))
        .unwrap()
        .into_answer()
    else {
        panic!("expected file");
    };
    assert_eq!(repeated, original);
    assert_eq!(parses.load(Ordering::SeqCst), 1);
    vfs.write(Path::new("/ws/a.p"), "def Replacement").unwrap();
    let changed = read();
    assert_eq!(changed.text, "def Replacement");
    assert_eq!(changed.symbols[0].name, "Replacement");
    assert!(
        changed
            .identifiers
            .iter()
            .all(|a| a.content == ContentId::of(&changed.text))
    );
    assert_eq!(parses.load(Ordering::SeqCst), 2);
    engine.touched();
    assert_eq!(
        read(),
        changed,
        "an unchanged file still reuses validated syntax"
    );
    assert_eq!(parses.load(Ordering::SeqCst), 2);
    vfs.remove_file(Path::new("/ws/a.p")).unwrap();
    assert!(FileQuery { path: "a.p".into() }.execute(&engine).is_err());
    vfs.write(Path::new("/ws/a.p"), "def Restored").unwrap();
    assert_eq!(read().symbols[0].name, "Restored");
    assert_eq!(parses.load(Ordering::SeqCst), 3);
}
