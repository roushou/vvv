//! The questions about the whole tree: what a package offers, who depends on
//! a declaration, what nothing refers to, which imports are worth a look.
//! Asked of the fake language, whose `pub use` re-exports and whose `ext/`
//! package is unknowable.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Fake;
use vvv_core::Address;
use vvv_engine::{
    DeadQuery, Engine, ImpactQuery, ImportsQuery, Languages, MemoryVfs, RelPath, Retention,
    SurfaceQuery, Vfs, Workspace,
};

/// `foo` is declared in `a/x.p`, re-exported by `lib.p`, taken by `b/y.p`
/// both ways; `c/w.p` only imports `b/y.p`. `bar` is used by nothing,
/// `baz` only by its own file.
fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/lib.p", "pub use a/x.p/foo\ndef main\nmain")
        .with_file("/ws/a/x.p", "pub def foo\ndef bar\ndef baz\nbaz")
        .with_file(
            "/ws/b/y.p",
            "use lib.p/foo\nuse a/x.p/foo\nuse a/x.p/foo\nuse ext/z\nuse a/x.p\nfoo",
        )
        .with_file("/ws/c/w.p", "use b/y.p\ny");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new().with(Fake::default()),
    )
}

fn module(parts: &[&str]) -> Address {
    Address::new("ws", parts.iter().map(|p| (*p).to_owned()))
}

#[test]
fn surface_lists_what_reexports_offer_and_who_takes_it() {
    let surface = engine()
        .run(SurfaceQuery {
            package: Some("ws".to_owned()),
        })
        .unwrap();
    // Only `foo` is `pub`; a re-export never widens what it re-exports.
    let items: Vec<(&str, &Address, &[Address], usize)> = surface
        .items
        .iter()
        .map(|e| {
            (
                e.declaration.symbol.name.as_str(),
                &e.declaration.address,
                e.via.as_slice(),
                e.importers,
            )
        })
        .collect();
    assert_eq!(
        items,
        [(
            "foo",
            &module(&["a", "x.p", "foo"]),
            &[module(&["lib.p", "foo"])][..],
            2, // lib.p re-exports it, b/y.p imports it
        )]
    );
    assert_eq!(surface.package.as_ref().map(|p| p.as_str()), Some("ws"));
}

#[test]
fn impact_walks_importers_outward_by_depth() {
    let impact = engine()
        .run(ImpactQuery {
            name: "foo".to_owned(),
            declared_in: None,
        })
        .unwrap();
    assert_eq!(impact.address, module(&["a", "x.p", "foo"]));
    let rings: Vec<(String, u32, &Address)> = impact
        .consumers
        .iter()
        .map(|c| (c.path.display().to_string(), c.depth, &c.through))
        .collect();
    assert_eq!(
        rings,
        [
            ("b/y.p".to_owned(), 1, &module(&["a", "x.p"])),
            ("lib.p".to_owned(), 1, &module(&["a", "x.p"])),
            ("c/w.p".to_owned(), 2, &module(&["b", "y.p"])),
        ]
    );
}

#[test]
fn dead_lists_what_nothing_refers_to_and_counts_the_unsure() {
    let dead = engine().run(DeadQuery::default()).unwrap();
    let items: Vec<(&str, &Path, usize)> = dead
        .items
        .iter()
        .map(|u| {
            (
                u.declaration.symbol.name.as_str(),
                u.declaration.path.as_path(),
                u.unsure,
            )
        })
        .collect();
    // `foo` is imported, `baz` names itself, `main` is spelled in lib.p.
    assert_eq!(items, [("bar", Path::new("a/x.p"), 0)]);
}

#[test]
fn imports_flag_unresolved_unused_and_redundant() {
    let report = engine().run(ImportsQuery::default()).unwrap();
    let paths = |sites: &[vvv_engine::ImportSite]| -> Vec<(RelPath, String)> {
        sites
            .iter()
            .map(|s| (s.path.clone(), s.import.path.to_string()))
            .collect()
    };
    let y = RelPath::from("b/y.p");
    assert_eq!(paths(&report.unresolved), [(y.clone(), "ext/z".to_owned())]);
    assert_eq!(
        paths(&report.redundant),
        [(y.clone(), "a/x.p/foo".to_owned())],
        "the second `use a/x.p/foo`"
    );
    // `use a/x.p` binds `x.p` and `use b/y.p` binds `y.p`: no token spells
    // either. The unresolved `ext/z` is not listed twice.
    assert_eq!(
        paths(&report.unused),
        [
            (y.clone(), "a/x.p".to_owned()),
            (RelPath::from("c/w.p"), "b/y.p".to_owned()),
        ]
    );
    let one = engine()
        .run(ImportsQuery {
            path: Some(RelPath::from("c/w.p")),
        })
        .unwrap();
    assert_eq!(one.path, Some(RelPath::from("c/w.p")));
    assert_eq!(one.unused.len(), 1);
    assert!(one.unresolved.is_empty() && one.redundant.is_empty());
}

/// A fragment resolves a file's imports once per file stamp: a session asks
/// the layout again only for a file that changed.
#[test]
fn a_fragment_is_built_once_per_stamp() {
    let resolves = Arc::new(AtomicUsize::new(0));
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/a.p", "def foo")
            .with_file("/ws/b.p", "use a.p/foo\nuse a.p\nfoo"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new()
            .with(common::Counting::new(Default::default()).resolving(resolves.clone())),
    )
    .with_retention(Retention::session());
    engine.run(DeadQuery::default()).unwrap();
    let first = resolves.load(Ordering::SeqCst);
    assert!(first >= 2, "both imports of b.p were placed");
    engine.run(ImportsQuery::default()).unwrap();
    engine.run(SurfaceQuery { package: None }).unwrap();
    // A rename judges through the file's scope, read off the same fragment.
    engine
        .run(vvv_engine::RenameIntent::new("foo", "bar"))
        .unwrap();
    assert_eq!(
        resolves.load(Ordering::SeqCst),
        first,
        "nothing changed, nothing re-resolved"
    );
    vfs.write(Path::new("/ws/b.p"), "use a.p/foo\nfoo").unwrap();
    engine.run(ImportsQuery::default()).unwrap();
    assert!(
        resolves.load(Ordering::SeqCst) > first,
        "the changed file is placed again"
    );
}
