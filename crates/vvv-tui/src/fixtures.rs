//! Hand-built engine values for rendering tests. Deterministic: fixed ids,
//! fixed timestamps, no file system.

use std::collections::BTreeMap;
use vvv_engine::RelPath;

use vvv_engine::protocol::{FileChange, Move, Rename, Search, UnifiedDiff};
use vvv_engine::{
    Address, Edit, Intent, LanguageId, Match, MatchId, MoveIntent, Notice, NoticeKind, Occurrence,
    Position, Query, Reason, RenameIntent, Respelling, Role, Selection, Span, Symbol, SymbolKind,
};

pub fn m(path: &str, line: u32, col: u32, text: &str, source_line: &str) -> Match {
    let start = Position::new(line, col);
    Match {
        id: MatchId::from(format!("{:0>12}", format!("{line}{col}"))),
        path: RelPath::from(path),
        language: LanguageId::new("rust"),
        span: Span::new(col as usize, col as usize + text.len()),
        start,
        end: Position::new(
            line + text.lines().count() as u32 - 1,
            col + text.len() as u32,
        ),
        kind: "identifier".into(),
        text: text.into(),
        line: source_line.into(),
        captures: BTreeMap::new(),
        symbol: None,
        role: if source_line.trim_start().starts_with("use ")
            || source_line.trim_start().starts_with("pub use ")
        {
            Role::Import
        } else {
            Role::Use
        },
        address: None,
    }
}

/// The same match at byte offset `start`, so two matches in one file have
/// distinct spans.
pub fn at(mut m: Match, start: usize) -> Match {
    m.span = Span::new(start, start + m.text.len());
    m
}

pub fn decl(path: &str, line: u32, kind: SymbolKind, name: &str, source_line: &str) -> Match {
    let mut m = m(path, line, 0, source_line, source_line);
    m.kind = "trait_item".into();
    m.end = Position::new(line + 8, 1);
    m.symbol = Some(Symbol::plain(
        kind,
        name,
        Span::new(10, 10 + name.len()),
        Span::new(0, 200),
    ));
    m.role = Role::Declaration;
    // `src/lang/mod.rs` → `vvv_engine::lang`, `src/other.rs` → `vvv_engine::other`.
    let module: Vec<&str> = path
        .trim_start_matches("src/")
        .trim_end_matches("/mod.rs")
        .trim_end_matches(".rs")
        .split('/')
        .collect();
    m.address = Some(Address::new("vvv_core", module).join(name));
    m
}

pub fn search() -> Search {
    Search {
        query: Query::pattern("Language"),
        matches: vec![
            decl(
                "src/lang/mod.rs",
                63,
                SymbolKind::Trait,
                "Language",
                "pub trait Language: Send + Sync {",
            ),
            m(
                "src/lang/registry.rs",
                3,
                12,
                "Language",
                "use super::{Language, LanguageId};",
            ),
            m(
                "src/lib.rs",
                25,
                15,
                "Language",
                "pub use lang::{Language, LanguageId};",
            ),
            m("src/lib.rs", 40, 4, "Language", "    Language::new()"),
        ],
        skipped: vec![],
    }
}

pub fn change(path: &str, moved_to: Option<&str>, before: &str, after: &str) -> FileChange {
    let p = RelPath::from(path);
    let to = moved_to.map(RelPath::from);
    FileChange {
        diff: UnifiedDiff::between(&p, to.as_deref().unwrap_or(&p), before, after),
        path: p,
        moved_to: to,
        edits: vec![Edit::replace(Span::new(0, 1), "x")],
    }
}

pub fn rename(declarations: usize) -> Rename {
    let mut decls = vec![decl(
        "src/lang/mod.rs",
        63,
        SymbolKind::Trait,
        "Language",
        "pub trait Language: Send + Sync {",
    )];
    if declarations > 1 {
        decls.push(decl(
            "src/other.rs",
            2,
            SymbolKind::Struct,
            "Language",
            "pub struct Language;",
        ));
    }
    Rename {
        intent: RenameIntent::new("Language", "Lang").selecting(Selection::All),
        applied: false,
        history_id: None,
        declarations: decls,
        occurrences: vec![
            Occurrence::judged(
                m(
                    "src/lang/mod.rs",
                    63,
                    10,
                    "Language",
                    "pub trait Language: Send + Sync {",
                ),
                Reason::Declaring,
            ),
            Occurrence::judged(
                m(
                    "src/lib.rs",
                    25,
                    15,
                    "Language",
                    "pub use lang::{Language, LanguageId};",
                ),
                if declarations > 1 {
                    Reason::OtherDeclaration
                } else {
                    Reason::Imported
                },
            ),
            Occurrence::judged(
                at(
                    m(
                        "src/other.rs",
                        12,
                        9,
                        "Language",
                        "    vvv::Language::default()",
                    ),
                    300,
                ),
                Reason::ReExport,
            ),
            Occurrence::judged(
                at(
                    m("src/other.rs", 9, 4, "Language", "    Language::new()"),
                    200,
                ),
                Reason::Unresolved,
            ),
        ],
        files: vec![
            change("src/lang/mod.rs", None, "a", "b"),
            change("src/lib.rs", None, "a", "b"),
            change("src/other.rs", None, "a", "b"),
        ],
    }
}

pub fn move_file() -> Move {
    Move {
        intent: MoveIntent::new("./src/util/parse.rs", "src/net/parse.rs"),
        applied: false,
        history_id: None,
        from: "src/util/parse.rs".into(),
        to: "src/net/parse.rs".into(),
        from_address: Some(Address::new("cli", ["util", "parse"])),
        to_address: Some(Address::new("cli", ["net", "parse"])),
        respellings: vec![
            Respelling {
                path: "src/lib.rs".into(),
                span: Span::new(0, 1),
                start: Position::new(0, 4),
                from: "crate::util::parse::X".into(),
                to: "crate::net::parse::X".into(),
            },
            Respelling {
                path: "src/main.rs".into(),
                span: Span::new(0, 1),
                start: Position::new(6, 4),
                from: "crate::util::parse::{Config, X}".into(),
                to: "crate::net::parse::{Config, X}".into(),
            },
        ],
        notices: vec![
            Notice {
                path: "src/net.rs".into(),
                start: Position::new(2, 18),
                kind: NoticeKind::UnrewritableImport {
                    import: "crate::util::parse::Config".into(),
                    replacement: "crate::net::parse::Config".into(),
                },
            },
            Notice {
                path: "src/lib.rs".into(),
                start: Position::new(1, 4),
                kind: NoticeKind::Unreachable {
                    item: "net".into(),
                    from: Address::root("cli"),
                    needs: vvv_engine::ReachKind::Everyone,
                },
            },
        ],
        files: vec![
            change(
                "src/lib.rs",
                None,
                "use crate::util::parse::X;\n",
                "use crate::net::parse::X;\n",
            ),
            change(
                "src/main.rs",
                None,
                "use crate::util::parse::{Config, X};\n",
                "use crate::net::parse::{Config, X};\n",
            ),
            change("src/util/mod.rs", None, "mod parse;\nmod s;\n", "mod s;\n"),
            change(
                "src/util/parse.rs",
                Some("src/net/parse.rs"),
                "use super::s;\n",
                "use crate::util::s;\n",
            ),
        ],
    }
}

pub fn rename_intent(name: &str, to: &str) -> Intent {
    Intent::Rename(RenameIntent::new(name, to))
}
