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
    Rust::default()
        .symbols(src)
        .unwrap()
        .into_iter()
        .map(|s| (s.kind, s.name))
        .collect()
}

#[test]
fn extents_take_attributes_and_docs_up_to_a_blank_line() {
    let src = "// section\n\n/// Docs.\n#[derive(Debug)]\n#[cfg(test)]\npub(crate) struct S;\n\nfn f() {}\n";
    let symbols = Rust::default().symbols(src).unwrap();
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
    let mods: Vec<(String, Option<String>)> = Rust::default()
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
    let sem = Rust::default().semantics();
    assert_eq!(
        sem.reach_kind(Some("pub(in crate::x)")),
        vvv_core::ReachKind::Path
    );
    assert!(sem.is_addressable(SymbolKind::Struct) && !sem.is_addressable(SymbolKind::Method));
}

#[test]
fn facts_are_the_union_of_the_single_questions() {
    let lang = Rust::default();
    let src = format!("{DECLS}\nuse crate::a::b;\nfn g() {{ let p = Point::new(); }}\n");
    let facts = lang.facts(&src).unwrap();
    assert_eq!(facts.symbols, lang.symbols(&src).unwrap());
    assert_eq!(facts.imports, lang.imports(&src).unwrap());
    assert_eq!(facts.highlights, lang.highlights(&src).unwrap());
    for name in ["Point", "new", "x", "free", "b"] {
        let from_facts: Vec<vvv_core::Span> = facts.tokens_named(name).map(|(s, _)| s).collect();
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
    let found = Rust::default()
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
    let found = Rust::default()
        .find(src, &Query::pattern("Language"))
        .unwrap();
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
    let found = Rust::default()
        .find(src, &Query::pattern("Language"))
        .unwrap();
    let roles: Vec<Role> = found.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::Import, Role::Import, Role::Import, Role::Use]);
    let structural = Rust::default()
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
    let got: Vec<(vvv_core::HighlightKind, &str)> = Rust::default()
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
    let found = Rust::default()
        .find(SRC, &Query::of_kind("struct_item"))
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "struct S;");
}

#[test]
fn bad_kind_is_an_error() {
    let err = Rust::default()
        .find(SRC, &Query::of_kind("nope"))
        .unwrap_err();
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
    let found = Rust::default()
        .find(DECLS, &Query::of_kind("function_item"))
        .unwrap();
    assert_eq!(found[0].symbol.as_ref().unwrap().name, "free");

    let methods = Rust::default()
        .find(DECLS, &Query::of_symbol(SymbolKind::Method))
        .unwrap();
    let names: Vec<&str> = methods
        .iter()
        .map(|m| m.symbol.as_ref().unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["new", "area", "name"]);

    let both = Rust::default()
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

    let named = Rust::default().find(DECLS, &Query::named("Point")).unwrap();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].symbol.as_ref().unwrap().kind, SymbolKind::Struct);
}

#[test]
fn references_are_identifier_tokens_only() {
    let src = "fn foo() {}\nfn bar() { foo(); let foo = 1; \"foo\"; /* foo */ x.foo }";
    let refs = Rust::default().references(src, "foo").unwrap();
    let kinds: Vec<&str> = refs.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["identifier", "identifier", "identifier", "field_identifier"]
    );
    assert!(refs.iter().all(|r| r.text == "foo"));
}
