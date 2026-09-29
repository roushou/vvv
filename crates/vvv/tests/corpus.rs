//! Corpus workspaces under `tests/corpus/` cover Rust, TypeScript, moves, and
//! import resolution. Command cases retain exact human and JSON snapshots.
//! Mutation cases check three properties: apply equals preview, undo restores
//! the original tree, and batch equals sequential application.
//! Changing what a command means changes a snapshot; review it, then
//! `INSTA_UPDATE=always cargo test -p vvv-rs --test corpus` to accept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use vvv_engine::{
    Answer, Edit, Engine, FileChange, Intent, MemoryVfs, MoveIntent, MoveSymbolIntent, Query,
    RenameIntent, Request, Retention, RewriteIntent, Vfs, Workspace,
};
use vvv_rs::Builtins;

/// One corpus: its directory and every case run against it.
struct Corpus {
    name: &'static str,
    /// `(case name, arguments)`; `--json` and the human form are both kept.
    cases: &'static [(&'static str, &'static [&'static str])],
    /// Mutations the properties hold over, as requests without `apply`.
    mutations: fn() -> Vec<Request>,
}

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
    mutations: Vec::new,
};

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
    mutations: || {
        vec![
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

impl Corpus {
    fn dir(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus")
            .join(self.name)
    }

    /// The binary's exact output for `args`, stdout then stderr, with the
    /// exit code; paths spelled with `/` whatever the host writes.
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
        format!(
            "exit {}\n--- stdout ---\n{}--- stderr ---\n{}",
            output.status.code().unwrap_or(-1),
            text(output.stdout),
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

fn golden(corpus: &Corpus) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("corpus/snapshots");
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        for (case, args) in corpus.cases {
            insta::assert_snapshot!(
                format!("{}__{case}__json", corpus.name),
                corpus.run(args, true)
            );
            insta::assert_snapshot!(
                format!("{}__{case}__human", corpus.name),
                corpus.run(args, false)
            );
        }
    });
}

/// What is applied is exactly what was previewed: each previewed file, with
/// its edits made, is what the tree holds afterwards, at the path the
/// preview said; nothing else changed.
fn apply_is_preview(corpus: &Corpus) {
    for request in (corpus.mutations)() {
        let (vfs, engine) = corpus.engine();
        let before = snapshot(&vfs);
        let preview = engine.run(request.clone()).unwrap().into_answer();
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
        engine.run(applying(&request)).unwrap();
        let after = snapshot(&vfs);
        for (path, text) in &expected {
            assert_eq!(
                after.get(path),
                Some(text),
                "{request:?}: {} differs from its preview",
                path.display()
            );
        }
        let touched: Vec<&PathBuf> = after
            .iter()
            .filter(|(path, text)| before.get(*path) != Some(*text))
            .map(|(path, _)| path)
            .collect();
        let previewed: Vec<PathBuf> = expected
            .iter()
            .filter(|(path, text)| before.get(*path) != Some(*text))
            .map(|(path, _)| path.clone())
            .collect();
        assert_eq!(
            touched,
            previewed.iter().collect::<Vec<_>>(),
            "{request:?}: only previewed files change"
        );
    }
}

/// An apply undone leaves every file as it was.
fn undo_is_identity(corpus: &Corpus) {
    for request in (corpus.mutations)() {
        let (vfs, engine) = corpus.engine();
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

/// A batch of two applied is the second applied after the first, and the
/// one undo reverts both.
fn batch_is_composition(corpus: &Corpus) {
    let mutations = (corpus.mutations)();
    let mut checked = 0;
    for pair in mutations.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        let (vfs, engine) = corpus.engine();
        let before = snapshot(&vfs);
        engine.run(applying(first)).unwrap();
        // The second may no longer apply after the first (its file moved).
        if engine.run(applying(second)).is_err() {
            continue;
        }
        let composed = snapshot(&vfs);

        let (vfs, engine) = corpus.engine();
        let batch = Request::Batch {
            intent: vvv_engine::BatchIntent::new([intent_of(first), intent_of(second)]),
            apply: true,
        };
        engine.run(batch).unwrap();
        assert_eq!(snapshot(&vfs), composed, "{first:?} then {second:?}");
        engine.run(Request::Undo).unwrap();
        assert_eq!(snapshot(&vfs), before, "one undo reverts the batch");
        checked += 1;
    }
    assert!(checked > 0, "some pair composes");
}

#[test]
fn rust_golden() {
    golden(&RUST);
}

#[test]
fn ts_golden() {
    golden(&TS);
}

#[test]
fn rust_apply_is_preview() {
    apply_is_preview(&RUST);
}

#[test]
fn ts_apply_is_preview() {
    apply_is_preview(&TS);
}

#[test]
fn rust_undo_is_identity() {
    undo_is_identity(&RUST);
}

#[test]
fn ts_undo_is_identity() {
    undo_is_identity(&TS);
}

#[test]
fn rust_batch_is_composition() {
    batch_is_composition(&RUST);
}

#[test]
fn ts_batch_is_composition() {
    batch_is_composition(&TS);
}

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
#[test]
fn rust_moves_golden() {
    golden(&RUST_MOVES);
}

#[cfg(feature = "rust")]
#[test]
fn rust_moves_apply_is_preview() {
    apply_is_preview(&RUST_MOVES);
}

#[cfg(feature = "rust")]
#[test]
fn rust_moves_undo_is_identity() {
    undo_is_identity(&RUST_MOVES);
}

#[cfg(feature = "rust")]
#[test]
fn rust_moves_batch_is_composition() {
    batch_is_composition(&RUST_MOVES);
}

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
#[test]
fn rust_resolution_golden() {
    golden(&RUST_RESOLUTION);
}

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
#[ignore = "known limitation: cross-file private parent-module aliases are not followed"]
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
#[test]
fn rust_navigation_golden() {
    golden(&Corpus {
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
        mutations: Vec::new,
    });
}

#[cfg(feature = "typescript")]
#[test]
fn ts_navigation_golden() {
    golden(&Corpus {
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
        mutations: Vec::new,
    });
}

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
    #[cfg(unix)]
    fn validation(corpus: Corpus, path: &str, symbol: &str) {
        let disk = ValidationCorpus::new(&corpus);
        let engine = Engine::new(Workspace::disk(&disk.root).unwrap(), Builtins::registry());
        let prepared = vvv_engine::PrepareRenameQuery {
            intent: RenameIntent::new("Engine", "Runtime")
                .declared_in(path)
                .of_symbol(symbol.parse().unwrap()),
            max_bytes: 65536,
        }
        .execute(&engine)
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
                serde_json::to_string_pretty(&transcript.replies).unwrap()
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
                serde_json::to_string_pretty(&transcript.replies).unwrap()
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
                serde_json::to_string_pretty(&transcript.replies).unwrap()
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
            mutations: Vec::new,
        },
        "src/origin.ts",
        0,
        13,
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_relationships_golden() {
    golden(&Corpus {
        name: "rust-relationships",
        cases: &[
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
        mutations: Vec::new,
    });
}
#[cfg(feature = "typescript")]
#[test]
fn ts_relationships_golden() {
    golden(&Corpus {
        name: "ts-relationships",
        cases: &[
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
        mutations: Vec::new,
    });
}

#[cfg(feature = "rust")]
#[test]
fn rust_retained_plan_golden() {
    PageTranscript::plans(
        Corpus {
            name: "rust-navigation",
            cases: &[],
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
            mutations: Vec::new,
        },
        "src/origin.ts",
        "class",
    );
}

#[cfg(all(unix, any(feature = "rust", feature = "typescript")))]
struct ValidationCorpus {
    root: PathBuf,
}
#[cfg(all(unix, any(feature = "rust", feature = "typescript")))]
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
#[cfg(all(unix, any(feature = "rust", feature = "typescript")))]
impl Drop for ValidationCorpus {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[cfg(all(unix, any(feature = "rust", feature = "typescript")))]
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
#[cfg(all(unix, feature = "rust"))]
#[test]
fn rust_validation_golden() {
    PageTranscript::validation(
        Corpus {
            name: "rust-navigation",
            cases: &[],
            mutations: Vec::new,
        },
        "src/origin.rs",
        "struct",
    );
}
#[cfg(all(unix, feature = "typescript"))]
#[test]
fn typescript_validation_golden() {
    PageTranscript::validation(
        Corpus {
            name: "ts-navigation",
            cases: &[],
            mutations: Vec::new,
        },
        "src/origin.ts",
        "class",
    );
}
