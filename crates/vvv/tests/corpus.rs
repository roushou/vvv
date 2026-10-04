//! Corpus workspaces under `tests/corpus/` cover Rust, TypeScript, moves, and
//! import resolution. Command cases retain exact human and canonical JSON
//! snapshots. Complete source previews are checked once per fixture file and
//! compared across every response that refers to them.
//! Mutation cases check three properties: apply equals preview, undo restores
//! the original tree, and batch equals sequential application.
//! Changing what a command means changes a snapshot; review it, then
//! `INSTA_UPDATE=always cargo test -p vvv-rs --test corpus` to accept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use vvv_engine::{
    Answer, Apply, Edit, Engine, FileChange, Intent, MemoryVfs, MoveIntent, MoveSymbolIntent,
    Query, RenameIntent, Request, Retention, RewriteIntent, Vfs, Workspace,
};
use vvv_rs::Builtins;

#[path = "common/corpus_snapshot.rs"]
mod corpus_snapshot;
use corpus_snapshot::{JsonSnapshot, SourceSnapshot};

// Native libtest cases: each command can be listed, filtered, and rerun independently.
macro_rules! corpus_cases {
    ($module:ident, $corpus:ident; $($test:ident => $case:literal),* $(,)?) => {
        mod $module {
            use super::*;
            #[test]
            fn case_manifest_is_complete() {
                let mut actual: Vec<_> = $corpus.cases.iter().map(|(name, _)| *name).collect();
                let mut registered = vec![$($case),*];
                actual.sort_unstable();
                registered.sort_unstable();
                assert!(actual.windows(2).all(|pair| pair[0] != pair[1]), "case names must be unique");
                assert_eq!(registered, actual, "every corpus case must be registered exactly once");
            }
            $(#[test] fn $test() { $corpus.case($case); })*
        }
    };
}

macro_rules! corpus_properties {
    ($module:ident, $corpus:ident) => {
        mod $module {
            use super::*;
            #[test]
            fn apply_is_preview() {
                $corpus.apply_is_preview();
            }
            #[test]
            fn undo_is_identity() {
                $corpus.undo_is_identity();
            }
            #[test]
            fn batch_is_composition() {
                $corpus.batch_is_composition();
            }
        }
    };
}

/// One corpus: its directory and every case run against it.
struct Corpus {
    name: &'static str,
    /// `(case name, arguments)`; `--json` and the human form are both kept.
    cases: &'static [(&'static str, &'static [&'static str])],
    /// Mutations the properties hold over, as requests without `apply`.
    mutations: fn() -> Vec<Request>,
    /// Named pairs that must compose; an error fails the test.
    compositions: &'static [(&'static str, usize, usize)],
}

#[cfg(feature = "rust")]
const RUST_INLINE_MODULES: Corpus = Corpus {
    name: "rust-inline-modules",
    cases: &[
        (
            "noncapturing",
            &["navigate", "src/lib.rs:38:31", "--compact"],
        ),
        (
            "local-import",
            &["navigate", "src/lib.rs:42:21", "--compact"],
        ),
        (
            "restricted-inside",
            &["navigate", "src/lib.rs:49:47", "--compact"],
        ),
        (
            "package-visible",
            &["navigate", "src/lib.rs:51:31", "--compact"],
        ),
        (
            "parent-visible",
            &["navigate", "src/lib.rs:51:55", "--compact"],
        ),
        (
            "restricted-outside",
            &["navigate", "src/lib.rs:52:42", "--compact"],
        ),
        (
            "module-prefix",
            &["navigate", "src/lib.rs:24:19", "--compact"],
        ),
        ("inline-type", &["navigate", "src/lib.rs:6:20", "--compact"]),
        ("inline-call", &["navigate", "src/lib.rs:6:45", "--compact"]),
        (
            "nested-alias",
            &["navigate", "src/lib.rs:9:24", "--compact"],
        ),
        ("nested-call", &["navigate", "src/lib.rs:9:42", "--compact"]),
        (
            "qualified-inline",
            &["navigate", "src/lib.rs:24:27", "--compact"],
        ),
        ("reexport", &["navigate", "src/lib.rs:24:37", "--compact"]),
        (
            "private-module",
            &["navigate", "src/lib.rs:25:30", "--compact"],
        ),
        ("competing", &["navigate", "src/lib.rs:16:26", "--compact"]),
        ("cyclic", &["navigate", "src/lib.rs:20:31", "--compact"]),
        (
            "test-shadow",
            &["navigate", "src/lib.rs:31:20", "--compact"],
        ),
        (
            "test-helper",
            &["navigate", "src/lib.rs:31:28", "--compact"],
        ),
        (
            "test-nested",
            &["navigate", "src/lib.rs:34:24", "--compact"],
        ),
        (
            "disk-client",
            &["navigate", "src/disk.rs:2:18", "--compact"],
        ),
        ("disk-call", &["navigate", "src/disk.rs:2:34", "--compact"]),
        (
            "disk-qualified-alias",
            &["navigate", "src/disk.rs:3:44", "--compact"],
        ),
        (
            "inline-context",
            &["context", "src/lib.rs:31:28", "--detail", "signature"],
        ),
        (
            "inline-callers",
            &["relationships", "callers", "src/lib.rs:30:8"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_inline_modules_golden, RUST_INLINE_MODULES;
    case_noncapturing => "noncapturing",
    case_local_import => "local-import",
    case_restricted_inside => "restricted-inside",
    case_package_visible => "package-visible",
    case_parent_visible => "parent-visible",
    case_restricted_outside => "restricted-outside",
    case_module_prefix => "module-prefix",
    case_inline_type => "inline-type",
    case_inline_call => "inline-call",
    case_nested_alias => "nested-alias",
    case_nested_call => "nested-call",
    case_qualified_inline => "qualified-inline",
    case_reexport => "reexport",
    case_private_module => "private-module",
    case_competing => "competing",
    case_cyclic => "cyclic",
    case_test_shadow => "test-shadow",
    case_test_helper => "test-helper",
    case_test_nested => "test-nested",
    case_disk_client => "disk-client",
    case_disk_call => "disk-call",
    case_disk_qualified_alias => "disk-qualified-alias",
    case_inline_context => "inline-context",
    case_inline_callers => "inline-callers",
);

#[cfg(feature = "rust")]
const RUST_MACRO_BLOCKS: Corpus = Corpus {
    name: "rust-macro-blocks",
    cases: &[
        (
            "same-block-before",
            &["navigate", "src/lib.rs:58:13", "--compact"],
        ),
        (
            "outer-local-before",
            &["navigate", "src/lib.rs:63:15", "--compact"],
        ),
        (
            "inner-parameter",
            &["navigate", "src/lib.rs:67:42", "--compact"],
        ),
        (
            "identifier-pattern",
            &["navigate", "src/lib.rs:53:18", "--compact"],
        ),
        (
            "inner-ambiguity",
            &["navigate", "src/lib.rs:47:9", "--compact"],
        ),
        (
            "inner-selection",
            &["navigate", "src/lib.rs:47:9", "--compact", "--select", "2"],
        ),
        (
            "macro-argument",
            &["navigate", "src/lib.rs:7:13", "--compact"],
        ),
        (
            "expression-ambiguity",
            &["navigate", "src/lib.rs:27:61", "--compact"],
        ),
        (
            "between-macros",
            &["navigate", "src/lib.rs:31:13", "--compact"],
        ),
        (
            "after-second-macro",
            &["navigate", "src/lib.rs:33:13", "--compact"],
        ),
        (
            "custom-assert",
            &["navigate", "src/lib.rs:38:13", "--compact"],
        ),
        (
            "parameter-before",
            &["navigate", "src/lib.rs:5:22", "--compact"],
        ),
        ("item-before", &["navigate", "src/lib.rs:6:5", "--compact"]),
        ("item-after", &["navigate", "src/lib.rs:8:5", "--compact"]),
        (
            "local-before-macro",
            &["navigate", "src/lib.rs:9:13", "--compact"],
        ),
        (
            "local-after-macro",
            &["navigate", "src/lib.rs:11:13", "--compact"],
        ),
        ("rooted", &["navigate", "src/lib.rs:12:12", "--compact"]),
        (
            "inner-local",
            &["navigate", "src/lib.rs:13:34", "--compact"],
        ),
        (
            "nested-item",
            &["navigate", "src/lib.rs:14:19", "--compact"],
        ),
        (
            "expression-call",
            &["navigate", "src/lib.rs:19:5", "--compact"],
        ),
        (
            "expression-parameter",
            &["navigate", "src/lib.rs:20:13", "--compact"],
        ),
        (
            "expression-local",
            &["navigate", "src/lib.rs:21:13", "--compact"],
        ),
        (
            "rooted-context",
            &["context", "src/lib.rs:12:12", "--detail", "signature"],
        ),
        ("callers", &["relationships", "callers", "src/lib.rs:1:8"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_macro_blocks_golden, RUST_MACRO_BLOCKS;
    case_same_block_before => "same-block-before",
    case_outer_local_before => "outer-local-before",
    case_inner_parameter => "inner-parameter",
    case_identifier_pattern => "identifier-pattern",
    case_inner_ambiguity => "inner-ambiguity",
    case_inner_selection => "inner-selection",
    case_macro_argument => "macro-argument",
    case_expression_ambiguity => "expression-ambiguity",
    case_between_macros => "between-macros",
    case_after_second_macro => "after-second-macro",
    case_custom_assert => "custom-assert",
    case_parameter_before => "parameter-before",
    case_item_before => "item-before",
    case_item_after => "item-after",
    case_local_before_macro => "local-before-macro",
    case_local_after_macro => "local-after-macro",
    case_rooted => "rooted",
    case_inner_local => "inner-local",
    case_nested_item => "nested-item",
    case_expression_call => "expression-call",
    case_expression_parameter => "expression-parameter",
    case_expression_local => "expression-local",
    case_rooted_context => "rooted-context",
    case_callers => "callers",
);

/// Import resolution: same-file alias chains, grouped entries, and nested groups.
#[cfg(feature = "rust")]
const RUST_RESOLUTION: Corpus = Corpus {
    name: "rust-resolution",
    cases: &[
        ("search-preview-target", &["search", "Engine"]),
        (
            "references-preview-target",
            &["references", "Engine", "--in", "src/preview_origin.rs"],
        ),
        ("references-preview-ambiguous", &["references", "Engine"]),
        (
            "references-parent",
            &["references", "Foo", "--in", "src/a.rs"],
        ),
        (
            "references-child-module",
            &["references", "child", "--in", "src/a.rs"],
        ),
        (
            "references-leaf",
            &["references", "Child", "--in", "src/a/child.rs"],
        ),
        ("deps-chain", &["deps", "src/chained.rs"]),
        ("explain-chain", &["explain", "src/chained.rs:3:6"]),
        ("explain-grouped", &["explain", "src/consumer.rs:3:31"]),
        ("explain-nested-child", &["explain", "src/nested.rs:2:28"]),
        (
            "explain-nested-function",
            &["explain", "src/nested.rs:2:35"],
        ),
        ("explain-nested-sibling", &["explain", "src/nested.rs:2:46"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
const RUST_PARENT_ALIASES: Corpus = Corpus {
    name: "rust-parent-aliases",
    cases: &[
        ("references", &["references", "Foo", "--in", "src/a.rs"]),
        ("deps-child", &["deps", "src/parent/nested.rs"]),
        ("deps-grandchild", &["deps", "src/parent/nested/deeper.rs"]),
        ("deps-outside", &["deps", "src/outside.rs"]),
        ("deps-restricted", &["deps", "src/restricted/nested.rs"]),
        ("explain-child", &["explain", "src/parent/nested.rs:1:12"]),
        (
            "navigate-child",
            &["navigate", "src/parent/nested.rs:3:30", "--compact"],
        ),
        (
            "navigate-grandchild",
            &["navigate", "src/parent/nested/deeper.rs:2:35", "--compact"],
        ),
        (
            "navigate-ambiguous",
            &["navigate", "src/competing/nested.rs:2:28", "--compact"],
        ),
        (
            "navigate-cycle",
            &["navigate", "src/cycle_a.rs:2:29", "--compact"],
        ),
        ("rename", &["rename", "Foo", "Renamed", "--in", "src/a.rs"]),
        ("move", &["move", "src/a.rs", "src/renamed.rs"]),
    ],
    compositions: &[("rename-then-move", 0, 1)],
    mutations: || {
        vec![
            Request::Rename {
                intent: RenameIntent::new("Foo", "Renamed").declared_in("src/a.rs"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("src/a.rs", "src/renamed.rs"),
                apply: false,
            },
        ]
    },
};

#[cfg(feature = "rust")]
corpus_cases!(rust_parent_aliases_golden, RUST_PARENT_ALIASES;
    case_references => "references",
    case_deps_child => "deps-child",
    case_deps_grandchild => "deps-grandchild",
    case_deps_outside => "deps-outside",
    case_deps_restricted => "deps-restricted",
    case_explain_child => "explain-child",
    case_navigate_child => "navigate-child",
    case_navigate_grandchild => "navigate-grandchild",
    case_navigate_ambiguous => "navigate-ambiguous",
    case_navigate_cycle => "navigate-cycle",
    case_rename => "rename",
    case_move => "move",
);

#[cfg(feature = "rust")]
#[test]
fn parent_alias_visibility_and_lexical_scope_are_respected_by_navigation() {
    let (vfs, engine) = RUST_PARENT_ALIASES.engine();
    let source = vfs.read(Path::new("/ws/src/outside.rs")).unwrap();
    for (line, text) in source
        .lines()
        .enumerate()
        .filter(|(_, text)| text.contains("Foo"))
    {
        let reply = vvv_engine::NavigationQuery::at(
            "src/outside.rs",
            vvv_engine::Position::new(line as u32, text.find("Foo").unwrap() as u32),
        )
        .execute(&engine)
        .unwrap();
        if text.contains("package_visible") || text.contains("parent_visible") {
            let vvv_engine::NavigationOutcome::Resolved { target, .. } = reply.outcome else {
                panic!("{reply:?}")
            };
            assert_eq!(target.declaration.path.as_path(), Path::new("src/a.rs"));
        } else {
            assert!(
                matches!(
                    reply.outcome,
                    vvv_engine::NavigationOutcome::Unavailable {
                        reason: vvv_engine::UnavailableReason::Unresolved
                    }
                ),
                "{reply:?}"
            );
        }
    }
    let reply = vvv_engine::NavigationQuery::at(
        "src/restricted/nested.rs",
        vvv_engine::Position::new(1, 36),
    )
    .execute(&engine)
    .unwrap();
    assert!(
        matches!(
            reply.outcome,
            vvv_engine::NavigationOutcome::Resolved { .. }
        ),
        "{reply:?}"
    );
}

const RUST: Corpus = Corpus {
    name: "rust",
    cases: &[
        ("search-name", &["search", "--name", "Point"]),
        ("search-symbol", &["search", "--symbol", "enum"]),
        ("search-pattern", &["search", "Point::new($X, $Y)"]),
        ("outline", &["outline", "core/src/geometry/shape.rs"]),
        ("references", &["references", "Point"]),
        (
            "references-in",
            &["references", "area", "--in", "core/src/util.rs"],
        ),
        ("where", &["where", "Point", "--from", "app/src/cli.rs"]),
        ("deps", &["deps", "app/src/main.rs"]),
        ("deps-reexporter", &["deps", "core/src/geometry/point.rs"]),
        (
            "explain-declaration",
            &["explain", "core/src/geometry/point.rs:3:12"],
        ),
        ("explain-import", &["explain", "app/src/main.rs:4:24"]),
        ("surface", &["surface", "corpus_core"]),
        ("impact", &["impact", "Point"]),
        ("dead", &["dead"]),
        ("imports", &["imports"]),
        ("rename", &["rename", "Point", "Coord"]),
        ("rename-alias", &["rename", "Shape", "Figure"]),
        ("rename-shared-name", &["rename", "area", "size"]),
        ("rename-missing", &["rename", "Nope", "X"]),
        (
            "move-file",
            &["move", "core/src/util.rs", "core/src/helpers.rs"],
        ),
        ("move-dir", &["move", "core/src/geometry", "core/src/geo"]),
        (
            "move-symbol",
            &[
                "move",
                "core/src/util.rs",
                "core/src/geometry/mod.rs",
                "--symbol",
                "area",
            ],
        ),
        (
            "rewrite",
            &["rewrite", "Point::new($X, $Y)", "Point { x: $X, y: $Y }"],
        ),
        ("history", &["history"]),
    ],
    compositions: &[
        ("rename-independent-symbols", 0, 1),
        ("rename-then-move-file", 1, 2),
        ("move-file-then-directory", 2, 3),
        ("move-symbol-then-rewrite", 4, 5),
    ],
    mutations: || {
        vec![
            Request::Rename {
                intent: RenameIntent::new("Point", "Coord"),
                apply: false,
            },
            Request::Rename {
                intent: RenameIntent::new("area", "size").declared_in("core/src/util.rs"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("core/src/util.rs", "core/src/helpers.rs"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("core/src/geometry", "core/src/geo"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new(
                    "area",
                    "core/src/util.rs",
                    "core/src/geometry/mod.rs",
                ),
                apply: false,
            },
            Request::Rewrite {
                intent: RewriteIntent::new(
                    Query::pattern("Point::new($X, $Y)"),
                    "Point { x: $X, y: $Y }",
                ),
                apply: false,
            },
        ]
    },
};

const TS: Corpus = Corpus {
    name: "ts",
    cases: &[
        ("search-name", &["search", "--name", "Point"]),
        ("outline", &["outline", "src/geometry/shape.ts"]),
        ("references", &["references", "Point"]),
        ("where", &["where", "area", "--from", "src/app.ts"]),
        ("deps", &["deps", "src/app.ts"]),
        ("surface", &["surface"]),
        ("impact", &["impact", "area"]),
        ("dead", &["dead"]),
        ("imports", &["imports"]),
        ("rename", &["rename", "Point", "Coord"]),
        ("move-file", &["move", "src/util.ts", "src/lib/util.ts"]),
        ("move-dir", &["move", "src/geometry", "src/shapes"]),
    ],
    compositions: &[
        ("rename-then-move-file", 0, 1),
        ("move-file-then-directory", 1, 2),
    ],
    mutations: || {
        vec![
            Request::Rename {
                intent: RenameIntent::new("Point", "Coord"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("src/util.ts", "src/lib/util.ts"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("src/geometry", "src/shapes"),
                apply: false,
            },
        ]
    },
};

/// Rust forms whose move invariants need a real grammar. These cases are
/// kept separate so adding move cases cannot alter query golden outputs.
#[cfg(feature = "rust")]
const RUST_MOVES: Corpus = Corpus {
    name: "rust-moves",
    cases: &[
        (
            "move-selection-method-name",
            &[
                "move",
                "src/selection_methods.rs",
                "src/b.rs",
                "--symbol",
                "Selected",
            ],
        ),
        (
            "move-selection-module",
            &["move", "src/selection.rs", "src/b.rs", "--symbol", "nested"],
        ),
        (
            "move-selection-ambiguous",
            &[
                "move",
                "src/selection.rs",
                "src/b.rs",
                "--symbol",
                "Selected",
            ],
        ),
        (
            "move-selection-primary",
            &[
                "move",
                "src/selection.rs",
                "src/b.rs",
                "--symbol",
                "Selected",
                "--select",
                "1",
            ],
        ),
        (
            "move-selection-nested",
            &[
                "move",
                "src/selection.rs",
                "src/b.rs",
                "--symbol",
                "Selected",
                "--select",
                "2",
            ],
        ),
        (
            "move-selection-conditional",
            &[
                "move",
                "src/selection.rs",
                "src/b.rs",
                "--symbol",
                "conditional",
                "--select",
                "1",
            ],
        ),
        (
            "move-selection-qualified",
            &[
                "move",
                "src/selection.rs",
                "src/b.rs",
                "--symbol",
                "Qualified",
            ],
        ),
        (
            "move-self",
            &["move", "src/a.rs", "src/b.rs", "--symbol", "foo"],
        ),
        (
            "move-self-sibling",
            &[
                "move",
                "src/selfrefs.rs",
                "src/b.rs",
                "--symbol",
                "recursive",
            ],
        ),
        (
            "move-companion",
            &[
                "move",
                "src/companions.rs",
                "src/b.rs",
                "--symbol",
                "Bundle",
            ],
        ),
        (
            "move-symbol-alias",
            &["move", "src/a.rs", "src/b.rs", "--symbol", "Foo"],
        ),
        (
            "move-child-alias",
            &["move", "src/a/sub.rs", "src/b/sub.rs"],
        ),
        ("move-module-alias", &["move", "src/a.rs", "src/d.rs"]),
        (
            "move-provision-alias",
            &["move", "src/origin.rs", "src/b.rs", "--symbol", "moved"],
        ),
        (
            "move-cleanup-alias",
            &[
                "move",
                "src/origin.rs",
                "src/cleanup.rs",
                "--symbol",
                "needed",
            ],
        ),
    ],
    compositions: &[
        ("selected-symbol-then-function", 0, 1),
        ("recursive-then-companions", 2, 3),
        ("independent-import-cleanups", 7, 8),
    ],
    mutations: || {
        vec![
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("Selected", "src/selection.rs", "src/b.rs")
                    .selecting(vvv_engine::Selection::ordinals([1])),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("foo", "src/a.rs", "src/b.rs"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("recursive", "src/selfrefs.rs", "src/b.rs"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("Bundle", "src/companions.rs", "src/b.rs"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("Foo", "src/a.rs", "src/b.rs"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("src/a/sub.rs", "src/b/sub.rs"),
                apply: false,
            },
            Request::Move {
                intent: MoveIntent::new("src/a.rs", "src/d.rs"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("moved", "src/origin.rs", "src/b.rs"),
                apply: false,
            },
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new("needed", "src/origin.rs", "src/cleanup.rs"),
                apply: false,
            },
        ]
    },
};

corpus_properties!(rust_mutations, RUST);
corpus_properties!(typescript_mutations, TS);
#[cfg(feature = "rust")]
corpus_properties!(rust_move_mutations, RUST_MOVES);
#[cfg(feature = "rust")]
corpus_properties!(rust_parent_alias_mutations, RUST_PARENT_ALIASES);
#[cfg(feature = "typescript")]
corpus_properties!(
    typescript_symbol_move_mutations,
    CASES_TYPESCRIPT_SYMBOL_MOVES_GOLDEN_AND_MUTATION_PROPERTIES
);

impl Corpus {
    fn dir(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus")
            .join(self.name)
    }

    /// The binary's output for `args`, stdout then stderr, with the exit code.
    /// JSON values are canonicalized; human text is exact. Paths use `/`.
    fn run(&self, args: &[&str], json: bool) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vvv"));
        command
            .arg("-C")
            .arg(self.dir())
            .arg("--color")
            .arg("never");
        if json {
            command.arg("--json");
        }
        let output = command.args(args).output().expect("vvv runs");
        #[cfg(feature = "schemas")]
        if json {
            Self::validate_response(args, &output.stdout);
        }
        // Windows spells its path separator as `\`, which JSON escapes as
        // two characters; the snapshots are taken with `/`.
        let text = |bytes: Vec<u8>| {
            let text = String::from_utf8(bytes).unwrap();
            // Session handles are opaque; a CLI process cannot retain them for another invocation.
            let text = if json {
                serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|value| value["result"]["next_cursor"].as_str().map(str::to_owned))
                    .map_or(text.clone(), |cursor| text.replace(&cursor, "<cursor>"))
            } else {
                text
            };

            // Platform support is checked by MCP/engine tests; keep this corpus portable.
            let text = if json && args[0] == "discover" {
                text.replace(
                    "\"validation_available\": true",
                    "\"validation_available\": \"<platform>\"",
                )
                .replace(
                    "\"validation_available\": false",
                    "\"validation_available\": \"<platform>\"",
                )
            } else {
                text
            };
            if !cfg!(windows) {
                text
            } else if json {
                text.replace("\\\\", "/")
            } else {
                text.replace('\\', "/")
            }
        };
        let stdout = text(output.stdout);
        let stdout = if json {
            self.json_snapshot(&stdout)
        } else {
            stdout
        };
        format!(
            "exit {}\n--- stdout ---\n{}--- stderr ---\n{}",
            output.status.code().unwrap_or(-1),
            stdout,
            text(output.stderr)
        )
    }

    #[cfg(feature = "schemas")]
    fn validate_response(args: &[&str], bytes: &[u8]) {
        static VALIDATORS: std::sync::OnceLock<
            BTreeMap<vvv_engine::Command, jsonschema::Validator>,
        > = std::sync::OnceLock::new();
        let validators = VALIDATORS.get_or_init(|| {
            vvv_engine::Command::ALL
                .iter()
                .map(|&command| {
                    let schema = vvv_engine::SchemaQuery {
                        for_command: Some(command),
                        contract: vvv_engine::SchemaContract::Response,
                    }
                    .execute()
                    .unwrap();
                    (
                        command,
                        jsonschema::validator_for(&serde_json::Value::Object(schema.document))
                            .unwrap(),
                    )
                })
                .collect()
        });
        let command = if args[0] == "navigate" && args.contains(&"--compact") {
            vvv_engine::Command::Resolve
        } else if args[0] == "move" && args.contains(&"--symbol") {
            vvv_engine::Command::MoveSymbol
        } else {
            args[0].parse().unwrap()
        };
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let errors: Vec<_> = validators[&command]
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "schema mismatch for {args:?}: {errors:?}"
        );
    }

    /// The corpus as an in-memory tree, so a property can write to it.
    fn memory(&self) -> Arc<MemoryVfs> {
        let mut vfs = MemoryVfs::new();
        let root = self.dir();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path
                        .file_name()
                        .is_some_and(|n| n == "target" || n == ".vvv")
                    {
                        continue;
                    }
                    pending.push(path);
                } else if let Ok(text) = std::fs::read_to_string(&path) {
                    let relative = path.strip_prefix(&root).unwrap();
                    let spelled = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    vfs = vfs.with_file(format!("/ws/{spelled}"), text);
                }
            }
        }
        Arc::new(vfs)
    }

    fn engine(&self) -> (Arc<MemoryVfs>, Engine) {
        let vfs = self.memory();
        let engine = Engine::new(Workspace::new("/ws", vfs.clone()), Builtins::registry())
            .with_retention(Retention::session());
        (vfs, engine)
    }
}

/// Every file of a tree as text, by absolute path — the engine's own
/// ledger under `.vvv/` aside, which is not the tree.
fn snapshot(vfs: &MemoryVfs) -> BTreeMap<PathBuf, String> {
    vfs.walk(Path::new("/ws"))
        .unwrap()
        .into_iter()
        .filter(|path| !path.starts_with("/ws/.vvv"))
        .map(|path| {
            let text = vfs.read(&path).unwrap();
            (path, text)
        })
        .collect()
}

/// The preview's edit sequence interpreted independently of the engine.
struct Edited<'a> {
    text: &'a str,
    edits: &'a [Edit],
}

impl Edited<'_> {
    fn apply(&self) -> String {
        let mut edits: Vec<_> = self.edits.iter().enumerate().collect();
        // Applying backwards also reverses coincident insertions, so their
        // final text preserves the order carried by the preview.
        edits.sort_by_key(|(index, edit)| std::cmp::Reverse((edit.span.start, *index)));
        let mut out = self.text.to_owned();
        for (_, edit) in edits {
            out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
        }
        out
    }
}

fn files(answer: &Answer) -> &[FileChange] {
    match answer {
        Answer::Rewrite(r) => &r.files,
        Answer::Rename(r) => &r.files,
        Answer::Move(m) => &m.files,
        Answer::MoveSymbol(m) => &m.files,
        Answer::Batch(b) => &b.files,
        other => panic!("not a mutation: {other:?}"),
    }
}

fn applying(request: &Request) -> Request {
    match request.clone() {
        Request::Rewrite { intent, .. } => Request::Rewrite {
            intent,
            apply: true,
        },
        Request::Rename { intent, .. } => Request::Rename {
            intent,
            apply: true,
        },
        Request::Move { intent, .. } => Request::Move {
            intent,
            apply: true,
        },
        Request::MoveSymbol { intent, .. } => Request::MoveSymbol {
            intent,
            apply: true,
        },
        Request::Batch { intent, .. } => Request::Batch {
            intent,
            apply: true,
        },
        other => panic!("not a mutation: {other:?}"),
    }
}

fn intent_of(request: &Request) -> Intent {
    match request.clone() {
        Request::Rewrite { intent, .. } => intent.into(),
        Request::Rename { intent, .. } => intent.into(),
        Request::Move { intent, .. } => intent.into(),
        Request::MoveSymbol { intent, .. } => intent.into(),
        other => panic!("not a batchable mutation: {other:?}"),
    }
}

impl Corpus {
    fn json_snapshot(&self, output: &str) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(output).expect("CLI produces a JSON envelope");
        if let Some(source) = value
            .pointer_mut("/result/preview/source")
            .filter(|source| source.is_object())
        {
            *source = SourceSnapshot::new(self.name, source.take()).reference();
        }
        value.sort_all_objects();
        format!("{}\n", JsonSnapshot(&value))
    }

    fn case(&self, name: &str) {
        let (_, args) = self
            .cases
            .iter()
            .find(|(case, _)| *case == name)
            .expect("registered corpus case");
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/snapshots"),
        );
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(format!("{}__{name}__json", self.name), self.run(args, true));
            insta::assert_snapshot!(
                format!("{}__{name}__human", self.name),
                self.run(args, false)
            );
        });
    }
}

/// What is applied is exactly what was previewed: each previewed file, with
/// its edits made, is what the tree holds afterwards, at the path the
/// preview said; nothing else changed.
impl Corpus {
    fn apply_is_preview(&self) {
        for request in (self.mutations)() {
            let (vfs, engine) = self.engine();
            let before = snapshot(&vfs);
            let planned = engine.run(request.clone()).unwrap().into_preview().unwrap();
            let preview: Answer = (*planned).clone().into();
            assert!(
                !files(&preview).is_empty(),
                "{request:?}: previews something"
            );
            assert_eq!(
                snapshot(&vfs),
                before,
                "{request:?}: a preview writes nothing"
            );
            let mut expected = before.clone();
            for change in files(&preview) {
                let from = Path::new("/ws").join(&change.path);
                let original = before.get(&from).cloned().unwrap_or_default();
                let to = change
                    .moved_to
                    .as_ref()
                    .map_or(from.clone(), |p| Path::new("/ws").join(p));
                if to != from {
                    expected.remove(&from);
                }
                expected.insert(
                    to,
                    Edited {
                        text: &original,
                        edits: &change.edits,
                    }
                    .apply(),
                );
            }
            Apply(planned).apply(&engine).unwrap();
            assert_eq!(
                snapshot(&vfs),
                expected,
                "{request:?}: apply equals the complete previewed tree"
            );
        }
    }
}

/// An apply undone leaves every file as it was.
impl Corpus {
    fn undo_is_identity(&self) {
        for request in (self.mutations)() {
            let (vfs, engine) = self.engine();
            let before = snapshot(&vfs);
            engine.run(applying(&request)).unwrap();
            assert_ne!(
                snapshot(&vfs),
                before,
                "{request:?}: an apply changes something"
            );
            engine.run(Request::Undo).unwrap();
            assert_eq!(
                snapshot(&vfs),
                before,
                "{request:?}: undo restores the tree"
            );
        }
    }
}

/// A batch of two applied is the second applied after the first, and the
/// one undo reverts both.
impl Corpus {
    fn batch_is_composition(&self) {
        let mutations = (self.mutations)();
        assert!(
            !self.compositions.is_empty(),
            "declare at least one composition"
        );
        for &(name, first, second) in self.compositions {
            let (first, second) = (&mutations[first], &mutations[second]);
            let (vfs, engine) = self.engine();
            let before = snapshot(&vfs);
            engine
                .run(applying(first))
                .unwrap_or_else(|error| panic!("{name}: first: {error}"));
            engine
                .run(applying(second))
                .unwrap_or_else(|error| panic!("{name}: second: {error}"));
            let composed = snapshot(&vfs);
            let (vfs, engine) = self.engine();
            let batch = Request::Batch {
                intent: vvv_engine::BatchIntent::new([intent_of(first), intent_of(second)]),
                apply: true,
            };
            engine
                .run(batch)
                .unwrap_or_else(|error| panic!("{name}: batch: {error}"));
            assert_eq!(
                snapshot(&vfs),
                composed,
                "{name}: batch equals sequential apply"
            );
            engine.run(Request::Undo).unwrap();
            assert_eq!(snapshot(&vfs), before, "{name}: one undo reverts the batch");
        }
    }
}

corpus_cases!(rust_golden, RUST;
    case_search_name => "search-name",
    case_search_symbol => "search-symbol",
    case_search_pattern => "search-pattern",
    case_outline => "outline",
    case_references => "references",
    case_references_in => "references-in",
    case_where => "where",
    case_deps => "deps",
    case_deps_reexporter => "deps-reexporter",
    case_explain_declaration => "explain-declaration",
    case_explain_import => "explain-import",
    case_surface => "surface",
    case_impact => "impact",
    case_dead => "dead",
    case_imports => "imports",
    case_rename => "rename",
    case_rename_alias => "rename-alias",
    case_rename_shared_name => "rename-shared-name",
    case_rename_missing => "rename-missing",
    case_move_file => "move-file",
    case_move_dir => "move-dir",
    case_move_symbol => "move-symbol",
    case_rewrite => "rewrite",
    case_history => "history",
);

corpus_cases!(ts_golden, TS;
    case_search_name => "search-name",
    case_outline => "outline",
    case_references => "references",
    case_where => "where",
    case_deps => "deps",
    case_surface => "surface",
    case_impact => "impact",
    case_dead => "dead",
    case_imports => "imports",
    case_rename => "rename",
    case_move_file => "move-file",
    case_move_dir => "move-dir",
);

#[cfg(feature = "rust")]
#[test]
fn symbol_moves_preserve_references_through_module_aliases() {
    use vvv_engine::{Confidence, ReferencesQuery};

    let (vfs, engine) = RUST_MOVES.engine();
    let before = ReferencesQuery::new("Foo")
        .declared_in("src/a.rs")
        .execute(&engine)
        .unwrap();
    assert!(before.occurrences.iter().any(|occurrence| {
        occurrence.m.path == Path::new("src/c.rs") && occurrence.confidence == Confidence::Resolved
    }));

    let planned = MoveSymbolIntent::new("Foo", "src/a.rs", "src/b.rs")
        .plan(&engine)
        .unwrap();
    vvv_engine::Apply(planned).apply(&engine).unwrap();
    let after = ReferencesQuery::new("Foo")
        .declared_in("src/b.rs")
        .execute(&engine)
        .unwrap();
    let consumers: Vec<_> = after
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.m.path == Path::new("src/c.rs"))
        .collect();
    assert!(
        !consumers.is_empty(),
        "the consumer must retain its reference"
    );
    assert!(
        consumers
            .iter()
            .all(|occurrence| occurrence.confidence == Confidence::Resolved),
        "the module-alias consumer must resolve to the moved declaration: {consumers:?}"
    );
    assert!(
        !vfs.read(Path::new("/ws/src/a.rs"))
            .unwrap()
            .contains("struct Foo")
    );
    assert!(
        vfs.read(Path::new("/ws/src/b.rs"))
            .unwrap()
            .contains("pub struct Foo;")
    );
}

#[cfg(feature = "rust")]
#[test]
fn symbol_moves_preserve_self_references() {
    use vvv_engine::{Confidence, ReferencesQuery};

    let (vfs, engine) = RUST_MOVES.engine();
    let planned = MoveSymbolIntent::new("foo", "src/a.rs", "src/b.rs")
        .plan(&engine)
        .expect("a self-reference must be movable without conflicting edits");
    vvv_engine::Apply(planned).apply(&engine).unwrap();

    let references = ReferencesQuery::new("foo")
        .declared_in("src/b.rs")
        .execute(&engine)
        .unwrap();
    let moved: Vec<_> = references
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.m.path == Path::new("src/b.rs"))
        .collect();
    assert_eq!(
        moved.len(),
        2,
        "the declaration and its self-reference must travel together"
    );
    assert!(
        moved
            .iter()
            .all(|occurrence| occurrence.confidence == Confidence::Resolved)
    );
    let source = vfs.read(Path::new("/ws/src/a.rs")).unwrap();
    assert!(!source.contains("fn foo"));
    assert!(
        source.contains("pub struct Foo;"),
        "the sibling declaration must stay behind"
    );
}

#[cfg(feature = "rust")]
corpus_cases!(rust_moves_golden, RUST_MOVES;
    case_move_selection_method_name => "move-selection-method-name",
    case_move_selection_module => "move-selection-module",
    case_move_selection_ambiguous => "move-selection-ambiguous",
    case_move_selection_primary => "move-selection-primary",
    case_move_selection_nested => "move-selection-nested",
    case_move_selection_conditional => "move-selection-conditional",
    case_move_selection_qualified => "move-selection-qualified",
    case_move_self => "move-self",
    case_move_self_sibling => "move-self-sibling",
    case_move_companion => "move-companion",
    case_move_symbol_alias => "move-symbol-alias",
    case_move_child_alias => "move-child-alias",
    case_move_module_alias => "move-module-alias",
    case_move_provision_alias => "move-provision-alias",
    case_move_cleanup_alias => "move-cleanup-alias",
);

#[cfg(feature = "rust")]
#[test]
fn file_moves_preserve_references_through_module_aliases() {
    use vvv_engine::{Confidence, ReferencesQuery};
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveIntent::new("src/a/sub.rs", "src/b/sub.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let after = ReferencesQuery::new("Nested")
        .declared_in("src/b/sub.rs")
        .execute(&engine)
        .unwrap();
    assert!(
        after
            .occurrences
            .iter()
            .any(|o| o.m.path == Path::new("src/c.rs") && o.confidence == Confidence::Resolved)
    );
    assert!(
        vfs.read(Path::new("/ws/src/c.rs"))
            .unwrap()
            .contains("crate::b::sub::Nested")
    );
}

#[cfg(feature = "rust")]
#[test]
fn module_moves_keep_alias_spellings_when_the_binding_is_rebased() {
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveIntent::new("src/a.rs", "src/d.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let consumer = vfs.read(Path::new("/ws/src/c.rs")).unwrap();
    assert!(consumer.contains("use crate::d as alias;"));
    assert!(consumer.contains("alias::Foo"));
    assert!(consumer.contains("alias::sub::Nested"));
}

#[cfg(feature = "rust")]
#[test]
fn moved_declarations_provision_imports_from_resolved_edges() {
    use vvv_engine::{Confidence, ReferencesQuery};
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveSymbolIntent::new("moved", "src/origin.rs", "src/b.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let dest = vfs.read(Path::new("/ws/src/b.rs")).unwrap();
    assert!(dest.contains("use crate::dependency::Dep;"));
    assert!(!dest.contains("use alias::Dep;"));
    let after = ReferencesQuery::new("Dep")
        .declared_in("src/dependency.rs")
        .execute(&engine)
        .unwrap();
    assert!(
        after
            .occurrences
            .iter()
            .any(|o| o.m.path == Path::new("src/b.rs") && o.confidence == Confidence::Resolved)
    );
}

#[cfg(feature = "rust")]
#[test]
fn destination_cleanup_recognizes_imports_through_module_aliases() {
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveSymbolIntent::new("needed", "src/origin.rs", "src/cleanup.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let dest = vfs.read(Path::new("/ws/src/cleanup.rs")).unwrap();
    assert!(!dest.contains("use alias::needed;"));
    assert!(dest.contains("pub fn needed() {}"));
    assert!(dest.contains("needed();"));
}

#[test]
fn preview_insertions_keep_their_wire_order() {
    let edits = [Edit::insert(0, "import\n"), Edit::insert(0, "item\n")];
    assert_eq!(
        Edited {
            text: "",
            edits: &edits
        }
        .apply(),
        "import\nitem\n"
    );
}

#[cfg(feature = "rust")]
#[test]
fn grouped_alias_prefixes_keep_their_resolved_meaning() {
    use vvv_engine::{Confidence, ReferencesQuery};
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveIntent::new("src/a/sub.rs", "src/b/sub.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let text = vfs.read(Path::new("/ws/src/grouped.rs")).unwrap();
    assert!(
        text.contains("use root::{b::sub::Nested, a::Foo};"),
        "{text}"
    );
    let references = ReferencesQuery::new("Nested")
        .declared_in("src/b/sub.rs")
        .execute(&engine)
        .unwrap();
    assert!(
        references.occurrences.iter().any(
            |o| o.m.path == Path::new("src/grouped.rs") && o.confidence == Confidence::Resolved
        )
    );
}

#[cfg(feature = "rust")]
#[test]
fn moving_and_staying_paths_use_their_own_render_contexts() {
    use vvv_engine::{Confidence, ReferencesQuery};
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveSymbolIntent::new("recursive", "src/selfrefs.rs", "src/b.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let moved = vfs.read(Path::new("/ws/src/b.rs")).unwrap();
    assert!(moved.contains("self::recursive();"), "{moved}");
    assert!(moved.contains("crate::selfrefs::sibling();"), "{moved}");
    let source = vfs.read(Path::new("/ws/src/selfrefs.rs")).unwrap();
    assert!(source.contains("pub fn sibling() {}"));
    assert!(
        source.contains("pub fn outside() { crate::b::recursive(); }"),
        "{source}"
    );
    let references = ReferencesQuery::new("recursive")
        .declared_in("src/b.rs")
        .execute(&engine)
        .unwrap();
    assert!(
        references
            .occurrences
            .iter()
            .all(|o| o.confidence == Confidence::Resolved),
        "{:?}",
        references.occurrences
    );
}

#[cfg(feature = "rust")]
#[test]
fn companion_paths_travel_with_the_declaration_and_keep_their_targets() {
    use vvv_engine::{Confidence, ReferencesQuery};
    let (vfs, engine) = RUST_MOVES.engine();
    vvv_engine::Apply(
        MoveSymbolIntent::new("Bundle", "src/companions.rs", "src/b.rs")
            .plan(&engine)
            .unwrap(),
    )
    .apply(&engine)
    .unwrap();
    let moved = vfs.read(Path::new("/ws/src/b.rs")).unwrap();
    assert!(moved.contains("pub struct Bundle;"));
    assert!(moved.contains("impl Bundle"));
    assert!(moved.contains("-> self::Bundle"));
    assert!(moved.contains("crate::companions::helper();"));
    let source = vfs.read(Path::new("/ws/src/companions.rs")).unwrap();
    assert_eq!(source.trim(), "pub fn helper() {}");
    let references = ReferencesQuery::new("Bundle")
        .declared_in("src/b.rs")
        .execute(&engine)
        .unwrap();
    assert!(references.occurrences.len() >= 3);
    assert!(
        references
            .occurrences
            .iter()
            .all(|o| o.confidence == Confidence::Resolved),
        "{:?}",
        references.occurrences
    );
}

#[cfg(feature = "rust")]
struct ResolutionFixture {
    engine: Engine,
}

#[cfg(feature = "rust")]
impl ResolutionFixture {
    fn new() -> Self {
        Self {
            engine: RUST_RESOLUTION.engine().1,
        }
    }

    fn import_at(&self, path: &str, position: vvv_engine::Position) -> vvv_engine::Dep {
        vvv_engine::ExplainQuery {
            path: path.into(),
            position,
        }
        .execute(&self.engine)
        .unwrap()
        .import
        .expect("the position is inside an import")
    }
}

#[cfg(feature = "rust")]
corpus_cases!(rust_resolution_golden, RUST_RESOLUTION;
    case_search_preview_target => "search-preview-target",
    case_references_preview_target => "references-preview-target",
    case_references_preview_ambiguous => "references-preview-ambiguous",
    case_references_parent => "references-parent",
    case_references_child_module => "references-child-module",
    case_references_leaf => "references-leaf",
    case_deps_chain => "deps-chain",
    case_explain_chain => "explain-chain",
    case_explain_grouped => "explain-grouped",
    case_explain_nested_child => "explain-nested-child",
    case_explain_nested_function => "explain-nested-function",
    case_explain_nested_sibling => "explain-nested-sibling",
);

#[cfg(feature = "rust")]
#[test]
fn definition_preview_follows_engine_import_and_field_despite_variant_names() {
    let fixture = ResolutionFixture::new();
    let references = vvv_engine::ReferencesQuery::new("Engine")
        .definitions(&fixture.engine)
        .unwrap();
    assert_eq!(references.candidates.len(), 2);
    let search = vvv_engine::SearchQuery::from(Query::pattern("Engine"))
        .execute(&fixture.engine)
        .unwrap();
    let mut checked = 0;
    for token in search.matches.iter().filter(|m| {
        m.path.as_path() == Path::new("src/preview_consumer.rs")
            && ([0, 3].contains(&m.start.line) || m.line.contains("pub fn roundtrip"))
    }) {
        let definition = references
            .definition_of(token)
            .expect("import and field resolve");
        assert_eq!(
            definition.path.as_path(),
            Path::new("src/preview_origin.rs")
        );
        assert_eq!(
            definition.symbol.as_ref().unwrap().kind,
            vvv_engine::SymbolKind::Struct
        );
        checked += 1;
    }
    assert_eq!(checked, 4, "import, field, parameter and return type");
    let variant_use = search
        .matches
        .iter()
        .find(|m| m.path.as_path() == Path::new("src/preview_consumer.rs") && m.start.line == 11)
        .unwrap();
    assert!(references.definition_of(variant_use).is_none());
}

#[cfg(feature = "rust")]
#[test]
fn explain_selects_the_grouped_entry_under_the_cursor() {
    let fixture = ResolutionFixture::new();
    let import = fixture.import_at("src/consumer.rs", vvv_engine::Position::new(2, 30));
    assert_eq!(import.import.path.to_string(), "module_alias::child::Child");
    assert_eq!(
        import.address,
        Some(vvv_engine::Address::new(
            "resolution_probe",
            ["a", "child", "Child"]
        ))
    );
}

#[cfg(feature = "rust")]
#[test]
fn explain_selects_exact_entries_in_nested_groups() {
    let fixture = ResolutionFixture::new();
    for (column, path, address) in [
        (
            27,
            "module_alias::child::Child",
            vec!["a", "child", "Child"],
        ),
        (
            34,
            "module_alias::child::child_fn",
            vec!["a", "child", "child_fn"],
        ),
        (45, "module_alias::Foo", vec!["a", "Foo"]),
    ] {
        let import = fixture.import_at("src/nested.rs", vvv_engine::Position::new(1, column));
        assert_eq!(import.import.path.to_string(), path, "column {column}");
        assert_eq!(
            import.address,
            Some(vvv_engine::Address::new("resolution_probe", address))
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn explain_retains_a_statement_fallback_outside_import_entry_spans() {
    let fixture = ResolutionFixture::new();
    for (column, path) in [
        (0, "module_alias::Foo"),
        (4, "module_alias"),
        (23, "module_alias::Foo"),
    ] {
        let import = fixture.import_at("src/consumer.rs", vvv_engine::Position::new(2, column));
        assert_eq!(import.import.path.to_string(), path, "column {column}");
    }
    let prefix = fixture.import_at("src/nested.rs", vvv_engine::Position::new(1, 19));
    assert_eq!(prefix.import.path.to_string(), "module_alias::child");
}

#[cfg(feature = "rust")]
#[test]
fn same_file_alias_chains_agree_across_references_deps_and_explain() {
    use vvv_engine::{Address, Confidence, DepsQuery, Position, ReferencesQuery};

    let fixture = ResolutionFixture::new();
    for (name, line) in [("Foo", 4), ("child", 2)] {
        let references = ReferencesQuery::new(name)
            .declared_in("src/a.rs")
            .execute(&fixture.engine)
            .unwrap();
        let occurrence = references
            .occurrences
            .iter()
            .find(|o| o.m.path == Path::new("src/chained.rs") && o.m.start.line == line)
            .unwrap();
        assert_eq!(
            occurrence.confidence,
            Confidence::Resolved,
            "{name} already resolves correctly"
        );
    }
    let deps = DepsQuery {
        path: "src/chained.rs".into(),
    }
    .execute(&fixture.engine)
    .unwrap();
    let leaf = deps
        .imports
        .iter()
        .find(|dep| dep.import.alias.as_deref() == Some("leaf"))
        .unwrap();
    let expected = Address::new("resolution_probe", ["a", "child"]);
    assert_eq!(
        leaf.address.as_ref(),
        Some(&expected),
        "deps must follow the same binding as references"
    );
    let explained = fixture.import_at("src/chained.rs", Position::new(2, 5));
    assert_eq!(explained.address, Some(expected));
    let references = ReferencesQuery::new("Child")
        .declared_in("src/a/child.rs")
        .execute(&fixture.engine)
        .unwrap();
    let occurrence = references
        .occurrences
        .iter()
        .find(|o| o.m.path == Path::new("src/chained.rs") && o.m.start.line == 5)
        .unwrap();
    assert_eq!(
        occurrence.confidence,
        Confidence::Resolved,
        "the next alias hop must resolve too"
    );
}

#[cfg(feature = "rust")]
#[test]
fn child_modules_follow_private_module_aliases_imported_from_their_parent() {
    use vvv_engine::{Address, Confidence, DepsQuery, Position, ReferencesQuery};

    let fixture = ResolutionFixture::new();
    let deps = DepsQuery {
        path: "src/parent_context/nested.rs".into(),
    }
    .execute(&fixture.engine)
    .unwrap();
    let expected = Address::new("resolution_probe", ["a"]);
    assert_eq!(deps.imports[0].address, Some(expected.clone()));
    assert_eq!(
        fixture
            .import_at("src/parent_context/nested.rs", Position::new(0, 10))
            .address,
        Some(expected)
    );
    let references = ReferencesQuery::new("Foo")
        .declared_in("src/a.rs")
        .execute(&fixture.engine)
        .unwrap();
    let occurrence = references
        .occurrences
        .iter()
        .find(|o| o.m.path == Path::new("src/parent_context/nested.rs"))
        .unwrap();
    assert_eq!(occurrence.confidence, Confidence::Resolved);
}

#[cfg(feature = "rust")]
const CASES_RUST_NAVIGATION_GOLDEN: Corpus = Corpus {
    name: "rust-navigation",
    cases: &[
        (
            "context-signature",
            &["context", "src/signatures.rs:3:8", "--detail", "signature"],
        ),
        (
            "compact-definition",
            &["navigate", "src/consumer.rs:5:13", "--compact"],
        ),
        (
            "compact-ambiguity",
            &["navigate", "src/ambiguous.rs:3:26", "--compact"],
        ),
        (
            "scoped-search",
            &[
                "search",
                "--name",
                "Engine",
                "--path",
                "src/origin.rs",
                "--package",
                "navigation",
            ],
        ),
        (
            "context-method-location",
            &["context", "src/consumer.rs:14:19"],
        ),
        (
            "context-method-body",
            &["context", "src/consumer.rs:14:19", "--include-enclosing"],
        ),
        (
            "context-tests",
            &[
                "context",
                "src/origin.rs:2:12",
                "--references",
                "--max-items",
                "32",
                "--max-lookups",
                "128",
            ],
        ),
        ("discovery", &["discover"]),
        #[cfg(feature = "schemas")]
        ("schema-history", &["schema", "history"]),
        #[cfg(feature = "schemas")]
        (
            "schema-invalid-target",
            &["schema", "history", "--contract", "call"],
        ),
        ("lexical-generic", &["navigate", "src/lexical.rs:2:27"]),
        ("lexical-parameter", &["navigate", "src/lexical.rs:2:47"]),
        ("lexical-initializer", &["navigate", "src/lexical.rs:4:17"]),
        ("lexical-shadow", &["navigate", "src/lexical.rs:5:30"]),
        ("lexical-outer", &["navigate", "src/lexical.rs:6:13"]),
        ("lexical-nested-item", &["navigate", "src/lexical.rs:8:46"]),
        (
            "lexical-pattern-limit",
            &["navigate", "src/lexical.rs:9:67"],
        ),
        (
            "navigate-type-namespace",
            &["navigate", "src/namespaces.rs:3:19"],
        ),
        (
            "navigate-method-parameter",
            &["navigate", "src/consumer.rs:14:34"],
        ),
        (
            "navigate-method-return",
            &["navigate", "src/consumer.rs:14:45"],
        ),
        (
            "navigate-selected",
            &["navigate", "src/ambiguous.rs:3:26", "--select", "2"],
        ),
        (
            "navigate-bad-selection",
            &["navigate", "src/ambiguous.rs:3:26", "--select", "3"],
        ),
        ("navigate-import", &["navigate", "src/consumer.rs:2:20"]),
        ("navigate-field", &["navigate", "src/consumer.rs:5:13"]),
        (
            "bounded-context",
            &["context", "src/consumer.rs:5:13", "--max-bytes", "2048"],
        ),
        (
            "context-relations",
            &[
                "context",
                "src/consumer.rs:4:12",
                "--references",
                "--max-items",
                "5",
            ],
        ),
        ("context-ambiguous", &["context", "src/ambiguous.rs:3:26"]),
        ("tuple-parameter", &["navigate", "src/bindings.rs:1:45"]),
        ("closure-parameter", &["navigate", "src/bindings.rs:2:41"]),
        ("local-annotation", &["navigate", "src/bindings.rs:5:30"]),
        ("tuple-local", &["navigate", "src/bindings.rs:3:68"]),
        ("navigate-alias", &["navigate", "src/consumer.rs:6:14"]),
        ("navigate-parameter", &["navigate", "src/consumer.rs:9:26"]),
        ("navigate-return", &["navigate", "src/consumer.rs:9:37"]),
        ("navigate-generic", &["navigate", "src/consumer.rs:10:32"]),
        ("navigate-local", &["navigate", "src/consumer.rs:11:47"]),
        ("navigate-unicode", &["navigate", "src/consumer.rs:12:31"]),
        ("navigate-variant", &["navigate", "src/origin.rs:6:5"]),
        ("navigate-ambiguous", &["navigate", "src/ambiguous.rs:3:26"]),
        ("navigate-cycle", &["navigate", "src/cyclic.rs:2:25"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_navigation_golden, CASES_RUST_NAVIGATION_GOLDEN;
    case_context_signature => "context-signature",
    case_compact_definition => "compact-definition",
    case_compact_ambiguity => "compact-ambiguity",
    case_scoped_search => "scoped-search",
    case_context_method_location => "context-method-location",
    case_context_method_body => "context-method-body",
    case_context_tests => "context-tests",
    case_discovery => "discovery",
    case_schema_history => "schema-history",
    case_schema_invalid_target => "schema-invalid-target",
    case_lexical_generic => "lexical-generic",
    case_lexical_parameter => "lexical-parameter",
    case_lexical_initializer => "lexical-initializer",
    case_lexical_shadow => "lexical-shadow",
    case_lexical_outer => "lexical-outer",
    case_lexical_nested_item => "lexical-nested-item",
    case_lexical_pattern_limit => "lexical-pattern-limit",
    case_navigate_type_namespace => "navigate-type-namespace",
    case_navigate_method_parameter => "navigate-method-parameter",
    case_navigate_method_return => "navigate-method-return",
    case_navigate_selected => "navigate-selected",
    case_navigate_bad_selection => "navigate-bad-selection",
    case_navigate_import => "navigate-import",
    case_navigate_field => "navigate-field",
    case_bounded_context => "bounded-context",
    case_context_relations => "context-relations",
    case_context_ambiguous => "context-ambiguous",
    case_tuple_parameter => "tuple-parameter",
    case_closure_parameter => "closure-parameter",
    case_local_annotation => "local-annotation",
    case_tuple_local => "tuple-local",
    case_navigate_alias => "navigate-alias",
    case_navigate_parameter => "navigate-parameter",
    case_navigate_return => "navigate-return",
    case_navigate_generic => "navigate-generic",
    case_navigate_local => "navigate-local",
    case_navigate_unicode => "navigate-unicode",
    case_navigate_variant => "navigate-variant",
    case_navigate_ambiguous => "navigate-ambiguous",
    case_navigate_cycle => "navigate-cycle",
);

#[cfg(feature = "typescript")]
const CASES_TS_SCOPE_OWNERS_GOLDEN: Corpus = Corpus {
    name: "ts-scope-owners",
    cases: &[
        ("root-function", &["navigate", "src/module.ts:3:1"]),
        ("root-import", &["navigate", "src/module.ts:4:29"]),
        ("root-var", &["navigate", "src/module.ts:5:13"]),
        ("root-lexical", &["navigate", "src/module.ts:7:1"]),
        ("root-tdz", &["navigate", "src/module.ts:8:14"]),
        ("nested-function", &["navigate", "src/module.ts:11:3"]),
        ("overloads", &["navigate", "src/module.ts:15:3"]),
        ("static-var", &["navigate", "src/module.ts:22:5"]),
        ("static-lexical", &["navigate", "src/module.ts:25:5"]),
        ("static-isolation", &["navigate", "src/module.ts:27:21"]),
        ("namespace-function", &["navigate", "src/module.ts:30:3"]),
        ("namespace-var", &["navigate", "src/module.ts:33:3"]),
        ("assignment-object", &["navigate", "src/module.ts:37:17"]),
        ("assignment-array", &["navigate", "src/module.ts:38:9"]),
        ("local-interface", &["navigate", "src/module.ts:41:14"]),
        ("class-self", &["navigate", "src/module.ts:44:27"]),
        ("switch-function", &["navigate", "src/module.ts:49:5"]),
        ("switch-lexical", &["navigate", "src/module.ts:53:5"]),
        ("switch-same-case", &["navigate", "src/module.ts:51:27"]),
        (
            "wrapped-signature",
            &["context", "src/module.ts:55:7", "--detail", "signature"],
        ),
        ("enum-member", &["navigate", "src/module.ts:57:34"]),
        ("enum-forward", &["navigate", "src/module.ts:57:49"]),
        ("enum-outer", &["navigate", "src/module.ts:58:1"]),
        ("script-legacy", &["navigate", "src/script.ts:3:3"]),
        ("script-strict", &["navigate", "src/script.ts:7:5"]),
        ("script-root", &["navigate", "src/script.ts:9:1"]),
        ("script-redeclarations", &["navigate", "src/script.ts:13:1"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_scope_owners_golden, CASES_TS_SCOPE_OWNERS_GOLDEN;
    case_root_function => "root-function",
    case_root_import => "root-import",
    case_root_var => "root-var",
    case_root_lexical => "root-lexical",
    case_root_tdz => "root-tdz",
    case_nested_function => "nested-function",
    case_overloads => "overloads",
    case_static_var => "static-var",
    case_static_lexical => "static-lexical",
    case_static_isolation => "static-isolation",
    case_namespace_function => "namespace-function",
    case_namespace_var => "namespace-var",
    case_assignment_object => "assignment-object",
    case_assignment_array => "assignment-array",
    case_local_interface => "local-interface",
    case_class_self => "class-self",
    case_switch_function => "switch-function",
    case_switch_lexical => "switch-lexical",
    case_switch_same_case => "switch-same-case",
    case_wrapped_signature => "wrapped-signature",
    case_enum_member => "enum-member",
    case_enum_forward => "enum-forward",
    case_enum_outer => "enum-outer",
    case_script_legacy => "script-legacy",
    case_script_strict => "script-strict",
    case_script_root => "script-root",
    case_script_redeclarations => "script-redeclarations",
);

#[cfg(feature = "typescript")]
const CASES_TS_CALLABLE_REDECLARATIONS_GOLDEN: Corpus = Corpus {
    name: "ts-callable-redeclarations",
    cases: &[
        ("shared-before", &["navigate", "src/scopes.ts:2:3"]),
        ("shared-after", &["navigate", "src/scopes.ts:4:10"]),
        ("separate", &["navigate", "src/scopes.ts:8:10"]),
        ("combined", &["navigate", "src/scopes.ts:13:10"]),
        ("lexical", &["navigate", "src/scopes.ts:18:10"]),
        ("pattern", &["navigate", "src/scopes.ts:22:10"]),
        ("pattern-default", &["navigate", "src/scopes.ts:26:10"]),
        ("functions", &["navigate", "src/scopes.ts:30:10"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_callable_redeclarations_golden, CASES_TS_CALLABLE_REDECLARATIONS_GOLDEN;
    case_shared_before => "shared-before",
    case_shared_after => "shared-after",
    case_separate => "separate",
    case_combined => "combined",
    case_lexical => "lexical",
    case_pattern => "pattern",
    case_pattern_default => "pattern-default",
    case_functions => "functions",
);

#[cfg(feature = "typescript")]
const CASES_TS_VAR_HOISTING_GOLDEN: Corpus = Corpus {
    name: "ts-var-hoisting",
    cases: &[
        ("before", &["navigate", "src/scopes.ts:3:3"]),
        ("default-import", &["navigate", "src/scopes.ts:2:30"]),
        ("capture", &["navigate", "src/scopes.ts:5:25"]),
        ("inner-shadow", &["navigate", "src/scopes.ts:6:20"]),
        ("after-block", &["navigate", "src/scopes.ts:7:10"]),
        ("pattern-before", &["navigate", "src/scopes.ts:10:3"]),
        ("pattern-self", &["navigate", "src/scopes.ts:11:24"]),
        ("pattern-forward", &["navigate", "src/scopes.ts:11:49"]),
        ("pattern-key", &["navigate", "src/scopes.ts:11:32"]),
        ("array-rest", &["navigate", "src/scopes.ts:12:19"]),
        ("classic-before", &["navigate", "src/scopes.ts:16:3"]),
        ("classic-header", &["navigate", "src/scopes.ts:17:23"]),
        ("iteration-default", &["navigate", "src/scopes.ts:18:27"]),
        ("iteration-body", &["navigate", "src/scopes.ts:18:49"]),
        ("iteration-after", &["navigate", "src/scopes.ts:20:10"]),
        ("duplicate", &["navigate", "src/scopes.ts:23:3"]),
        ("inner-before", &["navigate", "src/scopes.ts:28:25"]),
        ("outer-capture", &["navigate", "src/scopes.ts:29:10"]),
        ("parameter-conflict", &["navigate", "src/scopes.ts:33:10"]),
        ("function-conflict", &["navigate", "src/scopes.ts:38:10"]),
        ("unsupported-pattern", &["navigate", "src/scopes.ts:43:10"]),
        ("legacy-header", &["navigate", "src/scopes.ts:47:10"]),
        ("await-after", &["navigate", "src/scopes.ts:51:10"]),
        ("outside", &["navigate", "src/scopes.ts:53:1"]),
        ("tsx", &["navigate", "src/view.tsx:2:3"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_var_hoisting_golden, CASES_TS_VAR_HOISTING_GOLDEN;
    case_before => "before",
    case_default_import => "default-import",
    case_capture => "capture",
    case_inner_shadow => "inner-shadow",
    case_after_block => "after-block",
    case_pattern_before => "pattern-before",
    case_pattern_self => "pattern-self",
    case_pattern_forward => "pattern-forward",
    case_pattern_key => "pattern-key",
    case_array_rest => "array-rest",
    case_classic_before => "classic-before",
    case_classic_header => "classic-header",
    case_iteration_default => "iteration-default",
    case_iteration_body => "iteration-body",
    case_iteration_after => "iteration-after",
    case_duplicate => "duplicate",
    case_inner_before => "inner-before",
    case_outer_capture => "outer-capture",
    case_parameter_conflict => "parameter-conflict",
    case_function_conflict => "function-conflict",
    case_unsupported_pattern => "unsupported-pattern",
    case_legacy_header => "legacy-header",
    case_await_after => "await-after",
    case_outside => "outside",
    case_tsx => "tsx",
);

#[cfg(feature = "typescript")]
const CASES_TS_FUNCTION_HOISTING_GOLDEN: Corpus = Corpus {
    name: "ts-function-hoisting",
    cases: &[
        ("before", &["navigate", "src/scopes.ts:3:3"]),
        ("parameter-default", &["navigate", "src/scopes.ts:2:34"]),
        ("recursive", &["navigate", "src/scopes.ts:4:47"]),
        ("capture", &["navigate", "src/scopes.ts:5:25"]),
        ("inner-shadow", &["navigate", "src/scopes.ts:6:29"]),
        ("after-inner", &["navigate", "src/scopes.ts:7:3"]),
        ("generator-before", &["navigate", "src/scopes.ts:11:3"]),
        ("generator-recursive", &["navigate", "src/scopes.ts:12:51"]),
        ("arrow-before", &["navigate", "src/scopes.ts:15:3"]),
        ("arrow-recursive", &["navigate", "src/scopes.ts:16:28"]),
        ("method-before", &["navigate", "src/scopes.ts:21:5"]),
        ("duplicate", &["navigate", "src/scopes.ts:27:3"]),
        ("nested-before", &["navigate", "src/scopes.ts:32:3"]),
        ("nested-inside", &["navigate", "src/scopes.ts:33:26"]),
        ("nested-after", &["navigate", "src/scopes.ts:34:3"]),
        ("overload", &["navigate", "src/scopes.ts:37:3"]),
        ("var-barrier", &["navigate", "src/scopes.ts:42:3"]),
        ("outside", &["navigate", "src/scopes.ts:46:1"]),
        ("default-import", &["navigate", "src/scopes.ts:47:39"]),
        ("default-body", &["navigate", "src/scopes.ts:48:3"]),
        (
            "signature",
            &["context", "src/scopes.ts:3:3", "--detail", "signature"],
        ),
        ("tsx", &["navigate", "src/view.tsx:2:3"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_function_hoisting_golden, CASES_TS_FUNCTION_HOISTING_GOLDEN;
    case_before => "before",
    case_parameter_default => "parameter-default",
    case_recursive => "recursive",
    case_capture => "capture",
    case_inner_shadow => "inner-shadow",
    case_after_inner => "after-inner",
    case_generator_before => "generator-before",
    case_generator_recursive => "generator-recursive",
    case_arrow_before => "arrow-before",
    case_arrow_recursive => "arrow-recursive",
    case_method_before => "method-before",
    case_duplicate => "duplicate",
    case_nested_before => "nested-before",
    case_nested_inside => "nested-inside",
    case_nested_after => "nested-after",
    case_overload => "overload",
    case_var_barrier => "var-barrier",
    case_outside => "outside",
    case_default_import => "default-import",
    case_default_body => "default-body",
    case_signature => "signature",
    case_tsx => "tsx",
);

#[cfg(feature = "typescript")]
const CASES_TS_CALLABLE_SIGNATURES_GOLDEN: Corpus = Corpus {
    name: "ts-callable-signatures",
    cases: &[
        (
            "arrow",
            &["context", "src/values.ts:2:14", "--detail", "signature"],
        ),
        (
            "block",
            &["context", "src/values.ts:3:14", "--detail", "signature"],
        ),
        (
            "expression-value",
            &["context", "src/values.ts:4:14", "--detail", "signature"],
        ),
        (
            "expression-name",
            &["context", "src/values.ts:5:18", "--detail", "signature"],
        ),
        (
            "generator-value",
            &["context", "src/values.ts:7:14", "--detail", "signature"],
        ),
        (
            "generator-name",
            &["context", "src/values.ts:8:22", "--detail", "signature"],
        ),
        (
            "async",
            &["context", "src/values.ts:10:14", "--detail", "signature"],
        ),
        (
            "literal",
            &["context", "src/values.ts:11:14", "--detail", "signature"],
        ),
        (
            "wrapped",
            &["context", "src/values.ts:12:14", "--detail", "signature"],
        ),
        (
            "first",
            &["context", "src/values.ts:13:12", "--detail", "signature"],
        ),
        (
            "second",
            &["context", "src/values.ts:13:46", "--detail", "signature"],
        ),
        (
            "field",
            &["context", "src/values.ts:15:3", "--detail", "signature"],
        ),
        (
            "literal-field",
            &["context", "src/values.ts:16:3", "--detail", "signature"],
        ),
        (
            "local",
            &["context", "src/values.ts:20:10", "--detail", "signature"],
        ),
        (
            "tsx",
            &["context", "src/view.tsx:1:14", "--detail", "signature"],
        ),
        (
            "tsx-field",
            &["context", "src/view.tsx:3:3", "--detail", "signature"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_callable_signatures_golden, CASES_TS_CALLABLE_SIGNATURES_GOLDEN;
    case_arrow => "arrow",
    case_block => "block",
    case_expression_value => "expression-value",
    case_expression_name => "expression-name",
    case_generator_value => "generator-value",
    case_generator_name => "generator-name",
    case_async => "async",
    case_literal => "literal",
    case_wrapped => "wrapped",
    case_first => "first",
    case_second => "second",
    case_field => "field",
    case_literal_field => "literal-field",
    case_local => "local",
    case_tsx => "tsx",
    case_tsx_field => "tsx-field",
);

#[cfg(feature = "typescript")]
const CASES_TS_CALLABLE_SCOPES_GOLDEN: Corpus = Corpus {
    name: "ts-scopes",
    cases: &[
        ("single", &["navigate", "src/scopes.ts:2:27"]),
        ("capture", &["navigate", "src/scopes.ts:2:35"]),
        ("pattern", &["navigate", "src/scopes.ts:6:13"]),
        ("inner-default", &["navigate", "src/scopes.ts:10:44"]),
        ("default-earlier", &["navigate", "src/scopes.ts:10:60"]),
        ("forward", &["navigate", "src/scopes.ts:14:29"]),
        ("recursion", &["navigate", "src/scopes.ts:19:5"]),
        ("recursion-default", &["navigate", "src/scopes.ts:18:45"]),
        ("name-no-leak", &["navigate", "src/scopes.ts:22:3"]),
        ("name-shadow", &["navigate", "src/scopes.ts:26:49"]),
        ("generator", &["navigate", "src/scopes.ts:31:9"]),
        ("generator-local", &["navigate", "src/scopes.ts:31:22"]),
        ("generator-expression", &["navigate", "src/scopes.ts:35:11"]),
        ("async", &["navigate", "src/scopes.ts:40:35"]),
        ("catch-pattern", &["navigate", "src/scopes.ts:46:5"]),
        ("catch-local", &["navigate", "src/scopes.ts:46:31"]),
        ("catch-default", &["navigate", "src/scopes.ts:44:83"]),
        ("catch-no-leak", &["navigate", "src/scopes.ts:48:3"]),
        ("simple-catch", &["navigate", "src/scopes.ts:52:34"]),
        ("after-catch", &["navigate", "src/scopes.ts:53:10"]),
        ("optional-catch", &["navigate", "src/scopes.ts:56:26"]),
        ("catch-forward", &["navigate", "src/scopes.ts:60:26"]),
        ("isolated-var", &["navigate", "src/scopes.ts:65:10"]),
        ("invalid", &["navigate", "src/scopes.ts:68:56"]),
        ("invalid-after", &["navigate", "src/scopes.ts:69:10"]),
        ("duplicates", &["navigate", "src/scopes.ts:72:38"]),
        ("generic-value", &["navigate", "src/scopes.ts:80:40"]),
        ("generic-type", &["navigate", "src/scopes.ts:80:35"]),
        ("tsx", &["navigate", "src/view.tsx:3:50"]),
        (
            "references",
            &[
                "relationships",
                "references",
                "src/scopes.ts:2:27",
                "--path",
                "src/scopes.ts",
                "--max-lookups",
                "100",
            ],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_callable_scopes_golden, CASES_TS_CALLABLE_SCOPES_GOLDEN;
    case_single => "single",
    case_capture => "capture",
    case_pattern => "pattern",
    case_inner_default => "inner-default",
    case_default_earlier => "default-earlier",
    case_forward => "forward",
    case_recursion => "recursion",
    case_recursion_default => "recursion-default",
    case_name_no_leak => "name-no-leak",
    case_name_shadow => "name-shadow",
    case_generator => "generator",
    case_generator_local => "generator-local",
    case_generator_expression => "generator-expression",
    case_async => "async",
    case_catch_pattern => "catch-pattern",
    case_catch_local => "catch-local",
    case_catch_default => "catch-default",
    case_catch_no_leak => "catch-no-leak",
    case_simple_catch => "simple-catch",
    case_after_catch => "after-catch",
    case_optional_catch => "optional-catch",
    case_catch_forward => "catch-forward",
    case_isolated_var => "isolated-var",
    case_invalid => "invalid",
    case_invalid_after => "invalid-after",
    case_duplicates => "duplicates",
    case_generic_value => "generic-value",
    case_generic_type => "generic-type",
    case_tsx => "tsx",
    case_references => "references",
);

#[cfg(feature = "typescript")]
const CASES_TS_LOOP_BINDINGS_GOLDEN: Corpus = Corpus {
    name: "ts-loops",
    cases: &[
        ("classic-condition", &["navigate", "src/loops.ts:2:41"]),
        ("classic-update", &["navigate", "src/loops.ts:2:55"]),
        ("earlier-declarator", &["navigate", "src/loops.ts:2:34"]),
        ("classic-body", &["navigate", "src/loops.ts:3:5"]),
        ("inner-shadow", &["navigate", "src/loops.ts:4:28"]),
        ("classic-no-leak", &["navigate", "src/loops.ts:6:3"]),
        ("parameter-after", &["navigate", "src/loops.ts:7:10"]),
        ("self", &["navigate", "src/loops.ts:10:20"]),
        ("self-body", &["navigate", "src/loops.ts:10:45"]),
        ("self-after", &["navigate", "src/loops.ts:11:10"]),
        ("forward", &["navigate", "src/loops.ts:14:20"]),
        ("each", &["navigate", "src/loops.ts:19:5"]),
        ("each-default", &["navigate", "src/loops.ts:18:56"]),
        ("each-rest", &["navigate", "src/loops.ts:19:22"]),
        ("label", &["navigate", "src/loops.ts:18:16"]),
        ("each-no-leak", &["navigate", "src/loops.ts:21:3"]),
        ("iterable-tdz", &["navigate", "src/loops.ts:25:21"]),
        ("iterable-body", &["navigate", "src/loops.ts:25:30"]),
        ("iterable-after", &["navigate", "src/loops.ts:26:10"]),
        ("keys-body", &["navigate", "src/loops.ts:29:28"]),
        ("keys-source", &["navigate", "src/loops.ts:29:21"]),
        ("awaited", &["navigate", "src/loops.ts:33:54"]),
        ("duplicate", &["navigate", "src/loops.ts:37:37"]),
        ("invalid", &["navigate", "src/loops.ts:40:48"]),
        ("invalid-after", &["navigate", "src/loops.ts:41:10"]),
        ("var-after", &["navigate", "src/loops.ts:45:10"]),
        ("assignment", &["navigate", "src/loops.ts:48:29"]),
        ("assignment-after", &["navigate", "src/loops.ts:49:10"]),
        ("empty-body", &["navigate", "src/loops.ts:52:14"]),
        ("tsx", &["navigate", "src/view.tsx:3:46"]),
        ("malformed-header", &["navigate", "src/loops.ts:56:46"]),
        ("malformed-after", &["navigate", "src/loops.ts:57:10"]),
        ("initialized-var-after", &["navigate", "src/loops.ts:61:10"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_loop_bindings_golden, CASES_TS_LOOP_BINDINGS_GOLDEN;
    case_classic_condition => "classic-condition",
    case_classic_update => "classic-update",
    case_earlier_declarator => "earlier-declarator",
    case_classic_body => "classic-body",
    case_inner_shadow => "inner-shadow",
    case_classic_no_leak => "classic-no-leak",
    case_parameter_after => "parameter-after",
    case_self => "self",
    case_self_body => "self-body",
    case_self_after => "self-after",
    case_forward => "forward",
    case_each => "each",
    case_each_default => "each-default",
    case_each_rest => "each-rest",
    case_label => "label",
    case_each_no_leak => "each-no-leak",
    case_iterable_tdz => "iterable-tdz",
    case_iterable_body => "iterable-body",
    case_iterable_after => "iterable-after",
    case_keys_body => "keys-body",
    case_keys_source => "keys-source",
    case_awaited => "awaited",
    case_duplicate => "duplicate",
    case_invalid => "invalid",
    case_invalid_after => "invalid-after",
    case_var_after => "var-after",
    case_assignment => "assignment",
    case_assignment_after => "assignment-after",
    case_empty_body => "empty-body",
    case_tsx => "tsx",
    case_malformed_header => "malformed-header",
    case_malformed_after => "malformed-after",
    case_initialized_var_after => "initialized-var-after",
);

#[cfg(feature = "typescript")]
const CASES_TS_LOCAL_BINDINGS_GOLDEN: Corpus = Corpus {
    name: "ts-locals",
    cases: &[
        ("parameter", &["navigate", "src/locals.ts:6:11"]),
        ("const", &["navigate", "src/locals.ts:6:18"]),
        (
            "let-uninitialized-value",
            &["navigate", "src/locals.ts:6:25"],
        ),
        ("assignment", &["navigate", "src/locals.ts:5:10"]),
        ("before", &["navigate", "src/locals.ts:9:3"]),
        ("after", &["navigate", "src/locals.ts:11:10"]),
        ("self", &["navigate", "src/locals.ts:14:17"]),
        ("self-after", &["navigate", "src/locals.ts:15:10"]),
        ("nested", &["navigate", "src/locals.ts:21:5"]),
        ("outer", &["navigate", "src/locals.ts:23:10"]),
        ("inner-wins", &["navigate", "src/locals.ts:28:5"]),
        ("outer-after", &["navigate", "src/locals.ts:31:10"]),
        ("renamed", &["navigate", "src/locals.ts:36:11"]),
        ("nested-pattern", &["navigate", "src/locals.ts:36:20"]),
        ("shorthand-default", &["navigate", "src/locals.ts:36:27"]),
        ("object-rest", &["navigate", "src/locals.ts:36:34"]),
        ("array-default", &["navigate", "src/locals.ts:36:40"]),
        ("array-rest", &["navigate", "src/locals.ts:36:46"]),
        ("label", &["navigate", "src/locals.ts:34:11"]),
        ("earlier-default", &["navigate", "src/locals.ts:39:34"]),
        ("earlier-key", &["navigate", "src/locals.ts:39:42"]),
        ("forward-default", &["navigate", "src/locals.ts:43:19"]),
        ("whole-initializer", &["navigate", "src/locals.ts:47:21"]),
        ("multiple", &["navigate", "src/locals.ts:51:31"]),
        ("forward-declarator", &["navigate", "src/locals.ts:55:15"]),
        ("duplicate", &["navigate", "src/locals.ts:64:10"]),
        ("unsupported-peer", &["navigate", "src/locals.ts:69:10"]),
        ("var-barrier", &["navigate", "src/locals.ts:73:10"]),
        ("loop-barrier", &["navigate", "src/locals.ts:77:10"]),
        ("tsx-parameter", &["navigate", "src/methods.tsx:4:22"]),
        ("tsx-local", &["navigate", "src/methods.tsx:4:29"]),
        ("tsx-array", &["navigate", "src/methods.tsx:4:38"]),
        ("tsx-rest", &["navigate", "src/methods.tsx:4:45"]),
        ("tsx-default", &["navigate", "src/methods.tsx:3:30"]),
        (
            "indirect-callee",
            &["relationships", "callees", "src/locals.ts:58:17"],
        ),
        (
            "local-references",
            &[
                "relationships",
                "references",
                "src/locals.ts:6:18",
                "--path",
                "src/locals.ts",
                "--max-lookups",
                "100",
            ],
        ),
        (
            "signature",
            &["context", "src/locals.ts:79:17", "--detail", "signature"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_local_bindings_golden, CASES_TS_LOCAL_BINDINGS_GOLDEN;
    case_parameter => "parameter",
    case_const => "const",
    case_let_uninitialized_value => "let-uninitialized-value",
    case_assignment => "assignment",
    case_before => "before",
    case_after => "after",
    case_self => "self",
    case_self_after => "self-after",
    case_nested => "nested",
    case_outer => "outer",
    case_inner_wins => "inner-wins",
    case_outer_after => "outer-after",
    case_renamed => "renamed",
    case_nested_pattern => "nested-pattern",
    case_shorthand_default => "shorthand-default",
    case_object_rest => "object-rest",
    case_array_default => "array-default",
    case_array_rest => "array-rest",
    case_label => "label",
    case_earlier_default => "earlier-default",
    case_earlier_key => "earlier-key",
    case_forward_default => "forward-default",
    case_whole_initializer => "whole-initializer",
    case_multiple => "multiple",
    case_forward_declarator => "forward-declarator",
    case_duplicate => "duplicate",
    case_unsupported_peer => "unsupported-peer",
    case_var_barrier => "var-barrier",
    case_loop_barrier => "loop-barrier",
    case_tsx_parameter => "tsx-parameter",
    case_tsx_local => "tsx-local",
    case_tsx_array => "tsx-array",
    case_tsx_rest => "tsx-rest",
    case_tsx_default => "tsx-default",
    case_indirect_callee => "indirect-callee",
    case_local_references => "local-references",
    case_signature => "signature",
);

#[cfg(feature = "typescript")]
const CASES_TS_PARAMETER_PATTERNS_GOLDEN: Corpus = Corpus {
    name: "ts-parameters",
    cases: &[
        ("renamed", &["navigate", "src/parameters.ts:3:11"]),
        ("nested", &["navigate", "src/parameters.ts:3:20"]),
        ("shorthand", &["navigate", "src/parameters.ts:3:27"]),
        ("default-binding", &["navigate", "src/parameters.ts:3:34"]),
        ("object-rest", &["navigate", "src/parameters.ts:3:41"]),
        ("array-default", &["navigate", "src/parameters.ts:3:47"]),
        ("array-rest", &["navigate", "src/parameters.ts:3:53"]),
        ("label", &["navigate", "src/parameters.ts:2:26"]),
        ("declaration", &["navigate", "src/parameters.ts:2:64"]),
        ("default-import", &["navigate", "src/parameters.ts:5:35"]),
        ("earlier-field", &["navigate", "src/parameters.ts:5:50"]),
        ("earlier-parameter", &["navigate", "src/parameters.ts:5:67"]),
        ("forward-default", &["navigate", "src/parameters.ts:8:35"]),
        ("self-default", &["navigate", "src/parameters.ts:11:32"]),
        ("computed-import", &["navigate", "src/parameters.ts:14:29"]),
        ("computed-earlier", &["navigate", "src/parameters.ts:14:47"]),
        ("whole-default", &["navigate", "src/parameters.ts:17:42"]),
        ("whole-body", &["navigate", "src/parameters.ts:18:10"]),
        ("duplicate", &["navigate", "src/parameters.ts:24:10"]),
        ("unsupported-peer", &["navigate", "src/parameters.ts:27:10"]),
        ("local-barrier", &["navigate", "src/parameters.ts:31:10"]),
        (
            "anonymous-barrier",
            &["navigate", "src/parameters.ts:34:16"],
        ),
        ("tsx-heading", &["navigate", "src/methods.tsx:4:22"]),
        ("tsx-array", &["navigate", "src/methods.tsx:4:31"]),
        ("tsx-rest", &["navigate", "src/methods.tsx:4:38"]),
        ("tsx-later-default", &["navigate", "src/methods.tsx:3:72"]),
        ("nested-array-rest", &["navigate", "src/methods.tsx:7:12"]),
        (
            "duplicate-selected",
            &[
                "navigate",
                "src/parameters.ts:24:10",
                "--select",
                "0d2839d288e7",
            ],
        ),
        (
            "parameter-references",
            &[
                "relationships",
                "references",
                "src/parameters.ts:3:11",
                "--path",
                "src/parameters.ts",
                "--max-lookups",
                "100",
            ],
        ),
        (
            "signature",
            &["context", "src/parameters.ts:2:17", "--detail", "signature"],
        ),
        (
            "indirect-callee",
            &["relationships", "callees", "src/parameters.ts:20:17"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_parameter_patterns_golden, CASES_TS_PARAMETER_PATTERNS_GOLDEN;
    case_renamed => "renamed",
    case_nested => "nested",
    case_shorthand => "shorthand",
    case_default_binding => "default-binding",
    case_object_rest => "object-rest",
    case_array_default => "array-default",
    case_array_rest => "array-rest",
    case_label => "label",
    case_declaration => "declaration",
    case_default_import => "default-import",
    case_earlier_field => "earlier-field",
    case_earlier_parameter => "earlier-parameter",
    case_forward_default => "forward-default",
    case_self_default => "self-default",
    case_computed_import => "computed-import",
    case_computed_earlier => "computed-earlier",
    case_whole_default => "whole-default",
    case_whole_body => "whole-body",
    case_duplicate => "duplicate",
    case_unsupported_peer => "unsupported-peer",
    case_local_barrier => "local-barrier",
    case_anonymous_barrier => "anonymous-barrier",
    case_tsx_heading => "tsx-heading",
    case_tsx_array => "tsx-array",
    case_tsx_rest => "tsx-rest",
    case_tsx_later_default => "tsx-later-default",
    case_nested_array_rest => "nested-array-rest",
    case_duplicate_selected => "duplicate-selected",
    case_parameter_references => "parameter-references",
    case_signature => "signature",
    case_indirect_callee => "indirect-callee",
);

#[cfg(feature = "typescript")]
const CASES_TS_NAVIGATION_GOLDEN: Corpus = Corpus {
    name: "ts-navigation",
    cases: &[
        (
            "context-signature",
            &["context", "src/signatures.ts:2:17", "--detail", "signature"],
        ),
        (
            "context-signature-unsupported",
            &["context", "src/signatures.ts:5:14", "--detail", "signature"],
        ),
        ("named-import", &["navigate", "src/consumer.ts:1:10"]),
        (
            "context-default-import",
            &["context", "src/bindings.ts:4:19"],
        ),
        ("default-import", &["navigate", "src/bindings.ts:4:19"]),
        ("namespace-import", &["navigate", "src/bindings.ts:5:25"]),
        (
            "forwarded-local-export",
            &["navigate", "src/bindings.ts:12:16"],
        ),
        ("direct-local-export", &["navigate", "src/bindings.ts:8:26"]),
        ("unknown-namespace", &["navigate", "src/bindings.ts:10:31"]),
        ("local-export", &["navigate", "src/bindings.ts:6:23"]),
        ("import-alias", &["navigate", "src/consumer.ts:1:19"]),
        ("reexport-alias", &["navigate", "src/consumer.ts:4:12"]),
        ("not-imported", &["navigate", "src/consumer.ts:5:16"]),
        ("private", &["navigate", "src/consumer.ts:6:16"]),
        ("generic", &["navigate", "src/consumer.ts:7:34"]),
        ("parameter", &["navigate", "src/consumer.ts:7:61"]),
        ("interface-field", &["navigate", "src/consumer.ts:8:24"]),
        (
            "default-is-not-named",
            &["navigate", "src/consumer.ts:10:17"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_navigation_golden, CASES_TS_NAVIGATION_GOLDEN;
    case_context_signature => "context-signature",
    case_context_signature_unsupported => "context-signature-unsupported",
    case_named_import => "named-import",
    case_context_default_import => "context-default-import",
    case_default_import => "default-import",
    case_namespace_import => "namespace-import",
    case_forwarded_local_export => "forwarded-local-export",
    case_direct_local_export => "direct-local-export",
    case_unknown_namespace => "unknown-namespace",
    case_local_export => "local-export",
    case_import_alias => "import-alias",
    case_reexport_alias => "reexport-alias",
    case_not_imported => "not-imported",
    case_private => "private",
    case_generic => "generic",
    case_parameter => "parameter",
    case_interface_field => "interface-field",
    case_default_is_not_named => "default-is-not-named",
);

/// Protocol paging over real grammars; only opaque cursor bytes are normalized.
#[cfg(any(feature = "rust", feature = "typescript"))]
struct PageTranscript {
    replies: Vec<serde_json::Value>,
    cursors: BTreeMap<String, String>,
}
#[cfg(any(feature = "rust", feature = "typescript"))]
impl PageTranscript {
    fn normalize(&mut self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if ["next_cursor", "expansion", "body_expansion", "plan_id"]
                        .contains(&key.as_str())
                        && value.is_string()
                    {
                        let next = format!("cursor-{}", self.cursors.len() + 1);
                        let name = self
                            .cursors
                            .entry(value.as_str().unwrap().into())
                            .or_insert(next);
                        *value = serde_json::Value::String(name.clone());
                    } else {
                        self.normalize(value);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    self.normalize(value);
                }
            }
            _ => {}
        }
    }

    fn call(&mut self, engine: &Engine, request: serde_json::Value) -> serde_json::Value {
        #[cfg(feature = "schemas")]
        let command = request["command"].as_str().unwrap().to_owned();
        let reply = serde_json::to_value(
            serde_json::from_value::<vvv_engine::Call>(request)
                .unwrap()
                .execute(engine),
        )
        .unwrap();
        #[cfg(feature = "schemas")]
        Corpus::validate_response(&[&command], &serde_json::to_vec(&reply).unwrap());
        assert_eq!(reply["status"], "ok", "{reply}");
        let mut normalized = reply.clone();
        self.normalize(&mut normalized);
        self.replies.push(normalized);
        reply
    }
    #[cfg(any(unix, windows))]
    fn validation(corpus: Corpus, path: &str, symbol: &str) {
        let disk = ValidationCorpus::new(&corpus);
        let engine = Engine::new(Workspace::disk(&disk.root).unwrap(), Builtins::registry());
        let prepared = vvv_engine::PrepareRenameQuery {
            intent: RenameIntent::new("Engine", "Runtime")
                .declared_in(path)
                .of_symbol(symbol.parse().unwrap()),
            max_bytes: 65536,
            page: None,
        }
        .execute(&engine)
        .unwrap()
        .into_complete()
        .unwrap();
        let receipt = vvv_engine::ApplyPlanQuery {
            plan_id: prepared.plan_id,
        }
        .execute(&engine)
        .unwrap();
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let report = transcript.call(&engine, serde_json::json!({"command":"validate_plan", "plan_id":receipt.plan_id, "checks":[{"name":"check renamed declarations", "program": std::env::current_exe().unwrap(), "args":["--exact","corpus_validation_command", "--ignored", "--nocapture"]}], "budget":{"max_bytes":8192,"timeout_ms":5000}}));
        assert_eq!(report["result"]["passed"], true);
        let inspected = transcript.call(
            &engine,
            serde_json::json!({"command":"inspect_plan", "plan_id":receipt.plan_id}),
        );
        assert_eq!(inspected["result"]["validation"], report["result"]);
        for reply in &mut transcript.replies {
            let result = if reply["result"]["validation"].is_object() {
                &mut reply["result"]["validation"]
            } else {
                &mut reply["result"]
            };
            result["checks"][0]["command"]["program"] = "<corpus test executable>".into();
            result["checks"][0]["duration_ms"] = 0.into();
        }
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__validation__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }

    fn plans(corpus: Corpus, path: &str, symbol: &str) {
        let (vfs, engine) = corpus.engine();
        let before = snapshot(&vfs);
        let intent = RenameIntent::new("Engine", "Runtime")
            .declared_in(path)
            .of_symbol(symbol.parse().unwrap());
        let expected = intent.plan(&engine).unwrap();
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let prepared = transcript.call(&engine, serde_json::json!({"command":"prepare_rename","intent":{"name":"Engine","to":"Runtime","declared_in":path,"symbol":symbol},"max_bytes":65536}));
        let id = &prepared["result"]["plan_id"];
        let inspected = transcript.call(
            &engine,
            serde_json::json!({"command":"inspect_plan","plan_id":id,"max_bytes":65536}),
        );
        assert_eq!(inspected, prepared);
        let applied = transcript.call(
            &engine,
            serde_json::json!({"command":"apply_plan","plan_id":id}),
        );
        for file in expected.preview() {
            assert_eq!(
                vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
                file.after
            );
        }
        let replay = transcript.call(
            &engine,
            serde_json::json!({"command":"apply_plan","plan_id":id}),
        );
        assert_eq!(applied, replay);
        let inspected = transcript.call(
            &engine,
            serde_json::json!({"command":"inspect_plan","plan_id":id}),
        );
        assert_eq!(inspected["result"]["receipt"], applied["result"]);
        let undone = vvv_engine::Ledger::new(&engine).undo().unwrap();
        assert_eq!(
            undone.undone.id,
            applied["result"]["history_id"].as_u64().unwrap()
        );
        assert_eq!(snapshot(&vfs), before);
        assert_eq!(
            transcript.call(
                &engine,
                serde_json::json!({"command":"apply_plan","plan_id":id})
            ),
            applied
        );
        assert!(
            vvv_engine::Ledger::new(&engine)
                .history()
                .unwrap()
                .entries
                .is_empty()
        );
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__retained-plan__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }

    fn moves(corpus: Corpus, from: &str, to: &str) {
        let (vfs, engine) = corpus.engine();
        let before = snapshot(&vfs);
        let expected = vvv_engine::MoveIntent::new(from, to).plan(&engine).unwrap();
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let first = transcript.call(&engine, serde_json::json!({"command":"prepare_move","intent":{"from":from,"to":to},"page":{"max_items":3,"max_bytes":1024}}));
        let id = first["result"]["plan_id"].clone();
        let mut cursor = first["result"]["next_cursor"].clone();
        let mut count = 0;
        while !cursor.is_null() {
            count += 1;
            assert!(count < 200);
            let reply = transcript.call(&engine, serde_json::json!({"command":"review_plan","cursor":cursor,"page":{"max_items":3,"max_bytes":1024}}));
            assert!(serde_json::to_vec(&reply["result"]).unwrap().len() <= 1024);
            cursor = reply["result"]["next_cursor"].clone();
        }
        assert!(count > 0);
        let inspected = transcript.call(
            &engine,
            serde_json::json!({"command":"inspect_plan","plan_id":id,"max_bytes":65536}),
        );
        assert_eq!(
            inspected["result"]["preview"],
            serde_json::to_value(&*expected).unwrap()
        );
        let applied = transcript.call(
            &engine,
            serde_json::json!({"command":"apply_plan","plan_id":id}),
        );
        for file in expected.preview() {
            assert_eq!(
                vfs.read(&Path::new("/ws").join(file.moved_to.as_ref().unwrap_or(&file.path)))
                    .unwrap(),
                file.after
            );
            if file.moved_to.is_some() {
                assert!(!vfs.exists(&Path::new("/ws").join(&file.path)));
            }
        }
        assert_eq!(
            transcript.call(
                &engine,
                serde_json::json!({"command":"apply_plan","plan_id":id})
            ),
            applied
        );
        vvv_engine::Ledger::new(&engine).undo().unwrap();
        assert_eq!(snapshot(&vfs), before);
        assert_eq!(
            transcript.call(
                &engine,
                serde_json::json!({"command":"apply_plan","plan_id":id})
            ),
            applied
        );
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__retained-move__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }

    fn rewrites(corpus: Corpus) {
        let (vfs, engine) = corpus.engine();
        let before = snapshot(&vfs);
        let expected = RewriteIntent::new(Query::pattern("Engine"), "Runtime")
            .plan(&engine)
            .unwrap();
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let first = transcript.call(&engine, serde_json::json!({"command":"prepare_rewrite","intent":{"query":{"pattern":"Engine"},"template":"Runtime"},"page":{"max_items":3,"max_bytes":1024}}));
        let id = first["result"]["plan_id"].clone();
        let mut cursor = first["result"]["next_cursor"].clone();
        let mut pages = 0;
        while !cursor.is_null() {
            pages += 1;
            assert!(pages < 200);
            let page = transcript.call(&engine, serde_json::json!({"command":"review_plan","cursor":cursor,"page":{"max_items":3,"max_bytes":1024}}));
            assert!(serde_json::to_vec(&page["result"]).unwrap().len() <= 1024);
            cursor = page["result"]["next_cursor"].clone();
        }
        assert!(pages > 0);
        let inspected = transcript.call(
            &engine,
            serde_json::json!({"command":"inspect_plan","plan_id":id,"max_bytes":65536}),
        );
        assert_eq!(
            inspected["result"]["preview"],
            serde_json::to_value(&*expected).unwrap()
        );
        let applied = transcript.call(
            &engine,
            serde_json::json!({"command":"apply_plan","plan_id":id}),
        );
        for file in expected.preview() {
            assert_eq!(
                vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
                file.after
            );
        }
        assert_eq!(
            transcript.call(
                &engine,
                serde_json::json!({"command":"apply_plan","plan_id":id})
            ),
            applied
        );
        vvv_engine::Ledger::new(&engine).undo().unwrap();
        assert_eq!(snapshot(&vfs), before);
        assert_eq!(
            transcript.call(
                &engine,
                serde_json::json!({"command":"apply_plan","plan_id":id})
            ),
            applied
        );
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__retained-rewrite__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }

    fn relationships(corpus: Corpus, path: &str, line: u32, column: u32) {
        let (_, engine) = corpus.engine();
        let origin =
            vvv_engine::NavigationQuery::at(path, vvv_engine::Position::new(line, column)).origin;
        let mut query = vvv_engine::RelationshipsQuery::new(
            origin.clone(),
            vvv_engine::RelationshipKind::References,
        );
        query.budget.max_bytes = 65536;
        let expected = query.execute(&engine).unwrap();
        assert!(expected.next_cursor.is_none());
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let first = transcript.call(&engine, serde_json::json!({"command":"relationships","origin":origin,"kind":"references","budget":{"max_items":1,"max_lookups":1,"max_files":1,"max_bytes":8192}}));
        assert_eq!(first["status"], "ok", "{first}");
        let mut sites = first["result"]["items"].as_array().unwrap().clone();
        let mut cursor = first["result"]["next_cursor"].clone();
        let mut pages = 0;
        while cursor.is_string() {
            pages += 1;
            assert!(pages < 100);
            let reply = transcript.call(&engine, serde_json::json!({"command":"continue","cursor":cursor,"page":{"max_items":1,"max_bytes":8192},"work":{"max_lookups":1,"max_files":1}}));
            assert_eq!(reply["status"], "ok", "{reply}");
            sites.extend(reply["result"]["items"].as_array().unwrap().clone());
            cursor = reply["result"]["next_cursor"].clone();
        }
        let sites = sites
            .into_iter()
            .map(|site| serde_json::from_value::<vvv_engine::Relationship>(site).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sites, expected.items);
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__incoming-pages__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }

    fn corpus(corpus: Corpus, path: &str, line: u32, column: u32) {
        let (_, engine) = corpus.engine();
        let mut transcript = Self {
            replies: vec![],
            cursors: BTreeMap::new(),
        };
        let first = transcript.call(&engine, serde_json::json!({"command":"search_page","query":{"pattern":"Engine"},"page":{"max_items":1,"max_bytes":4096}}));
        let mut cursor = first["result"]["next_cursor"].clone();
        let mut items = first["result"]["items"].as_array().unwrap().clone();
        while !cursor.is_null() {
            let reply = transcript.call(&engine, serde_json::json!({"command":"continue","cursor":cursor,"page":{"max_items":3,"max_bytes":4096}}));
            cursor = reply["result"]["next_cursor"].clone();
            items.extend(reply["result"]["items"].as_array().unwrap().clone());
        }
        let expected = vvv_engine::SearchQuery::from(Query::pattern("Engine"))
            .execute(&engine)
            .unwrap();
        let matches = items
            .iter()
            .map(|item| serde_json::from_value::<vvv_engine::Match>(item.clone()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(matches, expected.matches);
        let origin = serde_json::json!({"kind":"position","path":path,"position":{"line":line,"column":column}});
        let first = transcript.call(&engine, serde_json::json!({"command":"context_page","origin":origin,"references":true,"page":{"max_items":1,"max_bytes":4096},"work":{"max_lookups":4,"max_files":2}}));
        let mut cursor = first["result"]["next_cursor"].clone();
        let mut items = first["result"]["items"].as_array().unwrap().clone();
        let mut pages = 0;
        while !cursor.is_null() {
            pages += 1;
            assert!(pages < 100);
            let reply = transcript.call(&engine, serde_json::json!({"command":"continue","cursor":cursor,"page":{"max_items":2,"max_bytes":4096},"work":{"max_lookups":4,"max_files":2}}));
            cursor = reply["result"]["next_cursor"].clone();
            items.extend(reply["result"]["items"].as_array().unwrap().clone());
        }
        let expected = vvv_engine::ContextQuery {
            detail: vvv_engine::ContextDetail::Body,
            include_enclosing: false,
            origin: serde_json::from_value(origin).unwrap(),
            selection: vvv_engine::Selection::All,
            references: true,
            budget: vvv_engine::ContextBudget::MAXIMUM,
        }
        .execute(&engine)
        .unwrap();
        assert_eq!(
            items
                .iter()
                .map(
                    |item| serde_json::from_value::<vvv_engine::ContextItem>(item.clone()).unwrap()
                )
                .collect::<Vec<_>>(),
            expected.items
        );
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("corpus/snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!(
                format!("{}__pagination__json", corpus.name),
                JsonSnapshot(&serde_json::to_value(&transcript.replies).unwrap()).to_string()
            )
        });
    }
}
#[cfg(feature = "rust")]
#[test]
fn rust_navigation_pagination_golden() {
    PageTranscript::corpus(
        Corpus {
            name: "rust-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        1,
        11,
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_navigation_pagination_golden() {
    PageTranscript::corpus(
        Corpus {
            name: "ts-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        0,
        13,
    );
}

#[cfg(feature = "rust")]
const CASES_RUST_RELATIONSHIPS_GOLDEN: Corpus = Corpus {
    name: "rust-relationships",
    cases: &[
        (
            "incoming-context",
            &[
                "context",
                "src/origin.rs:1:8",
                "--references",
                "--max-bytes",
                "32768",
                "--max-lookups",
                "128",
            ],
        ),
        (
            "ambiguous-call",
            &[
                "relationships",
                "callers",
                "tests/ambiguous.rs:2:4",
                "--path",
                "tests/ambiguous.rs",
            ],
        ),
        (
            "test-root",
            &[
                "relationships",
                "callers",
                "tests/local.rs:1:4",
                "--path",
                "tests/local.rs",
            ],
        ),
        (
            "callers",
            &[
                "relationships",
                "callers",
                "src/origin.rs:1:8",
                "--max-bytes",
                "32768",
            ],
        ),
        (
            "callees",
            &["relationships", "callees", "src/consumer.rs:3:8"],
        ),
        (
            "references",
            &[
                "relationships",
                "references",
                "src/origin.rs:1:8",
                "--max-bytes",
                "32768",
            ],
        ),
        (
            "limited",
            &[
                "relationships",
                "callers",
                "src/origin.rs:1:8",
                "--max-lookups",
                "1",
            ],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_relationships_golden, CASES_RUST_RELATIONSHIPS_GOLDEN;
    case_incoming_context => "incoming-context",
    case_ambiguous_call => "ambiguous-call",
    case_test_root => "test-root",
    case_callers => "callers",
    case_callees => "callees",
    case_references => "references",
    case_limited => "limited",
);
#[cfg(feature = "typescript")]
const CASES_TS_RELATIONSHIPS_GOLDEN: Corpus = Corpus {
    name: "ts-relationships",
    cases: &[
        (
            "incoming-context",
            &[
                "context",
                "src/origin.ts:1:17",
                "--references",
                "--max-bytes",
                "32768",
                "--max-lookups",
                "128",
            ],
        ),
        (
            "callers",
            &[
                "relationships",
                "callers",
                "src/origin.ts:1:17",
                "--max-bytes",
                "32768",
            ],
        ),
        (
            "callees",
            &["relationships", "callees", "src/consumer.ts:3:17"],
        ),
        (
            "references",
            &[
                "relationships",
                "references",
                "src/origin.ts:1:17",
                "--max-bytes",
                "32768",
            ],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "typescript")]
corpus_cases!(ts_relationships_golden, CASES_TS_RELATIONSHIPS_GOLDEN;
    case_incoming_context => "incoming-context",
    case_callers => "callers",
    case_callees => "callees",
    case_references => "references",
);

#[cfg(feature = "rust")]
#[test]
fn rust_retained_plan_golden() {
    PageTranscript::plans(
        Corpus {
            name: "rust-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        "struct",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_retained_plan_golden() {
    PageTranscript::plans(
        Corpus {
            name: "ts-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        "class",
    );
}

#[cfg(all(any(unix, windows), any(feature = "rust", feature = "typescript")))]
struct ValidationCorpus {
    root: PathBuf,
}
#[cfg(all(any(unix, windows), any(feature = "rust", feature = "typescript")))]
impl ValidationCorpus {
    fn new(corpus: &Corpus) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "vvv-validation-corpus-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let vfs = corpus.memory();
        for (path, text) in snapshot(&vfs) {
            let destination = root.join(path.strip_prefix("/ws").unwrap());
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(destination, text).unwrap();
        }
        Self { root }
    }
}
#[cfg(all(any(unix, windows), any(feature = "rust", feature = "typescript")))]
impl Drop for ValidationCorpus {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[cfg(all(any(unix, windows), any(feature = "rust", feature = "typescript")))]
#[test]
#[ignore = "subprocess fixture for validation corpus"]
fn corpus_validation_command() {
    use std::io::Write;
    let path = if Path::new("src/origin.rs").exists() {
        "src/origin.rs"
    } else {
        "src/origin.ts"
    };
    let source = std::fs::read_to_string(path).unwrap();
    assert!(source.contains("Runtime"));
    std::io::stdout()
        .write_all(b"renamed declaration checked\n")
        .unwrap();
    std::io::stdout().flush().unwrap();
    std::process::exit(0);
}
#[cfg(all(any(unix, windows), feature = "rust"))]
#[test]
fn rust_validation_golden() {
    PageTranscript::validation(
        Corpus {
            name: "rust-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        "struct",
    );
}
#[cfg(all(any(unix, windows), feature = "typescript"))]
#[test]
fn typescript_validation_golden() {
    PageTranscript::validation(
        Corpus {
            name: "ts-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        "class",
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_retained_rewrite_golden() {
    PageTranscript::rewrites(Corpus {
        name: "rust-navigation",
        cases: &[],
        compositions: &[],
        mutations: Vec::new,
    });
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_retained_rewrite_golden() {
    PageTranscript::rewrites(Corpus {
        name: "ts-navigation",
        cases: &[],
        compositions: &[],
        mutations: Vec::new,
    });
}

#[cfg(feature = "rust")]
#[test]
fn rust_retained_move_golden() {
    PageTranscript::moves(
        Corpus {
            name: "rust-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        "src/relocated.rs",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_retained_move_golden() {
    PageTranscript::moves(
        Corpus {
            name: "ts-navigation",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        "src/relocated.ts",
    );
}

#[cfg(feature = "typescript")]
const CASES_TYPESCRIPT_SYMBOL_MOVES_GOLDEN_AND_MUTATION_PROPERTIES: Corpus = Corpus {
    name: "ts-symbol-moves",
    cases: &[
        (
            "move-ambient-refused",
            &[
                "move",
                "src/move_ambient.ts",
                "src/move_destination.ts",
                "--symbol",
                "ambient",
            ],
        ),
        ("search-overloads", &["search", "--name", "overloaded"]),
        (
            "move-overload-ambiguous",
            &[
                "move",
                "src/move_selection.ts",
                "src/move_destination.ts",
                "--symbol",
                "overloaded",
            ],
        ),
        (
            "move-overload-selected",
            &[
                "move",
                "src/move_selection.ts",
                "src/move_destination.ts",
                "--symbol",
                "overloaded",
                "--select",
                "3",
            ],
        ),
        (
            "move-wrapper",
            &[
                "move",
                "src/move_selection.ts",
                "src/move_destination.ts",
                "--symbol",
                "Widget",
            ],
        ),
    ],
    compositions: &[("move-widget-then-rename-independent-symbol", 0, 1)],
    mutations: || {
        vec![
            Request::MoveSymbol {
                intent: MoveSymbolIntent::new(
                    "Widget",
                    "src/move_selection.ts",
                    "src/move_destination.ts",
                ),
                apply: false,
            },
            Request::Rename {
                intent: RenameIntent::new("untouched", "retained"),
                apply: false,
            },
        ]
    },
};

#[cfg(feature = "typescript")]
corpus_cases!(typescript_symbol_moves_golden_and_mutation_properties, CASES_TYPESCRIPT_SYMBOL_MOVES_GOLDEN_AND_MUTATION_PROPERTIES;
    case_move_ambient_refused => "move-ambient-refused",
    case_search_overloads => "search-overloads",
    case_move_overload_ambiguous => "move-overload-ambiguous",
    case_move_overload_selected => "move-overload-selected",
    case_move_wrapper => "move-wrapper",
);

#[cfg(feature = "rust")]
const RUST_LOCAL_IMPORTS: Corpus = Corpus {
    name: "rust-local-imports",
    cases: &[
        (
            "before-import",
            &["navigate", "src/lib.rs:7:5", "--compact"],
        ),
        ("after-import", &["navigate", "src/lib.rs:9:5", "--compact"]),
        (
            "alias-declaration",
            &["navigate", "src/lib.rs:8:26", "--compact"],
        ),
        ("chained", &["navigate", "src/lib.rs:14:5", "--compact"]),
        (
            "qualified-alias",
            &["navigate", "src/lib.rs:15:12", "--compact"],
        ),
        ("inner", &["navigate", "src/lib.rs:22:9", "--compact"]),
        (
            "outer-restored",
            &["navigate", "src/lib.rs:24:5", "--compact"],
        ),
        (
            "parameter-shadowed",
            &["navigate", "src/lib.rs:28:5", "--compact"],
        ),
        (
            "initializer",
            &["navigate", "src/lib.rs:32:16", "--compact"],
        ),
        (
            "local-shadow",
            &["navigate", "src/lib.rs:33:5", "--compact"],
        ),
        (
            "item-ambiguity",
            &["navigate", "src/lib.rs:38:5", "--compact"],
        ),
        (
            "import-ambiguity",
            &["navigate", "src/lib.rs:43:5", "--compact"],
        ),
        (
            "nested-item",
            &["navigate", "src/lib.rs:47:18", "--compact"],
        ),
        ("type", &["navigate", "src/lib.rs:52:12", "--compact"]),
        ("macro", &["navigate", "src/lib.rs:57:5", "--compact"]),
        (
            "rooted-macro",
            &["navigate", "src/lib.rs:58:15", "--compact"],
        ),
        ("glob", &["navigate", "src/lib.rs:62:5", "--compact"]),
        ("cycle", &["navigate", "src/lib.rs:67:5", "--compact"]),
        ("external", &["navigate", "src/lib.rs:71:5", "--compact"]),
        ("inline-self", &["navigate", "src/lib.rs:78:9", "--compact"]),
        (
            "inline-super",
            &["navigate", "src/lib.rs:79:9", "--compact"],
        ),
        ("isolated", &["navigate", "src/lib.rs:82:17", "--compact"]),
        (
            "selected",
            &["navigate", "src/lib.rs:43:5", "--compact", "--select", "2"],
        ),
        (
            "context",
            &["context", "src/lib.rs:14:5", "--detail", "signature"],
        ),
        ("callers", &["relationships", "callers", "src/lib.rs:2:12"]),
        (
            "constant-pattern",
            &["navigate", "src/lib.rs:86:9", "--compact"],
        ),
        (
            "constant-use",
            &["navigate", "src/lib.rs:87:13", "--compact"],
        ),
        ("namespace", &["navigate", "src/lib.rs:92:5", "--compact"]),
        (
            "grouped-call",
            &["navigate", "src/lib.rs:96:5", "--compact"],
        ),
        (
            "grouped-type",
            &["navigate", "src/lib.rs:97:12", "--compact"],
        ),
        ("closure", &["navigate", "src/lib.rs:101:16", "--compact"]),
        (
            "grouped-alias-call",
            &["navigate", "src/lib.rs:106:5", "--compact"],
        ),
        (
            "grouped-alias-type",
            &["navigate", "src/lib.rs:107:12", "--compact"],
        ),
        (
            "import-owner",
            &["navigate", "src/lib.rs:114:9", "--compact"],
        ),
        (
            "type-alias-declaration",
            &["navigate", "src/lib.rs:91:28", "--compact"],
        ),
        ("unplaced-file", &["navigate", "loose.rs:4:5", "--compact"]),
        (
            "let-else-initializer",
            &["navigate", "src/lib.rs:120:23", "--compact"],
        ),
        (
            "let-else-outer",
            &["navigate", "src/lib.rs:121:17", "--compact"],
        ),
        (
            "let-else-import-in-else",
            &["navigate", "src/lib.rs:122:9", "--compact"],
        ),
        (
            "let-else-binding",
            &["navigate", "src/lib.rs:125:13", "--compact"],
        ),
        (
            "let-else-import-after",
            &["navigate", "src/lib.rs:126:5", "--compact"],
        ),
    ],
    compositions: &[],
    mutations: || vec![],
};

#[cfg(feature = "rust")]
corpus_cases!(rust_local_imports, RUST_LOCAL_IMPORTS;
    case_before_import => "before-import",
    case_after_import => "after-import",
    case_alias_declaration => "alias-declaration",
    case_chained => "chained",
    case_qualified_alias => "qualified-alias",
    case_inner => "inner",
    case_outer_restored => "outer-restored",
    case_parameter_shadowed => "parameter-shadowed",
    case_initializer => "initializer",
    case_local_shadow => "local-shadow",
    case_item_ambiguity => "item-ambiguity",
    case_import_ambiguity => "import-ambiguity",
    case_nested_item => "nested-item",
    case_type => "type",
    case_macro => "macro",
    case_rooted_macro => "rooted-macro",
    case_glob => "glob",
    case_cycle => "cycle",
    case_external => "external",
    case_inline_self => "inline-self",
    case_inline_super => "inline-super",
    case_isolated => "isolated",
    case_selected => "selected",
    case_context => "context",
    case_callers => "callers",
    case_constant_pattern => "constant-pattern",
    case_constant_use => "constant-use",
    case_namespace => "namespace",
    case_grouped_call => "grouped-call",
    case_grouped_type => "grouped-type",
    case_closure => "closure",
    case_grouped_alias_call => "grouped-alias-call",
    case_grouped_alias_type => "grouped-alias-type",
    case_import_owner => "import-owner",
    case_type_alias_declaration => "type-alias-declaration",
    case_unplaced_file => "unplaced-file",
    case_let_else_initializer => "let-else-initializer",
    case_let_else_outer => "let-else-outer",
    case_let_else_import_in_else => "let-else-import-in-else",
    case_let_else_binding => "let-else-binding",
    case_let_else_import_after => "let-else-import-after",
);

#[cfg(feature = "rust")]
const RUST_CONDITIONALS: Corpus = Corpus {
    name: "rust-conditionals",
    cases: &[
        ("scrutinee", &["navigate", "src/lib.rs:7:26", "--compact"]),
        ("success", &["navigate", "src/lib.rs:8:17", "--compact"]),
        ("failure", &["navigate", "src/lib.rs:10:17", "--compact"]),
        ("after", &["navigate", "src/lib.rs:12:13", "--compact"]),
        (
            "chain-first",
            &["navigate", "src/lib.rs:16:12", "--compact"],
        ),
        (
            "chain-next-initializer",
            &["navigate", "src/lib.rs:17:35", "--compact"],
        ),
        (
            "chain-second",
            &["navigate", "src/lib.rs:18:12", "--compact"],
        ),
        (
            "chain-success",
            &["navigate", "src/lib.rs:20:17", "--compact"],
        ),
        (
            "chain-failure",
            &["navigate", "src/lib.rs:22:17", "--compact"],
        ),
        (
            "chain-after",
            &["navigate", "src/lib.rs:24:13", "--compact"],
        ),
        (
            "boolean-import",
            &["navigate", "src/lib.rs:29:9", "--compact"],
        ),
        (
            "boolean-type",
            &["navigate", "src/lib.rs:30:28", "--compact"],
        ),
        (
            "boolean-else-import",
            &["navigate", "src/lib.rs:32:9", "--compact"],
        ),
        (
            "pattern-import",
            &["navigate", "src/lib.rs:35:9", "--compact"],
        ),
        (
            "pattern-local",
            &["navigate", "src/lib.rs:36:17", "--compact"],
        ),
        ("capture", &["navigate", "src/lib.rs:37:20", "--compact"]),
        ("noncapture", &["navigate", "src/lib.rs:38:31", "--compact"]),
        (
            "callable-binding",
            &["navigate", "src/lib.rs:43:9", "--compact"],
        ),
        (
            "inner-import",
            &["navigate", "src/lib.rs:46:13", "--compact"],
        ),
        (
            "first-alternative",
            &["navigate", "src/lib.rs:52:17", "--compact"],
        ),
        (
            "second-alternative",
            &["navigate", "src/lib.rs:54:17", "--compact"],
        ),
        (
            "final-alternative",
            &["navigate", "src/lib.rs:56:17", "--compact"],
        ),
        (
            "struct-success",
            &["navigate", "src/lib.rs:61:17", "--compact"],
        ),
        (
            "struct-else",
            &["navigate", "src/lib.rs:63:17", "--compact"],
        ),
        (
            "struct-after",
            &["navigate", "src/lib.rs:65:13", "--compact"],
        ),
        (
            "macro-outer",
            &["navigate", "src/lib.rs:71:17", "--compact"],
        ),
        (
            "macro-inner",
            &["navigate", "src/lib.rs:73:17", "--compact"],
        ),
        (
            "macro-rooted",
            &["navigate", "src/lib.rs:74:21", "--compact"],
        ),
        (
            "constant-pattern",
            &["navigate", "src/lib.rs:79:17", "--compact"],
        ),
        (
            "constant-use",
            &["navigate", "src/lib.rs:80:17", "--compact"],
        ),
        (
            "loop-barrier",
            &["navigate", "src/lib.rs:85:30", "--compact"],
        ),
        ("context", &["context", "src/lib.rs:20:17"]),
        ("callers", &["relationships", "callers", "src/lib.rs:2:12"]),
        (
            "module-constant-pattern",
            &["navigate", "src/lib.rs:91:17", "--compact"],
        ),
        (
            "module-constant-use",
            &["navigate", "src/lib.rs:92:17", "--compact"],
        ),
        (
            "module-imported-pattern",
            &["navigate", "src/lib.rs:97:17", "--compact"],
        ),
        (
            "module-imported-use",
            &["navigate", "src/lib.rs:98:17", "--compact"],
        ),
        (
            "import-item-ambiguity",
            &["navigate", "src/lib.rs:105:9", "--compact"],
        ),
        (
            "import-item-ambiguity-selected",
            &["navigate", "src/lib.rs:105:9", "--compact", "--select", "2"],
        ),
        (
            "pattern-ambiguity",
            &["navigate", "src/lib.rs:110:17", "--compact"],
        ),
        (
            "pattern-ambiguity-selected",
            &[
                "navigate",
                "src/lib.rs:110:17",
                "--compact",
                "--select",
                "2",
            ],
        ),
    ],
    compositions: &[],
    mutations: || vec![],
};

#[cfg(feature = "rust")]
corpus_cases!(rust_conditionals, RUST_CONDITIONALS;
    case_scrutinee => "scrutinee",
    case_success => "success",
    case_failure => "failure",
    case_after => "after",
    case_chain_first => "chain-first",
    case_chain_next_initializer => "chain-next-initializer",
    case_chain_second => "chain-second",
    case_chain_success => "chain-success",
    case_chain_failure => "chain-failure",
    case_chain_after => "chain-after",
    case_boolean_import => "boolean-import",
    case_boolean_type => "boolean-type",
    case_boolean_else_import => "boolean-else-import",
    case_pattern_import => "pattern-import",
    case_pattern_local => "pattern-local",
    case_capture => "capture",
    case_noncapture => "noncapture",
    case_callable_binding => "callable-binding",
    case_inner_import => "inner-import",
    case_first_alternative => "first-alternative",
    case_second_alternative => "second-alternative",
    case_final_alternative => "final-alternative",
    case_struct_success => "struct-success",
    case_struct_else => "struct-else",
    case_struct_after => "struct-after",
    case_macro_outer => "macro-outer",
    case_macro_inner => "macro-inner",
    case_macro_rooted => "macro-rooted",
    case_constant_pattern => "constant-pattern",
    case_constant_use => "constant-use",
    case_loop_barrier => "loop-barrier",
    case_context => "context",
    case_callers => "callers",
    case_module_constant_pattern => "module-constant-pattern",
    case_module_constant_use => "module-constant-use",
    case_module_imported_pattern => "module-imported-pattern",
    case_module_imported_use => "module-imported-use",
    case_import_item_ambiguity => "import-item-ambiguity",
    case_import_item_ambiguity_selected => "import-item-ambiguity-selected",
    case_pattern_ambiguity => "pattern-ambiguity",
    case_pattern_ambiguity_selected => "pattern-ambiguity-selected",
);

#[cfg(feature = "rust")]
const CASES_RUST_CONSTRUCTS: Corpus = Corpus {
    name: "rust-constructs",
    cases: &[
        ("arm-body", &["navigate", "src/lib.rs:5:18", "--compact"]),
        ("arm-capture", &["navigate", "src/lib.rs:6:35", "--compact"]),
        (
            "arm-nested-item",
            &["navigate", "src/lib.rs:7:32", "--compact"],
        ),
        ("other-arm", &["navigate", "src/lib.rs:9:19", "--compact"]),
        (
            "after-match",
            &["navigate", "src/lib.rs:11:10", "--compact"],
        ),
        ("for-body", &["navigate", "src/lib.rs:13:14", "--compact"]),
        ("after-for", &["navigate", "src/lib.rs:15:10", "--compact"]),
        (
            "while-let-body",
            &["navigate", "src/lib.rs:17:14", "--compact"],
        ),
        (
            "after-while-let",
            &["navigate", "src/lib.rs:19:10", "--compact"],
        ),
        (
            "while-condition",
            &["navigate", "src/lib.rs:20:28", "--compact"],
        ),
        ("loop-body", &["navigate", "src/lib.rs:21:17", "--compact"]),
        (
            "closure-body",
            &["navigate", "src/lib.rs:22:40", "--compact"],
        ),
        (
            "after-closure",
            &["navigate", "src/lib.rs:23:10", "--compact"],
        ),
        ("struct-arm", &["navigate", "src/lib.rs:25:39", "--compact"]),
        (
            "supported-peer-arm",
            &["navigate", "src/lib.rs:26:19", "--compact"],
        ),
        (
            "after-struct-match",
            &["navigate", "src/lib.rs:28:10", "--compact"],
        ),
        (
            "for-iterator",
            &["navigate", "src/lib.rs:31:25", "--compact"],
        ),
        (
            "while-chain-initializer",
            &["navigate", "src/lib.rs:32:74", "--compact"],
        ),
        (
            "while-chain-body",
            &["navigate", "src/lib.rs:33:14", "--compact"],
        ),
        (
            "match-guard",
            &["navigate", "src/lib.rs:36:24", "--compact"],
        ),
        ("guard-peer", &["navigate", "src/lib.rs:37:19", "--compact"]),
        (
            "after-guard",
            &["navigate", "src/lib.rs:39:10", "--compact"],
        ),
        ("struct-for", &["navigate", "src/lib.rs:40:51", "--compact"]),
        (
            "after-struct-for",
            &["navigate", "src/lib.rs:41:10", "--compact"],
        ),
        (
            "arm-macro-explicit",
            &["navigate", "src/lib.rs:43:47", "--compact"],
        ),
        ("macro-peer", &["navigate", "src/lib.rs:44:19", "--compact"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_constructs, CASES_RUST_CONSTRUCTS;
    case_arm_body => "arm-body",
    case_arm_capture => "arm-capture",
    case_arm_nested_item => "arm-nested-item",
    case_other_arm => "other-arm",
    case_after_match => "after-match",
    case_for_body => "for-body",
    case_after_for => "after-for",
    case_while_let_body => "while-let-body",
    case_after_while_let => "after-while-let",
    case_while_condition => "while-condition",
    case_loop_body => "loop-body",
    case_closure_body => "closure-body",
    case_after_closure => "after-closure",
    case_struct_arm => "struct-arm",
    case_supported_peer_arm => "supported-peer-arm",
    case_after_struct_match => "after-struct-match",
    case_for_iterator => "for-iterator",
    case_while_chain_initializer => "while-chain-initializer",
    case_while_chain_body => "while-chain-body",
    case_match_guard => "match-guard",
    case_guard_peer => "guard-peer",
    case_after_guard => "after-guard",
    case_struct_for => "struct-for",
    case_after_struct_for => "after-struct-for",
    case_arm_macro_explicit => "arm-macro-explicit",
    case_macro_peer => "macro-peer",
);

#[cfg(feature = "rust")]
const CASES_RUST_STRUCT_PATTERNS: Corpus = Corpus {
    name: "rust-struct-patterns",
    cases: &[
        ("shorthand", &["navigate", "src/lib.rs:4:17", "--compact"]),
        ("field-label", &["navigate", "src/lib.rs:4:20", "--compact"]),
        (
            "renamed-declaration",
            &["navigate", "src/lib.rs:4:23", "--compact"],
        ),
        (
            "local-shorthand",
            &["navigate", "src/lib.rs:5:10", "--compact"],
        ),
        (
            "local-renamed",
            &["navigate", "src/lib.rs:6:10", "--compact"],
        ),
        (
            "condition-shorthand",
            &["navigate", "src/lib.rs:8:14", "--compact"],
        ),
        (
            "condition-ref",
            &["navigate", "src/lib.rs:9:17", "--compact"],
        ),
        ("else-outer", &["navigate", "src/lib.rs:10:19", "--compact"]),
        ("after-if", &["navigate", "src/lib.rs:11:10", "--compact"]),
        (
            "arm-renamed",
            &["navigate", "src/lib.rs:13:56", "--compact"],
        ),
        ("arm-ref", &["navigate", "src/lib.rs:13:70", "--compact"]),
        ("peer-arm", &["navigate", "src/lib.rs:14:19", "--compact"]),
        (
            "for-binding",
            &["navigate", "src/lib.rs:16:47", "--compact"],
        ),
        (
            "while-binding",
            &["navigate", "src/lib.rs:17:58", "--compact"],
        ),
        (
            "closure-binding",
            &["navigate", "src/lib.rs:18:59", "--compact"],
        ),
        (
            "let-else-failure",
            &["navigate", "src/lib.rs:19:58", "--compact"],
        ),
        (
            "let-else-success",
            &["navigate", "src/lib.rs:20:10", "--compact"],
        ),
        ("parameter", &["navigate", "src/lib.rs:22:52", "--compact"]),
        (
            "nested-capture",
            &["navigate", "src/lib.rs:24:67", "--compact"],
        ),
        (
            "after-capture",
            &["navigate", "src/lib.rs:25:10", "--compact"],
        ),
        ("duplicate", &["navigate", "src/lib.rs:29:10", "--compact"]),
        (
            "duplicate-selected",
            &["navigate", "src/lib.rs:29:10", "--compact", "--select", "2"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_struct_patterns, CASES_RUST_STRUCT_PATTERNS;
    case_shorthand => "shorthand",
    case_field_label => "field-label",
    case_renamed_declaration => "renamed-declaration",
    case_local_shorthand => "local-shorthand",
    case_local_renamed => "local-renamed",
    case_condition_shorthand => "condition-shorthand",
    case_condition_ref => "condition-ref",
    case_else_outer => "else-outer",
    case_after_if => "after-if",
    case_arm_renamed => "arm-renamed",
    case_arm_ref => "arm-ref",
    case_peer_arm => "peer-arm",
    case_for_binding => "for-binding",
    case_while_binding => "while-binding",
    case_closure_binding => "closure-binding",
    case_let_else_failure => "let-else-failure",
    case_let_else_success => "let-else-success",
    case_parameter => "parameter",
    case_nested_capture => "nested-capture",
    case_after_capture => "after-capture",
    case_duplicate => "duplicate",
    case_duplicate_selected => "duplicate-selected",
);

#[cfg(feature = "rust")]
const CASES_RUST_PATTERNS: Corpus = Corpus {
    name: "rust-patterns",
    cases: &[
        (
            "alternatives",
            &["navigate", "src/lib.rs:4:44", "--compact"],
        ),
        ("capture", &["navigate", "src/lib.rs:5:39", "--compact"]),
        (
            "capture-whole",
            &["navigate", "src/lib.rs:5:55", "--compact"],
        ),
        (
            "literal-range",
            &["navigate", "src/lib.rs:6:33", "--compact"],
        ),
        (
            "invalid-peer",
            &["navigate", "src/lib.rs:7:43", "--compact"],
        ),
        ("rest-first", &["navigate", "src/lib.rs:11:10", "--compact"]),
        ("rest-tail", &["navigate", "src/lib.rs:11:26", "--compact"]),
        ("rest-last", &["navigate", "src/lib.rs:11:37", "--compact"]),
        (
            "conditional",
            &["navigate", "src/lib.rs:12:47", "--compact"],
        ),
        ("after", &["navigate", "src/lib.rs:13:10", "--compact"]),
        (
            "alternative-selected",
            &["navigate", "src/lib.rs:4:44", "--compact", "--select", "2"],
        ),
        (
            "range-endpoint",
            &["navigate", "src/lib.rs:16:31", "--compact"],
        ),
        (
            "range-capture",
            &["navigate", "src/lib.rs:16:45", "--compact"],
        ),
        ("range-peer", &["navigate", "src/lib.rs:16:63", "--compact"]),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_patterns, CASES_RUST_PATTERNS;
    case_alternatives => "alternatives",
    case_capture => "capture",
    case_capture_whole => "capture-whole",
    case_literal_range => "literal-range",
    case_invalid_peer => "invalid-peer",
    case_rest_first => "rest-first",
    case_rest_tail => "rest-tail",
    case_rest_last => "rest-last",
    case_conditional => "conditional",
    case_after => "after",
    case_alternative_selected => "alternative-selected",
    case_range_endpoint => "range-endpoint",
    case_range_capture => "range-capture",
    case_range_peer => "range-peer",
);

#[cfg(feature = "rust")]
const CASES_RUST_PATTERN_REFERENCES: Corpus = Corpus {
    name: "rust-pattern-references",
    cases: &[
        ("range-min", &["navigate", "src/lib.rs:13:9", "--compact"]),
        ("range-max", &["navigate", "src/lib.rs:13:15", "--compact"]),
        ("range-body", &["navigate", "src/lib.rs:13:27", "--compact"]),
        (
            "constant-first",
            &["navigate", "src/lib.rs:14:9", "--compact"],
        ),
        (
            "constant-second",
            &["navigate", "src/lib.rs:14:15", "--compact"],
        ),
        (
            "constant-body",
            &["navigate", "src/lib.rs:14:27", "--compact"],
        ),
        ("unit-import", &["navigate", "src/lib.rs:15:9", "--compact"]),
        (
            "unit-qualified",
            &["navigate", "src/lib.rs:15:25", "--compact"],
        ),
        ("unit-body", &["navigate", "src/lib.rs:15:38", "--compact"]),
        ("tuple-alias", &["navigate", "src/lib.rs:16:9", "--compact"]),
        (
            "tuple-binding",
            &["navigate", "src/lib.rs:16:14", "--compact"],
        ),
        (
            "record-constructor",
            &["navigate", "src/lib.rs:17:9", "--compact"],
        ),
        (
            "record-binding",
            &["navigate", "src/lib.rs:17:36", "--compact"],
        ),
        (
            "variant-alias",
            &["navigate", "src/lib.rs:18:9", "--compact"],
        ),
        (
            "variant-qualified",
            &["navigate", "src/lib.rs:18:35", "--compact"],
        ),
        (
            "variant-binding",
            &["navigate", "src/lib.rs:18:55", "--compact"],
        ),
        (
            "record-variant",
            &["navigate", "src/lib.rs:19:18", "--compact"],
        ),
        (
            "record-variant-binding",
            &["navigate", "src/lib.rs:19:46", "--compact"],
        ),
        (
            "incompatible-body",
            &["navigate", "src/lib.rs:20:29", "--compact"],
        ),
        (
            "outer-local",
            &["navigate", "src/lib.rs:21:19", "--compact"],
        ),
        (
            "local-endpoint",
            &["navigate", "src/lib.rs:26:23", "--compact"],
        ),
        (
            "macro-constant",
            &["navigate", "src/lib.rs:30:19", "--compact"],
        ),
        (
            "rooted-endpoint",
            &["navigate", "src/lib.rs:30:60", "--compact"],
        ),
        (
            "reexport-bound",
            &["navigate", "src/lib.rs:36:19", "--compact"],
        ),
        (
            "reexport-constructor",
            &["navigate", "src/lib.rs:36:46", "--compact"],
        ),
        (
            "reexport-binding",
            &["navigate", "src/lib.rs:36:68", "--compact"],
        ),
        (
            "commented-bound",
            &["navigate", "src/lib.rs:39:51", "--compact"],
        ),
        (
            "spaced-bound",
            &["navigate", "src/lib.rs:39:67", "--compact"],
        ),
    ],
    compositions: &[],
    mutations: Vec::new,
};

#[cfg(feature = "rust")]
corpus_cases!(rust_pattern_references, CASES_RUST_PATTERN_REFERENCES;
    case_range_min => "range-min",
    case_range_max => "range-max",
    case_range_body => "range-body",
    case_constant_first => "constant-first",
    case_constant_second => "constant-second",
    case_constant_body => "constant-body",
    case_unit_import => "unit-import",
    case_unit_qualified => "unit-qualified",
    case_unit_body => "unit-body",
    case_tuple_alias => "tuple-alias",
    case_tuple_binding => "tuple-binding",
    case_record_constructor => "record-constructor",
    case_record_binding => "record-binding",
    case_variant_alias => "variant-alias",
    case_variant_qualified => "variant-qualified",
    case_variant_binding => "variant-binding",
    case_record_variant => "record-variant",
    case_record_variant_binding => "record-variant-binding",
    case_incompatible_body => "incompatible-body",
    case_outer_local => "outer-local",
    case_local_endpoint => "local-endpoint",
    case_macro_constant => "macro-constant",
    case_rooted_endpoint => "rooted-endpoint",
    case_reexport_bound => "reexport-bound",
    case_reexport_constructor => "reexport-constructor",
    case_reexport_binding => "reexport-binding",
    case_commented_bound => "commented-bound",
    case_spaced_bound => "spaced-bound",
);

#[cfg(feature = "rust")]
#[test]
fn rust_incoming_relationship_pages_equal_the_complete_scan() {
    PageTranscript::relationships(
        Corpus {
            name: "rust-relationships",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        0,
        7,
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_incoming_relationship_pages_equal_the_complete_scan() {
    PageTranscript::relationships(
        Corpus {
            name: "ts-relationships",
            cases: &[],
            compositions: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        0,
        16,
    );
}
