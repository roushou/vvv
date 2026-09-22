//! Rust support: grammar and semantics as tables, the Cargo layout as pure
//! path algebra, and the surgery that spells `use` paths and `mod` lines.

mod grammar;
mod layout;
mod surgery;

use vvv_core::LanguageId;

use crate::syntax::AstGrepLanguage;

pub use layout::RustLayout;
pub use surgery::RustSurgery;

/// Rust as a [`vvv_core::Language`]: the grammar and semantics tables, the
/// Cargo layout, and the surgery for `use` paths and `mod` lines.
pub type Rust = AstGrepLanguage<ast_grep_language::Rust>;

impl Rust {
    pub const ID: LanguageId = LanguageId::new("rust");

    pub fn new() -> Self {
        AstGrepLanguage::describe(
            Self::ID,
            &["rs"],
            ast_grep_language::Rust,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_layout(RustLayout)
        .with_surgery(RustSurgery)
    }
}

impl Default for Rust {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_core::{CaptureValue, Language, Query, Role, SearchError, SymbolKind};

    const SRC: &str = "fn alpha() {}\nstruct S;\nfn beta(x: u8, y: u8) {}\n";

    const DECLS: &str = r#"
pub fn free() {}
pub struct Point { x: i32 }
impl Point {
    pub fn new() -> Self { fn helper() {} Self { x: 0 } }
}
pub trait Shape { fn area(&self) -> f64; fn name(&self) -> &str { "shape" } }
pub enum Dir { Up, Down }
type Alias = Point;
const MAX: u8 = 1;
static COUNT: u8 = 0;
mod inner {}
macro_rules! m { () => {} }
"#;

    fn symbols(src: &str) -> Vec<(SymbolKind, String)> {
        Rust::new()
            .symbols(src)
            .unwrap()
            .into_iter()
            .map(|s| (s.kind, s.name))
            .collect()
    }

    #[test]
    fn extents_take_attributes_and_docs_up_to_a_blank_line() {
        let src = "// section\n\n/// Docs.\n#[derive(Debug)]\n#[cfg(test)]\npub(crate) struct S;\n\nfn f() {}\n";
        let symbols = Rust::new().symbols(src).unwrap();
        let s = &symbols[0];
        assert_eq!(
            &src[s.extent.start..s.extent.end],
            "/// Docs.\n#[derive(Debug)]\n#[cfg(test)]\npub(crate) struct S;"
        );
        assert_eq!(&src[s.span.start..s.span.end], "pub(crate) struct S;");
        assert_eq!(s.modifier(), Some("pub(crate)"));
        let f = &symbols[1];
        assert_eq!(&src[f.extent.start..f.extent.end], "fn f() {}");
        assert_eq!(f.modifier(), None);
    }

    #[test]
    fn every_visibility_form_is_read_and_variants_take_none() {
        let src = "pub fn a() {}\npub(super) fn b() {}\npub(in crate::x) fn c() {}\nfn d() {}\npub enum E { /// doc\n V }\n";
        let mods: Vec<(String, Option<String>)> = Rust::new()
            .symbols(src)
            .unwrap()
            .into_iter()
            .map(|s| (s.name.clone(), s.modifier().map(str::to_owned)))
            .collect();
        assert_eq!(
            mods,
            [
                ("a".to_owned(), Some("pub".to_owned())),
                ("b".to_owned(), Some("pub(super)".to_owned())),
                ("c".to_owned(), Some("pub(in crate::x)".to_owned())),
                ("d".to_owned(), None),
                ("E".to_owned(), Some("pub".to_owned())),
                ("V".to_owned(), None),
            ]
        );
        let sem = Rust::new().semantics();
        assert_eq!(
            sem.reach_kind(Some("pub(in crate::x)")),
            vvv_core::ReachKind::Path
        );
        assert!(sem.is_addressable(SymbolKind::Struct) && !sem.is_addressable(SymbolKind::Method));
    }

    #[test]
    fn facts_are_the_union_of_the_single_questions() {
        let lang = Rust::new();
        let src = format!("{DECLS}\nuse crate::a::b;\nfn g() {{ let p = Point::new(); }}\n");
        let facts = lang.facts(&src).unwrap();
        assert_eq!(facts.symbols, lang.symbols(&src).unwrap());
        assert_eq!(facts.imports, lang.imports(&src).unwrap());
        assert_eq!(facts.highlights, lang.highlights(&src).unwrap());
        for name in ["Point", "new", "x", "free", "b"] {
            let from_facts: Vec<vvv_core::Span> =
                facts.tokens_named(name).map(|(s, _)| s).collect();
            let from_refs: Vec<vvv_core::Span> = lang
                .references(&src, name)
                .unwrap()
                .into_iter()
                .map(|r| r.span)
                .collect();
            assert_eq!(from_facts, from_refs, "{name}");
        }
    }

    #[test]
    fn pattern_with_captures() {
        let found = Rust::new()
            .find(SRC, &Query::pattern("fn $NAME($$$ARGS) {}"))
            .unwrap();
        let names: Vec<&str> = found
            .iter()
            .map(|m| match m.captures.get("NAME") {
                Some(CaptureValue::Single(c)) => c.text.as_str(),
                other => panic!("unexpected capture {other:?}"),
            })
            .collect();
        assert_eq!(names, ["alpha", "beta"]);
        assert!(matches!(
            found[1].captures.get("ARGS"),
            Some(CaptureValue::Multiple(args)) if args.len() == 3 // `x: u8`, `,`, `y: u8`
        ));
    }

    #[test]
    fn bare_name_finds_every_identifier_position_and_labels_declarations() {
        let src = "use a::Language;\npub trait Language {}\nimpl Language for S {}\nfn f(l: &dyn Language) {}\nlet Languages = 1;";
        let found = Rust::new().find(src, &Query::pattern("Language")).unwrap();
        let kinds: Vec<&str> = found.iter().map(|m| m.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "identifier",
                "type_identifier",
                "type_identifier",
                "type_identifier"
            ],
            "use path, trait name, impl target, dyn type; not `Languages`"
        );
        assert_eq!(found[1].symbol.as_ref().unwrap().kind, SymbolKind::Trait);
        assert!(found[0].symbol.is_none() && found[2].symbol.is_none());
        let roles: Vec<Role> = found.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [Role::Import, Role::Declaration, Role::Use, Role::Use]
        );
    }

    #[test]
    fn roles_see_through_grouped_and_extern_imports() {
        let src = "use a::{b, Language as L};
extern crate Language;
mod m { pub use x::Language; }
fn f() { Language::new(); }";
        let found = Rust::new().find(src, &Query::pattern("Language")).unwrap();
        let roles: Vec<Role> = found.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::Import, Role::Import, Role::Import, Role::Use]);
        let structural = Rust::new()
            .find(
                src,
                &Query::builder()
                    .kind(Some("use_declaration"))
                    .build()
                    .unwrap(),
            )
            .unwrap();
        assert!(structural.iter().all(|m| m.role == Role::Import));
    }

    #[test]
    fn highlights_cover_keywords_strings_types_and_calls() {
        use vvv_core::HighlightKind::*;
        let src = "/// doc\npub fn f(x: u8) -> String { let s = \"hi\"; g(1); s.len(); m!() }";
        let got: Vec<(vvv_core::HighlightKind, &str)> = Rust::new()
            .highlights(src)
            .unwrap()
            .into_iter()
            .map(|h| (h.kind, src[h.span.start..h.span.end].trim_end()))
            .collect();
        let expected = [
            (Comment, "/// doc"),
            (Keyword, "pub"),
            (Keyword, "fn"),
            (Function, "f"),
            (Type, "u8"),
            (Type, "String"),
            (Keyword, "let"),
            (String, "\"hi\""),
            (Function, "g"),
            (Number, "1"),
            (Function, "len"),
            (Macro, "m"),
        ];
        assert_eq!(got, expected);
    }

    #[test]
    fn kind_only() {
        let found = Rust::new()
            .find(SRC, &Query::of_kind("struct_item"))
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "struct S;");
    }

    #[test]
    fn bad_kind_is_an_error() {
        let err = Rust::new().find(SRC, &Query::of_kind("nope")).unwrap_err();
        assert!(matches!(err, SearchError::Kind(_)));
    }

    #[test]
    fn extracts_every_declaration_kind() {
        use SymbolKind::*;
        let expected = [
            (Function, "free"),
            (Struct, "Point"),
            (Field, "x"),
            (Impl, "Point"),
            (Method, "new"),
            (Function, "helper"),
            (Trait, "Shape"),
            (Method, "area"),
            (Method, "name"),
            (Enum, "Dir"),
            (Variant, "Up"),
            (Variant, "Down"),
            (TypeAlias, "Alias"),
            (Const, "MAX"),
            (Static, "COUNT"),
            (Module, "inner"),
            (Macro, "m"),
        ];
        let got = symbols(DECLS);
        let got: Vec<(SymbolKind, &str)> = got.iter().map(|(k, n)| (*k, n.as_str())).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn structural_matches_are_annotated_and_symbolic_queries_filter() {
        let found = Rust::new()
            .find(DECLS, &Query::of_kind("function_item"))
            .unwrap();
        assert_eq!(found[0].symbol.as_ref().unwrap().name, "free");

        let methods = Rust::new()
            .find(DECLS, &Query::of_symbol(SymbolKind::Method))
            .unwrap();
        let names: Vec<&str> = methods
            .iter()
            .map(|m| m.symbol.as_ref().unwrap().name.as_str())
            .collect();
        assert_eq!(names, ["new", "area", "name"]);

        let both = Rust::new()
            .find(
                DECLS,
                &Query::of_kind("function_item").with_symbol(SymbolKind::Method),
            )
            .unwrap();
        let names: Vec<&str> = both
            .iter()
            .map(|m| m.symbol.as_ref().unwrap().name.as_str())
            .collect();
        assert_eq!(
            names,
            ["new", "name"],
            "bodiless `area` is a function_signature_item"
        );

        let named = Rust::new().find(DECLS, &Query::named("Point")).unwrap();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].symbol.as_ref().unwrap().kind, SymbolKind::Struct);
    }

    #[test]
    fn references_are_identifier_tokens_only() {
        let src = "fn foo() {}\nfn bar() { foo(); let foo = 1; \"foo\"; /* foo */ x.foo }";
        let refs = Rust::new().references(src, "foo").unwrap();
        let kinds: Vec<&str> = refs.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["identifier", "identifier", "identifier", "field_identifier"]
        );
        assert!(refs.iter().all(|r| r.text == "foo"));
    }
}

#[cfg(test)]
mod resolver_tests {
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

    static RUST: std::sync::LazyLock<Rust> = std::sync::LazyLock::new(Rust::new);

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
        let got: Vec<(String, bool, bool)> = Rust::new()
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
        let got: Vec<(&str, bool, bool)> =
            got.iter().map(|(p, s, d)| (p.as_str(), *s, *d)).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn group_context_describes_each_entry() {
        let src = "pub use crate::u::{a::B as C, d::*, e::{self, F}, g};";
        let imports = Rust::new().imports(src).unwrap();
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

        let top = Rust::new()
            .imports("use crate::a::b::*;\nuse crate::a::c;")
            .unwrap();
        assert!(top[0].glob && !top[1].glob);
    }

    /// The name an import binds is its alias when it has one, whether the
    /// path stands alone or is an entry of a group.
    #[test]
    fn aliases_are_the_binding() {
        let imports = Rust::new()
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
        let lang = Rust::new();
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
}

#[cfg(test)]
mod regroup_tests {
    //! `regroup` on one statement at a time. The workspace is what a move of
    //! `crate::util::parse` to `crate::net::parse` sees before it happens.

    use std::path::Path;

    use vvv_core::{Address, ChangeSet, ImportRef, Language, SourceText};

    use super::*;
    use crate::syntax::fixture::Fixture;

    const OLD: &str = "crate::util::parse";
    const NEW: &str = "crate::net::parse";

    fn fixture(lang: &Rust) -> Fixture<'_> {
        Fixture::new(
            lang,
            &[
                ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
                ("src/lib.rs", "pub mod util;\npub mod net;\n"),
                ("src/util.rs", "pub mod parse;\npub mod strings;\n"),
                ("src/util/parse.rs", ""),
                ("src/util/strings.rs", ""),
                ("src/net.rs", "pub mod client;\n"),
                ("src/net/client.rs", ""),
            ],
        )
    }

    /// Apply `regroup` to every grouped entry of `src` (as seen from `file`)
    /// whose resolved address is under OLD, rebased onto NEW.
    fn regroup(file: &str, src: &str) -> String {
        let lang = Rust::new();
        let fx = fixture(&lang);
        let file = Path::new(file);
        let old = Address::new("fixture", OLD.trim_start_matches("crate::").split("::"));
        let new = Address::new("fixture", NEW.trim_start_matches("crate::").split("::"));
        let entries: Vec<(ImportRef, Address)> = lang
            .imports(src)
            .unwrap()
            .into_iter()
            .filter(|r| r.group.is_some())
            .filter_map(|r| {
                let resolved = fx.resolve_path(file, &r.path)?;
                let prefix = &r.group.as_ref().unwrap().prefix;
                let prefix_under_old = fx
                    .resolve_path(file, prefix)
                    .is_some_and(|p| p.starts_with(&old));
                if prefix_under_old {
                    return None;
                }
                let target = resolved.rebase(&old, &new)?;
                Some((r, target))
            })
            .collect();
        let source = SourceText::new(src);
        let out = fx.regroup(file, &source, &entries);
        assert!(out.skipped.is_empty(), "skipped: {:?}", out.skipped);
        let mut cs = ChangeSet::new();
        for e in out.edits {
            cs.insert("f.rs", e).unwrap();
        }
        cs.apply_to(Path::new("f.rs"), src)
    }

    #[test]
    fn entry_stays_in_group_when_prefix_still_covers_it() {
        assert_eq!(
            regroup(
                "src/lib.rs",
                "use crate::{util::parse::parse_line, net::Client};\n"
            ),
            "use crate::{net::parse::parse_line, net::Client};\n"
        );
    }

    #[test]
    fn entry_leaves_group_into_its_own_statement() {
        assert_eq!(
            regroup("src/lib.rs", "use crate::util::{parse::Config, strings};\n"),
            "use crate::util::{strings};\nuse crate::net::parse::Config;\n"
        );
    }

    #[test]
    fn alias_wildcard_and_nested_tails_are_kept() {
        assert_eq!(
            regroup(
                "src/lib.rs",
                "use crate::util::{parse::Config as Cfg, strings};\n"
            ),
            "use crate::util::{strings};\nuse crate::net::parse::Config as Cfg;\n"
        );
        assert_eq!(
            regroup("src/lib.rs", "use crate::util::{strings, parse::*};\n"),
            "use crate::util::{strings};\nuse crate::net::parse::*;\n"
        );
        assert_eq!(
            regroup(
                "src/lib.rs",
                "use crate::util::{strings, parse::{self, Config}};\n"
            ),
            "use crate::util::{strings};\nuse crate::net::parse::{self, Config};\n"
        );
    }

    #[test]
    fn whole_statement_is_replaced_when_every_entry_leaves() {
        assert_eq!(
            regroup(
                "src/net.rs",
                "    pub(crate) use crate::util::{parse::*, parse::Config};\n"
            ),
            "    pub(crate) use crate::net::parse::*;\n    pub(crate) use crate::net::parse::Config;\n"
        );
    }

    #[test]
    fn multi_line_groups_lose_whole_lines() {
        let src = "use crate::util::{\n    strings::trim,\n    parse::Config,\n    parse::parse_line,\n};\n";
        assert_eq!(
            regroup("src/net/client.rs", src),
            "use crate::util::{\n    strings::trim,\n};\nuse crate::net::parse::Config;\nuse crate::net::parse::parse_line;\n"
        );
    }

    #[test]
    fn indentation_and_visibility_follow_the_statement() {
        assert_eq!(
            regroup(
                "src/lib.rs",
                "fn f() {\n    pub use crate::util::{parse::Config, strings};\n}\n"
            ),
            "fn f() {\n    pub use crate::util::{strings};\n    pub use crate::net::parse::Config;\n}\n"
        );
    }
}
