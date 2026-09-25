//! Hand-built protocol values for rendering tests. Deterministic: fixed ids,
//! fixed timestamps, no file system.

use std::collections::BTreeMap;
use vvv_engine::RelPath;

use vvv_engine::protocol::{
    Diff, FileChange, History, HistoryEntry, Move, Rename, Rewrite, Search, Undo,
};
use vvv_engine::{
    Address, Consumer, Dead, Dep, Deps, Explanation, Exposed, Impact, ImportRef, ImportSite,
    Importer, ImportsReport, Locations, Modifier, Outline, OutlineItem, PackageId, PathSyntax,
    Placed, Reach, References, Site, Surface, Unreferenced,
};
use vvv_engine::{
    Edit, Intent, LanguageId, Match, MatchId, MoveIntent, Notice, NoticeKind, Occurrence, Position,
    Query, Reason, RenameIntent, Respelling, RewriteIntent, Role, Selection, Span, Symbol,
    SymbolKind,
};

pub fn m(path: &str, line: u32, col: u32, text: &str, source_line: &str) -> Match {
    let start = Position::new(line, col);
    Match {
        id: MatchId::from(format!("{:0>12}", format!("{line}{col}"))),
        path: RelPath::from(path),
        language: LanguageId::new("rust"),
        span: Span::new(0, text.len()),
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
        diff: Diff::between(&p, to.as_deref().unwrap_or(&p), before, after),
        path: p,
        moved_to: to,
        edits: vec![Edit::replace(Span::new(0, 1), "x")],
    }
}

pub fn rewrite(applied: bool) -> Rewrite {
    Rewrite {
        intent: RewriteIntent::new(Query::pattern("foo($$$A)"), "bar($$$A)"),
        applied,
        history_id: applied.then_some(7),
        files: vec![
            change(
                "src/a.rs",
                None,
                "fn f() {\n    foo(1);\n}\n",
                "fn f() {\n    bar(1);\n}\n",
            ),
            change("src/b.rs", None, "foo(2);\n", "bar(2);\n"),
        ],
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
                m(
                    "src/other.rs",
                    12,
                    9,
                    "Language",
                    "    vvv::Language::default()",
                ),
                Reason::ReExport,
            ),
            Occurrence::judged(
                m("src/other.rs", 9, 4, "Language", "    Language::new()"),
                Reason::Unresolved,
            ),
        ],
        files: vec![
            change(
                "src/lang/mod.rs",
                None,
                "pub trait Language: Send + Sync {\n",
                "pub trait Lang: Send + Sync {\n",
            ),
            change(
                "src/lib.rs",
                None,
                "pub use lang::{Language, LanguageId};\n",
                "pub use lang::{Lang, LanguageId};\n",
            ),
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

pub fn move_intent(from: &str, to: &str) -> Intent {
    Intent::Move(MoveIntent::new(from, to))
}

pub fn history_item(id: u64, at: u64, intent: Intent, files: usize) -> HistoryEntry {
    HistoryEntry {
        id,
        at,
        intent,
        files,
        paths: Vec::new(),
        moves: Vec::new(),
    }
}

pub fn history() -> History {
    History {
        entries: vec![
            history_item(1, 1_000, move_intent("src/a.rs", "src/b.rs"), 3),
            history_item(2, 4_000, rename_intent("Config", "Settings"), 5),
        ],
    }
}

pub fn undo() -> Undo {
    Undo {
        undone: history_item(2, 4_000, move_intent("src/a.rs", "src/b.rs"), 3),
        restored: vec!["src/a.rs".into(), "src/lib.rs".into()],
        moves_reverted: vec![("src/a.rs".into(), "src/b.rs".into())],
    }
}

/// A search that one language could not run.
pub fn search_with_skipped() -> Search {
    let mut result = search();
    result.skipped.push(vvv_engine::Skipped {
        language: "typescript".into(),
        reason: "invalid pattern: Multiple AST nodes are detected. Please check the pattern source `fn $F($$$A) { $$$B }`.".into(),
    });
    result
}

fn module() -> Address {
    Address::new("vvv_core", ["plan"])
}

fn symbol(kind: SymbolKind, name: &str, modifier: Option<&str>) -> Symbol {
    let mut symbol = Symbol::plain(kind, name, Span::new(10, 10 + name.len()), Span::new(0, 40));
    symbol.extent = Span::new(0, 40);
    symbol.visibility = modifier.map(|text| Modifier {
        span: Span::new(0, text.len()),
        text: text.into(),
    });
    symbol
}

pub fn outline() -> Outline {
    let item = |line: u32,
                kind: SymbolKind,
                name: &str,
                modifier: Option<&str>,
                reach: Reach,
                addressable: bool,
                span: (usize, usize)| {
        let mut symbol = symbol(kind, name, modifier);
        symbol.span = Span::new(span.0, span.1);
        symbol.extent = symbol.span;
        OutlineItem {
            symbol,
            start: Position::new(line, 0),
            end: Position::new(line + 3, 1),
            address: addressable.then(|| module().join(name)),
            reach: Some(reach),
        }
    };
    Outline {
        path: RelPath::from("src/plan/mod.rs"),
        module: Some(module()),
        items: vec![
            item(
                23,
                SymbolKind::Enum,
                "ApplyError",
                Some("pub"),
                Reach::Everyone,
                true,
                (100, 300),
            ),
            item(
                25,
                SymbolKind::Variant,
                "Vfs",
                None,
                Reach::Within(module()),
                false,
                (150, 170),
            ),
            item(
                40,
                SymbolKind::Struct,
                "Plan",
                Some("pub"),
                Reach::Everyone,
                true,
                (400, 500),
            ),
            item(
                46,
                SymbolKind::Impl,
                "Plan",
                None,
                Reach::Within(module()),
                false,
                (520, 700),
            ),
            item(
                48,
                SymbolKind::Method,
                "new",
                Some("pub(crate)"),
                Reach::Package("vvv_core".into()),
                false,
                (540, 600),
            ),
            item(
                70,
                SymbolKind::Function,
                "check",
                None,
                Reach::Within(module()),
                true,
                (800, 900),
            ),
        ],
    }
}

pub fn references() -> References {
    let r = rename(1);
    References {
        name: "Language".into(),
        declarations: r.declarations,
        occurrences: r.occurrences,
    }
}

pub fn locations(with_import: bool) -> Locations {
    Locations {
        name: "Plan".into(),
        sites: vec![Site {
            declaration: decl(
                "src/plan/mod.rs",
                41,
                SymbolKind::Struct,
                "Plan",
                "pub struct Plan {",
            ),
            address: Some(module().join("Plan")),
            import: with_import.then(|| "use crate::plan::Plan;".to_owned()),
        }],
    }
}

pub fn deps() -> Deps {
    let import = |line: u32, path: &str, file: Option<&str>| {
        let (head, rest) = path.split_once("::").unwrap_or((path, ""));
        let package = if head == "crate" { "vvv_core" } else { head };
        Dep {
            import: ImportRef::new(
                Span::new(line as usize * 100, line as usize * 100 + path.len()),
                PathSyntax::Scoped.parse(path),
            ),
            start: Position::new(line, 4),
            address: Some(Address::new(
                package,
                rest.split("::").filter(|s| !s.is_empty()),
            )),
            origin: None,
            file: file.map(RelPath::from),
        }
    };
    // `use crate::edit::{ChangeSet, Edit};` — two entries, one statement.
    let grouped = |line: u32, path: &str, file: &str| {
        let mut dep = import(line, path, Some(file));
        dep.import.group = Some(vvv_engine::ImportGroup {
            prefix: PathSyntax::Scoped.parse("crate::edit"),
            item: Span::new(0, 1),
            list: Span::new(0, 1),
            items: 2,
            statement: Span::new(line as usize * 100, line as usize * 100 + 40),
            top_level: true,
        });
        dep
    };
    Deps {
        path: RelPath::from("src/plan/mod.rs"),
        module: Some(module()),
        imports: vec![
            import(2, "std::path::PathBuf", None),
            grouped(4, "crate::edit::ChangeSet", "src/edit/change_set.rs"),
            grouped(4, "crate::edit::Edit", "src/edit/mod.rs"),
            import(5, "crate::vfs::VfsError", Some("src/vfs/mod.rs")),
            import(6, "serde::Serialize", None),
            // `use crate::Span;` re-exported from the text module.
            Dep {
                origin: Some(Address::new("vvv_core", ["text", "span", "Span"])),
                ..import(7, "crate::Span", Some("src/text/span.rs"))
            },
        ],
        importers: vec![
            Importer {
                path: RelPath::from("src/lib.rs"),
                import: ImportRef::new(Span::new(0, 10), PathSyntax::Scoped.parse("plan::Plan")),
                start: Position::new(27, 8),
            },
            Importer {
                path: RelPath::from("src/workspace/mod.rs"),
                import: ImportRef::new(
                    Span::new(0, 17),
                    PathSyntax::Scoped.parse("crate::plan::Plan"),
                ),
                start: Position::new(6, 4),
            },
        ],
        skipped: vec![],
    }
}

pub fn explanation() -> Explanation {
    Explanation {
        path: RelPath::from("src/plan/mod.rs"),
        position: Position::new(48, 11),
        symbol: Some(symbol(SymbolKind::Method, "new", Some("pub(crate)"))),
        declared: Some(Position::new(48, 19)),
        line: Some("    pub(crate) fn new(change_set: ChangeSet) -> Self {".into()),
        module: Some(module()),
        address: None,
        reach: Some(Reach::Package("vvv_core".into())),
        via: vec![],
        import: None,
        importers: vec![
            RelPath::from("src/lib.rs"),
            RelPath::from("src/workspace/mod.rs"),
        ],
    }
}

/// `explain` on a `use crate::Span;` line: no declaration, but the import.
pub fn explanation_of_an_import() -> Explanation {
    Explanation {
        path: RelPath::from("src/plan/mod.rs"),
        position: Position::new(7, 12),
        symbol: None,
        declared: None,
        line: None,
        module: Some(module()),
        address: None,
        reach: None,
        via: vec![],
        import: deps().imports.pop(),
        importers: vec![],
    }
}

pub fn move_symbol() -> vvv_engine::protocol::MoveSymbol {
    vvv_engine::protocol::MoveSymbol {
        intent: vvv_engine::MoveSymbolIntent::new("Config", "src/util.rs", "src/config.rs"),
        applied: false,
        history_id: None,
        from: Address::new("cli", ["util", "Config"]),
        to: Address::new("cli", ["config", "Config"]),
        respellings: vec![Respelling {
            path: "src/main.rs".into(),
            span: Span::new(0, 1),
            start: Position::new(2, 4),
            from: "crate::util::Config".into(),
            to: "crate::config::Config".into(),
        }],
        notices: vec![Notice {
            path: "src/config.rs".into(),
            start: Position::new(0, 4),
            kind: NoticeKind::RedundantImport {
                import: "crate::util::Config".into(),
            },
        }],
        files: vec![
            change(
                "src/util.rs",
                None,
                "use std::fmt;\n\npub struct Config;\n\nfn other() {}\n",
                "use std::fmt;\n\nfn other() {}\n",
            ),
            change(
                "src/config.rs",
                None,
                "use crate::util::{Config, Other};\n",
                "use crate::util::{Config, Other};\n\npub struct Config;\n",
            ),
            change(
                "src/main.rs",
                None,
                "use crate::util::Config;\n",
                "use crate::config::Config;\n",
            ),
        ],
    }
}

fn placed(path: &str, line: u32, kind: SymbolKind, name: &str, modifier: Option<&str>) -> Placed {
    let module = Address::new("vvv_core", ["plan"]);
    Placed {
        path: RelPath::from(path),
        symbol: symbol(kind, name, modifier),
        start: Position::new(line, 4),
        address: module.join(name),
        reach: match modifier {
            Some("pub") => Reach::Everyone,
            Some("pub(crate)") => Reach::Package("vvv_core".into()),
            _ => Reach::Within(module),
        },
    }
}

pub fn surface() -> Surface {
    Surface {
        package: Some(PackageId::new("vvv_core")),
        items: vec![
            Exposed {
                declaration: placed(
                    "src/plan/mod.rs",
                    12,
                    SymbolKind::Struct,
                    "Plan",
                    Some("pub"),
                ),
                via: vec![Address::new("vvv_core", ["Plan"])],
                importers: 4,
            },
            Exposed {
                declaration: placed(
                    "src/plan/mod.rs",
                    40,
                    SymbolKind::Function,
                    "apply",
                    Some("pub"),
                ),
                via: vec![],
                importers: 0,
            },
        ],
    }
}

pub fn impact() -> Impact {
    let plan = Address::new("vvv_core", ["plan"]);
    Impact {
        name: "Plan".into(),
        address: plan.join("Plan"),
        consumers: vec![
            Consumer {
                module: Address::new("vvv_core", ["workspace"]),
                path: RelPath::from("src/workspace/mod.rs"),
                depth: 1,
                through: plan.clone(),
            },
            Consumer {
                module: Address::new("vvv_core", ["lib"]),
                path: RelPath::from("src/lib.rs"),
                depth: 1,
                through: plan,
            },
            Consumer {
                module: Address::new("vvv_core", ["engine"]),
                path: RelPath::from("src/engine.rs"),
                depth: 2,
                through: Address::new("vvv_core", ["workspace"]),
            },
        ],
    }
}

pub fn dead() -> Dead {
    Dead {
        items: vec![
            Unreferenced {
                declaration: placed("src/plan/mod.rs", 60, SymbolKind::Function, "legacy", None),
                unsure: 0,
            },
            Unreferenced {
                declaration: placed(
                    "src/plan/mod.rs",
                    72,
                    SymbolKind::Struct,
                    "Draft",
                    Some("pub(crate)"),
                ),
                unsure: 2,
            },
        ],
    }
}

pub fn imports() -> ImportsReport {
    let site = |line: u32, text: &str, address: Option<Address>| ImportSite {
        path: RelPath::from("src/plan/mod.rs"),
        import: ImportRef::new(Span::new(0, text.len()), PathSyntax::Scoped.parse(text)),
        start: Position::new(line, 4),
        address,
    };
    ImportsReport {
        path: Some(RelPath::from("src/plan/mod.rs")),
        unused: vec![site(
            3,
            "std::collections::HashMap",
            Some(Address::new("std", ["collections", "HashMap"])),
        )],
        unresolved: vec![site(5, "missing::Thing", None)],
        redundant: vec![site(
            7,
            "crate::plan::Plan",
            Some(Address::new("vvv_core", ["plan", "Plan"])),
        )],
        unplaced: vec![RelPath::from("tests/plan.rs")],
    }
}
