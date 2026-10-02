use std::path::Path;
use std::path::PathBuf;

use vvv_core::{Address, ImportRef, Language, Name, ResolveError};

use super::*;
use crate::syntax::fixture::Fixture;

/// `crate::a::b` in the fixture crate, whose lib name is `fixture`.
fn addr(s: &str) -> Address {
    let rest = s.strip_prefix("crate").unwrap_or(s);
    Address::new("fixture", rest.split("::").filter(|seg| !seg.is_empty()))
}

static RUST: std::sync::LazyLock<Rust> = std::sync::LazyLock::new(Rust::default);

fn resolver() -> Fixture<'static> {
    Fixture::new(
        &*RUST,
        &[
            ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
            ("src/lib.rs", "pub mod foo;\npub mod baz;\n"),
            ("src/foo.rs", "pub mod bar;\nmod other;\n"),
            ("src/foo/bar.rs", "use super::other::X;\n"),
            ("src/foo/other.rs", "pub struct X;\n"),
            ("src/baz/mod.rs", "// baz\n"),
            ("src/bin/tool.rs", ""),
        ],
    )
}

fn rules() -> Fixture<'static> {
    resolver()
}

/// A Cargo workspace: `core` is used by `app` under a renamed dependency,
/// `serde` is external, the root manifest declares no package.
fn multi_crate() -> Fixture<'static> {
    Fixture::new(
        &*RUST,
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "crates/core/Cargo.toml",
                "[package]\nname = \"fff-search\"\n\n[lib]\npath = \"src/lib.rs\"\n",
            ),
            ("crates/core/src/lib.rs", "pub mod grep;\n"),
            ("crates/core/src/grep.rs", "pub struct GrepMatch;\n"),
            (
                "crates/app/Cargo.toml",
                "[package]\nname = \"fff-app\"\n\n[dependencies]\nfff = { package = \"fff-search\", path = \"../core\" }\nserde = \"1\"\n",
            ),
            ("crates/app/src/lib.rs", "use fff::grep::GrepMatch;\n"),
        ],
    )
}

#[test]
fn crates_are_addressed_by_the_name_paths_use() {
    let r = multi_crate();
    assert_eq!(
        r.address(Path::new("crates/core/src/grep.rs")).unwrap(),
        Address::new("fff_search", ["grep"])
    );
    assert_eq!(
        r.address(Path::new("crates/app/src/lib.rs")).unwrap(),
        Address::root("fff_app")
    );
    assert!(
        r.address(Path::new("Cargo.toml")).is_err(),
        "no package at the root"
    );
}

#[test]
fn paths_into_other_crates_resolve_through_the_dependency_name() {
    let r = multi_crate();
    let app = Path::new("crates/app/src/lib.rs");
    assert_eq!(
        r.resolve(app, "fff::grep::GrepMatch"),
        Some(Address::new("fff_search", ["grep", "GrepMatch"])),
        "renamed workspace dependency"
    );
    assert_eq!(
        r.resolve(app, "serde::Serialize"),
        Some(Address::new("serde", ["Serialize"])),
        "external dependency, by its own name"
    );
    assert_eq!(r.resolve(app, "tokio::spawn"), None, "not a dependency");
    assert_eq!(
        r.resolve(app, "std::collections::BTreeMap"),
        Some(Address::new("std", ["collections", "BTreeMap"])),
        "the standard library needs no manifest entry"
    );
    assert_eq!(
        r.resolve(app, "core::fmt::Display"),
        Some(Address::new("core", ["fmt", "Display"]))
    );
    assert_eq!(
        r.resolve(
            Path::new("crates/core/src/grep.rs"),
            "crate::grep::GrepMatch"
        ),
        Some(Address::new("fff_search", ["grep", "GrepMatch"]))
    );
}

#[test]
fn cross_crate_paths_render_with_the_dependency_name() {
    let r = multi_crate();
    let target = Address::new("fff_search", ["scan", "GrepMatch"]);
    assert_eq!(
        r.render(
            Path::new("crates/app/src/lib.rs"),
            &target,
            "fff::grep::GrepMatch"
        ),
        "fff::scan::GrepMatch"
    );
    assert_eq!(
        r.render(
            Path::new("crates/core/src/lib.rs"),
            &target,
            "crate::grep::GrepMatch"
        ),
        "crate::scan::GrepMatch"
    );
}

#[test]
fn addresses_follow_cargo_layout() {
    let r = resolver();
    assert_eq!(r.address(Path::new("src/lib.rs")).unwrap(), addr("crate"));
    assert_eq!(
        r.address(Path::new("src/foo.rs")).unwrap(),
        addr("crate::foo")
    );
    assert_eq!(
        r.address(Path::new("src/foo/bar.rs")).unwrap(),
        addr("crate::foo::bar")
    );
    assert_eq!(
        r.address(Path::new("src/baz/mod.rs")).unwrap(),
        addr("crate::baz")
    );
    assert_eq!(
        r.address(Path::new("src/new/dir/x.rs")).unwrap(),
        addr("crate::new::dir::x")
    );
    assert_eq!(
        r.address(Path::new("src/foo")).unwrap(),
        addr("crate::foo"),
        "a directory is its module"
    );
    assert_eq!(
        r.address(Path::new("src/new/dir")).unwrap(),
        addr("crate::new::dir")
    );
    assert!(
        r.address(Path::new("src")).is_err(),
        "the crate root dir is not a module"
    );
    assert!(r.address(Path::new("src/bin/tool.rs")).is_err());
    assert!(r.address(Path::new("README.md")).is_err());
}

#[test]
fn resolves_crate_self_and_super() {
    let r = resolver();
    let bar = Path::new("src/foo/bar.rs");
    assert_eq!(r.resolve(bar, "crate::a::B"), Some(addr("crate::a::B")));
    assert_eq!(r.resolve(bar, "self::Y"), Some(addr("crate::foo::bar::Y")));
    assert_eq!(
        r.resolve(bar, "super::other::X"),
        Some(addr("crate::foo::other::X"))
    );
    assert_eq!(
        r.resolve(bar, "super::super::baz"),
        Some(addr("crate::baz"))
    );
    assert_eq!(r.resolve(bar, "super::super::super::x"), None);
    assert_eq!(r.resolve(bar, "tokio::fmt"), None, "an unknown crate");
    assert_eq!(
        r.resolve(bar, "std::fmt"),
        Some(Address::new("std", ["fmt"])),
        "the standard library"
    );
    // child-module paths: `bar::X` in foo.rs is `crate::foo::bar::X`
    assert_eq!(
        r.resolve(Path::new("src/foo.rs"), "bar::X"),
        Some(addr("crate::foo::bar::X"))
    );
    assert_eq!(
        r.resolve(Path::new("src/foo.rs"), "nope::X"),
        None,
        "no such child file"
    );
    assert_eq!(
        r.resolve(Path::new("src/foo.rs"), "self::bar"),
        Some(addr("crate::foo::bar"))
    );
}

#[test]
fn render_keeps_relative_style_when_still_valid() {
    let r = resolver();
    let bar = Path::new("src/foo/bar.rs");
    assert_eq!(
        r.render(bar, &addr("crate::foo::other::X"), "super::other::X"),
        "super::other::X"
    );
    assert_eq!(
        r.render(bar, &addr("crate::baz::X"), "super::other::X"),
        "crate::baz::X"
    );
    assert_eq!(
        r.render(bar, &addr("crate::foo::bar::Y"), "self::Y"),
        "self::Y"
    );
    assert_eq!(
        r.render(bar, &addr("crate::q::Y"), "crate::z::Y"),
        "crate::q::Y"
    );
    // child-module style survives a rename within the same parent, not a move out
    let foo = Path::new("src/foo.rs");
    assert_eq!(
        r.render(foo, &addr("crate::foo::qux::X"), "bar::X"),
        "qux::X"
    );
    assert_eq!(
        r.render(foo, &addr("crate::baz::bar::X"), "bar::X"),
        "crate::baz::bar::X"
    );
}

#[test]
fn imports_are_outermost_paths_with_nested_entries_flagged() {
    let src = "use crate::a::b::C;\nuse super::x::{Y, z::W, q::*, r::{self, S}};\nfn f() { crate::a::g::<u8>(); let t: crate::a::T<u8> = crate::a::m!(); }";
    let got: Vec<(String, bool, bool)> = Rust::default()
        .imports(src)
        .unwrap()
        .into_iter()
        .map(|r| (r.path.to_string(), r.is_standalone(), r.declares))
        .collect();
    // (path, standalone, declares): paths in `use` bring a name into
    // scope; paths in the body are references only.
    let expected = [
        ("crate::a::b::C", true, true),
        ("super::x", true, true),
        ("super::x::Y", false, true),
        ("super::x::z::W", false, true),
        ("super::x::q", false, true),
        ("super::x::r", false, true),
        ("super::x::r::S", false, true),
        ("crate::a::g", true, false),
        ("crate::a::T", true, false),
        ("crate::a::m", true, false),
    ];
    let got: Vec<(&str, bool, bool)> = got.iter().map(|(p, s, d)| (p.as_str(), *s, *d)).collect();
    assert_eq!(got, expected);
}

#[test]
fn group_context_describes_each_entry() {
    let src = "pub use crate::u::{a::B as C, d::*, e::{self, F}, g};";
    let imports = Rust::default().imports(src).unwrap();
    let entry = |path: &str| {
        imports
            .iter()
            .find(|i| i.path.to_string() == path)
            .unwrap()
            .clone()
    };
    let item = |r: &ImportRef| {
        let g = r.group.as_ref().unwrap();
        src[g.item.start..g.item.end].to_owned()
    };

    let b = entry("crate::u::a::B");
    let g = b.group.as_ref().unwrap();
    assert_eq!(g.prefix.to_string(), "crate::u");
    assert_eq!(item(&b), "a::B as C");
    assert_eq!(
        &src[g.list.start..g.list.end],
        "{a::B as C, d::*, e::{self, F}, g}"
    );
    assert_eq!(g.items, 4);
    assert_eq!(&src[g.statement.start..g.statement.end], src);
    assert!(g.top_level);

    let f = entry("crate::u::e::F");
    let g = f.group.as_ref().unwrap();
    assert_eq!(g.prefix.to_string(), "crate::u::e");
    assert_eq!(item(&f), "F");
    assert_eq!(g.items, 2);
    assert!(!g.top_level);

    assert_eq!(item(&entry("crate::u::e")), "e::{self, F}");
    assert_eq!(item(&entry("crate::u::d")), "d::*");
    assert!(entry("crate::u::d").glob && !entry("crate::u::g").glob);
    assert_eq!(item(&entry("crate::u::g")), "g");

    let top = Rust::default()
        .imports("use crate::a::b::*;\nuse crate::a::c;")
        .unwrap();
    assert!(top[0].glob && !top[1].glob);
}

/// The name an import binds is its alias when it has one, whether the
/// path stands alone or is an entry of a group.
#[test]
fn aliases_are_the_binding() {
    let imports = Rust::default()
            .imports("use crate::a::B as C;\nuse crate::u::{d::E as F, g, H as I};\nuse crate::h::*;\nuse J as K;")
            .unwrap();
    let bound: Vec<(String, Option<&str>)> = imports
        .iter()
        .map(|i| (i.path.to_string(), i.binding().map(Name::as_str)))
        .collect();
    assert_eq!(
        bound,
        [
            ("crate::a::B".to_owned(), Some("C")),
            ("crate::u".to_owned(), Some("u")),
            ("crate::u::d::E".to_owned(), Some("F")),
            ("crate::u::g".to_owned(), Some("g")),
            ("crate::u::H".to_owned(), Some("I")),
            ("crate::h".to_owned(), None),
            ("J".to_owned(), Some("K")),
        ]
    );
}

#[test]
fn relocate_moves_the_mod_declaration() {
    let r = rules();
    let edits = r
        .relocate(Path::new("src/foo/bar.rs"), Path::new("src/baz/bar.rs"))
        .unwrap();
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].path, Path::new("src/foo.rs"));
    assert_eq!(edits[0].edit.replacement, "");
    assert_eq!(
        edits[0].edit.span,
        vvv_core::Span::new(0, 13),
        "`pub mod bar;\\n` removed"
    );
    assert_eq!(edits[1].path, Path::new("src/baz/mod.rs"));
    assert_eq!(edits[1].edit.replacement, "pub mod bar;\n");
}

#[test]
fn relocate_widens_the_mod_line_only_as_told() {
    let r = rules();
    let from = Path::new("src/foo/other.rs");
    let to = Path::new("src/baz/other.rs");
    // Nobody outside needs it: the line moves as written.
    let edits = r.relocate(from, to).unwrap();
    assert_eq!(edits[1].edit.replacement, "mod other;\n");
    // The engine found consumers elsewhere in the crate.
    let edits = r
        .relocate_widening(from, to, Some(vvv_core::ReachKind::Package))
        .unwrap();
    assert_eq!(edits[1].edit.replacement, "pub(crate) mod other;\n");
    // A `pub mod` moving keeps its modifier; widening replaces one that exists.
    let edits = r
        .relocate_widening(
            Path::new("src/foo/bar.rs"),
            Path::new("src/baz/bar.rs"),
            Some(vvv_core::ReachKind::Parent),
        )
        .unwrap();
    assert_eq!(edits[1].edit.replacement, "pub(super) mod bar;\n");
}

#[test]
fn widen_replaces_or_inserts_the_modifier() {
    use vvv_core::{Language, ReachKind, SourceText};
    let lang = Rust::default();
    let src = "pub(super) fn a() {}\nfn b() {}\npub fn c() {}\n";
    let symbols = lang.symbols(src).unwrap();
    let surgery = lang.surgery().unwrap();
    let text = SourceText::new(src);
    let apply = |i: usize, to: ReachKind| {
        surgery
            .widen(&symbols[i], &text, to)
            .map(|e| (e.span, e.replacement))
    };
    assert_eq!(
        apply(0, ReachKind::Package),
        Some((
            symbols[0].visibility.as_ref().unwrap().span,
            "pub(crate)".to_owned()
        ))
    );
    assert_eq!(
        apply(1, ReachKind::Parent),
        Some((
            vvv_core::Span::new(symbols[1].span.start, symbols[1].span.start),
            "pub(super) ".to_owned()
        ))
    );
    assert_eq!(apply(2, ReachKind::Everyone), None, "never inferred");
}

#[test]
fn relocate_within_parent_renames_in_place() {
    let r = rules();
    let edits = r
        .relocate(Path::new("src/foo/bar.rs"), Path::new("src/foo/qux.rs"))
        .unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].edit.replacement, "qux");
}

#[test]
fn a_module_moves_as_file_plus_directory() {
    let r = rules();
    // Named by the file: the directory comes along, and vice versa.
    assert_eq!(
        r.companions(Path::new("src/foo.rs"), Path::new("src/baz/foo.rs")),
        vec![(PathBuf::from("src/foo"), PathBuf::from("src/baz/foo"))]
    );
    assert_eq!(
        r.companions(Path::new("src/foo"), Path::new("src/baz/foo")),
        vec![(PathBuf::from("src/foo.rs"), PathBuf::from("src/baz/foo.rs"))]
    );
    assert!(
        r.companions(Path::new("src/foo/other.rs"), Path::new("src/x.rs"))
            .is_empty()
    );
    // Relocating the module named either way moves `mod foo;` into baz.
    let edits = r
        .relocate(Path::new("src/foo"), Path::new("src/baz/foo"))
        .unwrap();
    assert_eq!(edits[0].path, Path::new("src/lib.rs"));
    assert_eq!(edits[1].path, Path::new("src/baz/mod.rs"));
    assert_eq!(edits[1].edit.replacement, "pub mod foo;\n");
}

#[test]
fn relocate_refuses_what_it_cannot_do() {
    let r = rules();
    let err = |from: &str, to: &str| r.relocate(Path::new(from), Path::new(to)).unwrap_err();
    assert!(matches!(
        err("src/lib.rs", "src/x.rs"),
        ResolveError::Root(_)
    ));
    let missing = err("src/foo/bar.rs", "src/nope/bar.rs");
    assert!(matches!(
        &missing,
        ResolveError::NoParentFile { module, candidates, declaration }
            if module == "crate::nope" && candidates.len() == 2 && declaration == "mod bar;"
    ));
    // The message embeds a path, which Windows spells with `\`; the corpus
    // snapshots are taken with `/` for the same reason.
    assert_eq!(
        missing.to_string().replace('\\', "/"),
        "no file declares module `crate::nope`; create src/nope.rs (or src/nope/mod.rs) first so `mod bar;` has a home"
    );
}
