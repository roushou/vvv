//! Moving files with the shared fake language: `use <path>` imports,
//! path-component addresses, one side edit per move.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_engine::{
    Apply, Engine, EngineError, FileQuery, Languages, MemoryVfs, MoveIntent, MoveSymbolIntent,
    RelPath, Vfs, Workspace,
};

fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/manifest.p", "")
        .with_file("/ws/a/x.p", "use b/y.p\nuse c/z.p\n")
        .with_file("/ws/b/y.p", "use a/x.p\nuse {a/x.p}\n")
        .with_file("/ws/c/z.p", "use b/y.p\n");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

fn read(engine: &Engine, path: &str) -> String {
    FileQuery {
        path: RelPath::from(path),
    }
    .execute(engine)
    .unwrap()
    .text
}

fn exists(engine: &Engine, path: &str) -> bool {
    FileQuery {
        path: RelPath::from(path),
    }
    .execute(engine)
    .is_ok()
}

#[test]
fn rewrites_references_moves_the_file_and_reports_notices() {
    let engine = engine();
    let mv = MoveIntent::new("./a/x.p", "d/e/w.p").plan(&engine).unwrap();
    assert_eq!(mv.notices.len(), 1);
    assert_eq!(mv.notices[0].path, Path::new("b/y.p"));
    assert!(matches!(
        &mv.notices[0].kind,
        vvv_engine::NoticeKind::UnrewritableImport { import, replacement }
            if import == "a/x.p" && replacement == "d/e/w.p"
    ));

    Apply(mv).apply(&engine).unwrap();
    assert!(!exists(&engine, "a/x.p"));
    assert_eq!(read(&engine, "d/e/w.p"), "use b/y.p\nuse c/z.p\n");
    assert_eq!(read(&engine, "b/y.p"), "use d/e/w.p\nuse {a/x.p}\n");
    assert_eq!(read(&engine, "c/z.p"), "use b/y.p\n", "untouched");
    assert_eq!(read(&engine, "manifest.p"), "moved\n", "side edit applied");
}

#[test]
fn refuses_missing_source_existing_destination_and_foreign_extension() {
    let engine = engine();
    assert!(matches!(
        MoveIntent::new("nope.p", "x.p").plan(&engine),
        Err(EngineError::Vfs(_))
    ));
    assert!(matches!(
        MoveIntent::new("a/x.p", "b/y.p").plan(&engine),
        Err(EngineError::Exists(_))
    ));
    assert!(matches!(
        MoveIntent::new("a/x.p", "a/x.txt").plan(&engine),
        Err(EngineError::NoLanguage(_))
    ));
}

mod directory {
    use std::path::Path;
    use std::sync::Arc;

    use super::{exists, read};
    use vvv_core::ResolveError;
    use vvv_engine::{Apply, Engine, EngineError, Languages, MemoryVfs, MoveIntent, Workspace};

    use super::common::Fake;

    fn engine() -> Engine {
        let vfs = MemoryVfs::new()
            .with_file("/ws/manifest.p", "")
            .with_file("/ws/a/x.p", "use a/y.p\n")
            .with_file("/ws/a/deep/z.p", "use a/x.p\n")
            .with_file("/ws/b.p", "use a/x.p\nuse a/deep/z.p\n");
        Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        )
    }

    #[test]
    fn every_file_under_the_directory_moves_and_references_follow() {
        let engine = engine();
        let mv = MoveIntent::new("a", "d/a").plan(&engine).unwrap();
        let moves: Vec<(&Path, &Path)> = mv
            .files
            .iter()
            .filter_map(|f| Some((f.path.as_path(), f.moved_to.as_deref()?)))
            .collect();
        assert_eq!(
            moves,
            [
                (Path::new("a/deep/z.p"), Path::new("d/a/deep/z.p")),
                (Path::new("a/x.p"), Path::new("d/a/x.p")),
            ]
        );
        Apply(mv).apply(&engine).unwrap();
        assert_eq!(read(&engine, "b.p"), "use d/a/x.p\nuse d/a/deep/z.p\n");
        assert_eq!(
            read(&engine, "d/a/x.p"),
            "use d/a/y.p\n",
            "moved file's own reference"
        );
        assert!(!exists(&engine, "a/x.p"));
    }

    #[test]
    fn refuses_moving_into_itself_and_missing_directories() {
        let engine = engine();
        assert!(matches!(
            MoveIntent::new("a", "a/sub").plan(&engine),
            Err(EngineError::Resolve(ResolveError::IntoItself { .. }))
        ));
        assert!(matches!(
            MoveIntent::new("nope", "x").plan(&engine),
            Err(EngineError::NoLanguage(_) | EngineError::Vfs(_))
        ));
    }
}

/// The fake's declarations are private to their file (no modifier, default
/// `Declaring`). A file that imports a declaration by path can only see it
/// while inside the declaring file's module; once the declaration moves, the
/// engine judges the reference and widens the declaration — the fake spells
/// every widening as `pub `.
#[test]
fn a_move_widens_what_its_consumers_can_no_longer_see() {
    let vfs = MemoryVfs::new()
        .with_file("/ws/manifest.p", "")
        .with_file("/ws/lib.p", "use a/x.p/helper\nhelper")
        .with_file("/ws/a/x.p", "def helper\ndef local\nlocal");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    );
    let mv = MoveIntent::new("a/x.p", "b/y.p").plan(&engine).unwrap();
    assert!(mv.notices.is_empty(), "{:?}", mv.notices);
    Apply(mv).apply(&engine).unwrap();
    assert_eq!(read(&engine, "lib.p"), "use b/y.p/helper\nhelper");
    assert_eq!(
        read(&engine, "b/y.p"),
        "pub def helper\ndef local\nlocal",
        "only what an outside consumer names is widened"
    );
}

/// One declaration moves between files: its text is cut and pasted, every
/// consumer points at the new address, the old file imports it if it still
/// uses it, the new file drops its now-redundant import, and the declaration
/// is widened for the consumers that could no longer see it.
#[test]
fn a_symbol_moves_with_its_consumers_following() {
    let vfs = MemoryVfs::new()
        .with_file("/ws/manifest.p", "")
        .with_file("/ws/a/x.p", "def helper\ndef other\nhelper")
        .with_file("/ws/b/y.p", "use a/x.p/helper\nhelper")
        .with_file("/ws/lib.p", "use a/x.p/helper\nhelper");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    );
    let intent = MoveSymbolIntent::new("helper", "a/x.p", "b/y.p");
    let mv = intent.clone().plan(&engine).unwrap();
    assert!(mv.notices.is_empty(), "{:?}", mv.notices);
    assert_eq!(
        mv.from,
        vvv_core::Address::new("ws", ["a", "x.p", "helper"])
    );
    assert_eq!(mv.to, vvv_core::Address::new("ws", ["b", "y.p", "helper"]));
    Apply(mv).apply(&engine).unwrap();
    assert_eq!(
        read(&engine, "lib.p"),
        "use b/y.p/helper\nhelper",
        "consumer rewritten"
    );
    assert_eq!(
        read(&engine, "a/x.p"),
        "use b/y.p/helper\ndef other\nhelper",
        "cut, and imported back where the old file still uses it"
    );
    assert_eq!(
        read(&engine, "b/y.p"),
        "helper\n\npub def helper\n",
        "redundant import gone, pasted at the end, widened for its consumers"
    );
    vvv_engine::Ledger::new(&engine).undo().unwrap();
    assert_eq!(read(&engine, "a/x.p"), "def helper\ndef other\nhelper");
}

#[test]
fn grouped_rewrites_must_account_for_every_requested_entry_before_writes() {
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/manifest.p", "")
            .with_file("/ws/a.p", "def x\n")
            .with_file("/ws/consumer.p", "use {a.p}\n"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(Fake::default().with_regrouped(vvv_core::Regrouped::default())),
    );
    let error = match MoveIntent::new("a.p", "b.p").plan(&engine) {
        Err(error) => error,
        Ok(_) => panic!("an omitted grouped outcome must be rejected"),
    };
    assert!(matches!(
        error,
        EngineError::Regroup(vvv_core::RegroupError::MissingOutcome(_))
    ));
    assert_eq!(error.code(), vvv_engine::ErrorCode::Conflict);
    assert_eq!(vfs.read(Path::new("/ws/a.p")).unwrap(), "def x\n");
    assert_eq!(
        vfs.read(Path::new("/ws/consumer.p")).unwrap(),
        "use {a.p}\n"
    );
    assert!(!vfs.exists(Path::new("/ws/b.p")));
}
