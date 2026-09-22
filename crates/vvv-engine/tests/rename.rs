//! Rename against two instances of the shared fake language, to prove
//! occurrences stay within the declaring language and selection narrows them.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::Fake;
use vvv_core::SymbolKind;
use vvv_engine::{
    Apply, Engine, EngineError, FileQuery, Languages, MemoryVfs, RelPath, RenameIntent, Selection,
    Workspace,
};

fn engine() -> Engine {
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.one", "def foo\nfoo() foo_bar foo")
        .with_file("/ws/b.one", "call foo")
        .with_file("/ws/c.two", "def foo\nfoo");
    Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new()
            .with(Fake::new("one", &["one"]))
            .with(Fake::new("two", &["two"])),
    )
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
fn renames_declaration_and_references_in_declaring_language_only() {
    let engine = engine();
    let rename = engine
        .run(RenameIntent::new("foo", "qux").in_language("one"))
        .unwrap();
    assert_eq!(rename.declarations.len(), 1);
    assert_eq!(rename.occurrences.len(), 4);

    engine.run(Apply(rename)).unwrap();
    assert_eq!(read(&engine, "a.one"), "def qux\nqux() foo_bar qux");
    assert_eq!(read(&engine, "b.one"), "call qux");
    assert_eq!(
        read(&engine, "c.two"),
        "def foo\nfoo",
        "other language untouched"
    );
}

#[test]
fn without_language_every_declaring_language_is_renamed() {
    let engine = engine();
    let rename = engine.run(RenameIntent::new("foo", "qux")).unwrap();
    assert_eq!(rename.declarations.len(), 2);
    assert_eq!(rename.occurrences.len(), 6);
}

/// Two declarations in one language are ambiguous even when another
/// language declares the name too.
#[test]
fn ambiguity_is_judged_per_language() {
    let vfs = MemoryVfs::new()
        .with_file("/ws/a.one", "def foo")
        .with_file("/ws/b.one", "def foo")
        .with_file("/ws/c.two", "def foo\nfoo")
        .with_file("/ws/d.two", "foo");
    let engine = Engine::new(
        Workspace::new("/ws", Arc::new(vfs)),
        Languages::new()
            .with(Fake::new("one", &["one"]))
            .with(Fake::new("two", &["two"])),
    );
    let err = engine.run(RenameIntent::new("foo", "qux")).unwrap_err();
    assert!(
        matches!(&err, EngineError::AmbiguousSymbol { declarations, .. } if declarations.len() == 2),
        "{err}"
    );

    let rename = engine
        .run(RenameIntent::new("foo", "qux").declared_in("a.one"))
        .unwrap();
    let touched: Vec<&Path> = rename.files.iter().map(|f| f.path.as_path()).collect();
    assert_eq!(
        touched,
        [Path::new("a.one")],
        "declared_in narrows to one file"
    );
}

#[test]
fn selection_limits_occurrences() {
    let engine = engine();
    let all = engine
        .run(RenameIntent::new("foo", "qux").in_language("one"))
        .unwrap();
    let only_b = all
        .occurrences
        .iter()
        .find(|o| o.m.path == Path::new("b.one"))
        .unwrap();
    let rename = engine
        .run(
            RenameIntent::new("foo", "qux")
                .in_language("one")
                .selecting(Selection::ids([only_b.m.id.clone()])),
        )
        .unwrap();
    assert_eq!(rename.files.len(), 1);
    assert_eq!(rename.preview()[0].after, "call qux");
}

#[test]
fn missing_declaration_is_an_error() {
    let err = engine()
        .run(RenameIntent::new("nope", "x").of_symbol(SymbolKind::Struct))
        .unwrap_err();
    assert!(matches!(err, EngineError::NoSuchSymbol { .. }));
    assert_eq!(err.to_string(), "no struct named `nope`");
}

mod scope {
    //! Same name declared twice: the target's occurrences are resolved, the
    //! other declaration's are `Other`, bare tokens elsewhere unresolved.

    use std::path::Path;
    use std::sync::Arc;

    use vvv_engine::{
        Confidence, Engine, EngineError, Languages, MemoryVfs, Reason, RenameIntent, Workspace,
    };

    use super::common::Fake;

    fn engine() -> Engine {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a.p", "def foo\nfoo foo")
            .with_file("/ws/b.p", "def foo\nfoo")
            .with_file("/ws/c.p", "use a.p/*\nfoo")
            .with_file("/ws/d.p", "use b.p/*\nfoo")
            .with_file("/ws/e.p", "foo");
        Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        )
    }

    fn confidences(engine: &Engine, intent: &RenameIntent) -> Vec<(String, Confidence)> {
        engine
            .run(intent.clone())
            .unwrap()
            .occurrences
            .iter()
            .map(|o| (o.m.path.display().to_string(), o.confidence))
            .collect()
    }

    #[test]
    fn ambiguous_without_declared_in() {
        let err = engine().run(RenameIntent::new("foo", "bar")).unwrap_err();
        assert!(
            matches!(&err, EngineError::AmbiguousSymbol { declarations, .. } if declarations.len() == 2)
        );
        assert!(err.to_string().contains("a.p:1:1") && err.to_string().contains("--in"));
    }

    #[test]
    fn declared_in_classifies_and_renames_only_resolved() {
        let engine = engine();
        let intent = RenameIntent::new("foo", "bar").declared_in("a.p");
        let got = confidences(&engine, &intent);
        use Confidence::*;
        // Grouped by verdict, `✓ ? ✗`, as a preview lists them.
        assert_eq!(
            got,
            [
                ("a.p".to_owned(), Resolved),
                ("a.p".to_owned(), Resolved),
                ("a.p".to_owned(), Resolved),
                ("c.p".to_owned(), Resolved),
                ("e.p".to_owned(), Unresolved),
                ("b.p".to_owned(), Other),
                ("b.p".to_owned(), Other),
                ("d.p".to_owned(), Other),
            ]
        );
        let rename = engine.run(intent.clone()).unwrap();
        let touched: Vec<&Path> = rename.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            touched,
            [Path::new("a.p"), Path::new("c.p")],
            "only resolved by default"
        );
    }

    /// A token at the end of a path is judged by the path, never by what a
    /// bare token of that name would mean in the file.
    #[test]
    fn qualified_tokens_follow_their_path() {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a.p", "def foo")
            .with_file("/ws/b.p", "def foo")
            // Imports b's foo, then names one from an unknown package.
            .with_file("/ws/c.p", "use b.p/*\next::foo foo Self::foo");
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        );
        let intent = RenameIntent::new("foo", "bar").declared_in("a.p");
        let got = confidences(&engine, &intent);
        use Confidence::*;
        assert_eq!(
            got,
            [
                ("a.p".to_owned(), Resolved),
                ("c.p".to_owned(), Unresolved), // ext::foo
                ("b.p".to_owned(), Other),
                ("c.p".to_owned(), Other), // foo, via the glob
                ("c.p".to_owned(), Other), // Self::foo, judged as bare
            ]
        );
        let reasons: Vec<Reason> = engine
            .run(intent.clone())
            .unwrap()
            .occurrences
            .iter()
            .map(|o| o.reason)
            .collect();
        assert_eq!(
            reasons,
            [
                Reason::Declaring,
                Reason::Unresolved,
                Reason::OtherDeclaration,
                Reason::OtherDeclaration,
                Reason::OtherDeclaration,
            ]
        );
    }

    /// A token in the middle of a path names the path up to it: `foo` in
    /// `a.p::foo::new` is the declaration, whatever `new` turns out to be.
    #[test]
    fn tokens_inside_a_path_are_judged_by_the_path_up_to_them() {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a.p", "def foo")
            .with_file("/ws/b.p", "def foo")
            .with_file("/ws/c.p", "a.p::foo::new b.p::foo::new");
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        );
        let intent = RenameIntent::new("foo", "bar").declared_in("a.p");
        use Confidence::*;
        assert_eq!(
            confidences(&engine, &intent),
            [
                ("a.p".to_owned(), Resolved),
                ("c.p".to_owned(), Resolved), // a.p::foo::new
                ("b.p".to_owned(), Other),
                ("c.p".to_owned(), Other), // b.p::foo::new
            ]
        );
    }

    #[test]
    fn unambiguous_name_still_renames_unresolved_tokens() {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a.p", "def foo\nfoo")
            .with_file("/ws/e.p", "foo");
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        );
        let rename = engine.run(RenameIntent::new("foo", "bar")).unwrap();
        assert_eq!(
            rename
                .occurrences
                .iter()
                .filter(|o| o.confidence == Confidence::Unresolved)
                .count(),
            1
        );
        assert_eq!(
            rename.files.len(),
            2,
            "no competing declaration: everything is renamed"
        );
    }
}

mod reexports {
    use std::sync::Arc;

    use super::common::Fake;
    use vvv_engine::{Confidence, Engine, Languages, MemoryVfs, Reason, RenameIntent, Workspace};

    /// `lib.p` re-exports `a/x.p`'s `foo`; `c.p` imports it from `lib.p` and
    /// `d.p` opens `lib.p` with a glob. Both reach the declaration.
    #[test]
    fn a_name_reached_through_a_re_export_chain_is_resolved() {
        let vfs = MemoryVfs::new()
            .with_file("/ws/a/x.p", "def foo\nfoo")
            .with_file("/ws/lib.p", "pub use a/x.p/foo")
            .with_file("/ws/facade.p", "pub use lib.p/foo")
            .with_file("/ws/c.p", "use facade.p/foo\nfoo")
            .with_file("/ws/d.p", "use lib.p/*\nfoo");
        let engine = Engine::new(
            Workspace::new("/ws", Arc::new(vfs)),
            Languages::new().with(Fake::default()),
        );
        let rename = engine
            .run(RenameIntent::new("foo", "bar").declared_in("a/x.p"))
            .unwrap();
        let by_file: Vec<(String, Confidence, Reason)> = rename
            .occurrences
            .iter()
            .map(|o| (o.m.path.display().to_string(), o.confidence, o.reason))
            .collect();
        assert!(by_file.contains(&("c.p".to_owned(), Confidence::Resolved, Reason::ReExport)));
        assert!(by_file.contains(&("d.p".to_owned(), Confidence::Resolved, Reason::ReExport)));
        assert!(
            by_file
                .iter()
                .all(|(_, confidence, _)| *confidence == Confidence::Resolved),
            "{by_file:?}"
        );
    }
}
