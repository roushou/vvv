//! Re-exports followed everywhere: under another name, forward to the
//! declaration an import reaches, and by the per-file answers and a move.
//! `pub use <path>` re-exports in the fake language; `as <name>` binds
//! another name.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::Fake;
use vvv_core::{Address, Position};
use vvv_engine::{
    Apply, Confidence, DepsQuery, Engine, ExplainQuery, FileQuery, ImpactQuery, Languages,
    MemoryVfs, MoveSymbolIntent, RelPath, RenameIntent, SurfaceQuery, Workspace,
};

/// `foo` lives in `a/x.p`; `lib.p` offers it as `bar`, `d.p` offers `bar`
/// on again; `c.p` and `e.p` take it under that name.
fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/manifest.p", "")
        .with_file("/ws/a/x.p", "pub def foo")
        .with_file("/ws/lib.p", "pub use a/x.p/foo as bar")
        .with_file("/ws/d.p", "pub use lib.p/bar")
        .with_file("/ws/c.p", "use lib.p/bar\nbar")
        .with_file("/ws/e.p", "use d.p/bar\nbar");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

fn module(parts: &[&str]) -> Address {
    Address::new("ws", parts.iter().map(|p| (*p).to_owned()))
}

fn read(engine: &Engine, path: &str) -> String {
    engine
        .run(FileQuery {
            path: RelPath::from(path),
        })
        .unwrap()
        .text
}

#[test]
fn an_alias_under_another_name_is_followed_but_not_renamed() {
    let engine = engine();
    let surface = engine.run(SurfaceQuery { package: None }).unwrap();
    assert_eq!(surface.items.len(), 1);
    assert_eq!(
        surface.items[0].via,
        [module(&["lib.p", "bar"]), module(&["d.p", "bar"])]
    );
    assert_eq!(surface.items[0].importers, 4, "lib.p, d.p, c.p, e.p");

    let impact = engine
        .run(ImpactQuery {
            name: "foo".to_owned(),
            declared_in: None,
        })
        .unwrap();
    let rings: Vec<(String, u32)> = impact
        .consumers
        .iter()
        .map(|c| (c.path.display().to_string(), c.depth))
        .collect();
    assert_eq!(
        rings,
        [
            ("c.p".to_owned(), 1),
            ("d.p".to_owned(), 1),
            ("e.p".to_owned(), 1),
            ("lib.p".to_owned(), 1),
        ]
    );

    // The rename touches what spells `foo`; `bar` stays `bar`.
    let rename = engine.run(RenameIntent::new("foo", "qux")).unwrap();
    let sites: Vec<(String, Confidence)> = rename
        .occurrences
        .iter()
        .map(|o| (o.m.path.display().to_string(), o.confidence))
        .collect();
    assert_eq!(
        sites,
        [
            ("a/x.p".to_owned(), Confidence::Resolved),
            ("lib.p".to_owned(), Confidence::Resolved),
        ]
    );
    engine.run(Apply(rename)).unwrap();
    assert_eq!(read(&engine, "lib.p"), "pub use a/x.p/qux as bar");
    assert_eq!(read(&engine, "c.p"), "use lib.p/bar\nbar");
}

#[test]
fn deps_follow_imports_to_their_origin_and_count_importers_through_aliases() {
    let engine = engine();
    let deps = engine
        .run(DepsQuery {
            path: RelPath::from("e.p"),
        })
        .unwrap();
    let [dep] = deps.imports.as_slice() else {
        panic!("{:?}", deps.imports);
    };
    assert_eq!(dep.address, Some(module(&["d.p", "bar"])));
    assert_eq!(dep.origin, Some(module(&["a", "x.p", "foo"])));
    assert_eq!(dep.file, Some(RelPath::from("a/x.p")));

    let deps = engine
        .run(DepsQuery {
            path: RelPath::from("a/x.p"),
        })
        .unwrap();
    let importers: Vec<String> = deps
        .importers
        .iter()
        .map(|i| i.path.display().to_string())
        .collect();
    assert_eq!(importers, ["c.p", "d.p", "e.p", "lib.p"]);
}

#[test]
fn explain_on_an_import_says_where_it_comes_from() {
    let engine = engine();
    let explanation = engine
        .run(ExplainQuery {
            path: RelPath::from("e.p"),
            position: Position::new(0, 6),
        })
        .unwrap();
    assert!(explanation.symbol.is_none());
    let dep = explanation.import.expect("on the import");
    assert_eq!(dep.import.path.to_string(), "d.p/bar");
    assert_eq!(dep.origin, Some(module(&["a", "x.p", "foo"])));

    // On the declaration: every address it is offered at, and importers
    // through them.
    let explanation = engine
        .run(ExplainQuery {
            path: RelPath::from("a/x.p"),
            position: Position::new(0, 9),
        })
        .unwrap();
    assert_eq!(
        explanation.via,
        [module(&["lib.p", "bar"]), module(&["d.p", "bar"])]
    );
    assert_eq!(
        explanation.importers,
        ["c.p", "d.p", "e.p", "lib.p"].map(PathBuf::from)
    );
}

/// The re-exporting statement follows the declaration; what imports through
/// it keeps working untouched.
#[test]
fn a_symbol_move_rebases_the_reexport_and_leaves_its_takers_alone() {
    let vfs = MemoryVfs::new()
        .with_file("/ws/manifest.p", "")
        .with_file("/ws/a/x.p", "def foo")
        .with_file("/ws/a/z.p", "")
        .with_file("/ws/lib.p", "pub use a/x.p/foo")
        .with_file("/ws/c.p", "use lib.p/foo\nfoo");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    );
    let mv = engine
        .run(MoveSymbolIntent::new("foo", "a/x.p", "a/z.p"))
        .unwrap();
    assert!(mv.notices.is_empty(), "{:?}", mv.notices);
    engine.run(Apply(mv)).unwrap();
    assert_eq!(read(&engine, "lib.p"), "pub use a/z.p/foo");
    assert_eq!(read(&engine, "c.p"), "use lib.p/foo\nfoo", "untouched");
    assert_eq!(read(&engine, "a/x.p"), "");
    assert_eq!(read(&engine, "a/z.p"), "pub def foo\n");
}
