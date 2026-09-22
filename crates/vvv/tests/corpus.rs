//! The corpus gate: two small workspaces under `tests/corpus/` — one Rust
//! workspace of two crates, one TypeScript project — with every command's
//! exact output kept as a snapshot, and three properties every mutation must
//! keep: what is applied is what was previewed, an apply undone leaves the
//! tree as it was, and a batch of two is the second applied after the first.
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
        // Windows spells its path separator as `\`, which JSON escapes as
        // two characters; the snapshots are taken with `/`.
        let text = |bytes: Vec<u8>| {
            let text = String::from_utf8(bytes).unwrap();
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

/// `text` with `edits` made, whatever order they came in.
fn edited(text: &str, edits: &[Edit]) -> String {
    let mut edits: Vec<&Edit> = edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = text.to_owned();
    for edit in edits {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    out
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
        let preview = engine.run(request.clone()).unwrap();
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
            expected.insert(to, edited(&original, &change.edits));
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
