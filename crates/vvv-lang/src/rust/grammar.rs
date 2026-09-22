//! What in tree-sitter-rust's grammar counts as a declaration or an
//! identifier, and what its modifiers mean.

use vvv_core::HighlightKind as H;
use vvv_core::ReachKind as R;
use vvv_core::SymbolKind::{self, *};
use vvv_core::{
    Grammar, HighlightRule, ImportGrammar, ImportNesting, ImportRule, ModifierAt, PathSyntax,
    ReExportRule, Semantics, SymbolRule, VisibilityRule,
};

/// What sits above an item and belongs to it: attributes and comments
/// (`///` docs are `line_comment`s), up to the first blank line.
const LEADING: &[&str] = &["attribute_item", "line_comment", "block_comment"];
const VIS: ModifierAt = ModifierAt::Child("visibility_modifier");

/// An item: attributes and docs before it, a visibility modifier inside it.
const fn item(node: &'static str, kind: SymbolKind) -> SymbolRule {
    SymbolRule::new(node, "name", kind)
        .leading(LEADING)
        .visibility(VIS)
}

const SYMBOLS: &[SymbolRule] = &[
    item("function_item", Method).within("impl_item"),
    item("function_item", Method).within("trait_item"),
    item("function_signature_item", Method),
    item("function_item", Function),
    item("struct_item", Struct),
    item("union_item", Struct),
    item("enum_item", Enum),
    // Variants take the enum's visibility; they still carry docs and attributes.
    SymbolRule::new("enum_variant", "name", Variant).leading(LEADING),
    item("trait_item", Trait),
    item("type_item", TypeAlias),
    item("const_item", Const),
    item("static_item", Static),
    item("field_declaration", Field),
    item("mod_item", Module),
    SymbolRule::new("macro_definition", "name", Macro).leading(LEADING),
    // `impl Foo {}` and `impl Trait for Foo {}`: named after the type, so
    // the blocks travel with it when it moves.
    SymbolRule::new("impl_item", "type", Impl)
        .leading(LEADING)
        .name_inner("type"),
];

pub(crate) const SEMANTICS: Semantics = Semantics {
    // `use a::b;` scopes `b`, not `b::X`; only `use a::b::*` opens the module.
    import_scopes_names: false,
    addressable: &[
        Function, Struct, Enum, Trait, TypeAlias, Const, Static, Module, Macro,
    ],
    visibility: &[
        VisibilityRule::exact("pub", R::Everyone),
        VisibilityRule::exact("pub(crate)", R::Package),
        VisibilityRule::exact("pub(super)", R::Parent),
        VisibilityRule::exact("pub(self)", R::Declaring),
        VisibilityRule::prefix("pub(in", R::Path),
    ],
    default_visibility: R::Declaring,
};

const IDENTIFIERS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "shorthand_field_identifier",
];

/// Paths appear as nested `scoped_identifier`s; the extractor keeps the
/// outermost. Entries inside `use a::{b, c::d}` are prefixed with the list's
/// path and marked non-rewritable.
const IMPORTS: ImportGrammar = ImportGrammar::new(
    PathSyntax::Scoped,
    &[
        ImportRule::node("scoped_identifier"),
        ImportRule::node("scoped_type_identifier"),
        // Single-segment entries of a group: `b` in `use a::{b}`, `b` in
        // `use a::{b::*}`, `b` in `use a::{b::{c}}`.
        ImportRule::node("identifier").under("use_list"),
        ImportRule::node("identifier").under("use_wildcard"),
        ImportRule::node("identifier").under("scoped_use_list"),
        // The path of `B as C`, wherever the clause sits; the alias is the
        // clause's other field.
        ImportRule::field("use_as_clause", "path"),
    ],
)
.nested(ImportNesting {
    list: "use_list",
    scope: "scoped_use_list",
    prefix_field: "path",
    statement: "use_declaration",
})
.statements(&["use_declaration", "extern_crate_declaration"])
.aliased_by("use_as_clause", "alias")
.reexports(ReExportRule::Modifier("visibility_modifier"))
.globs_under("use_wildcard", "::*");

const HIGHLIGHTS: &[HighlightRule] = &[
    HighlightRule::new("line_comment", H::Comment),
    HighlightRule::new("block_comment", H::Comment),
    HighlightRule::new("string_literal", H::String),
    HighlightRule::new("raw_string_literal", H::String),
    HighlightRule::new("char_literal", H::String),
    HighlightRule::new("integer_literal", H::Number),
    HighlightRule::new("float_literal", H::Number),
    HighlightRule::new("boolean_literal", H::Number),
    HighlightRule::new("type_identifier", H::Type),
    HighlightRule::new("primitive_type", H::Type),
    HighlightRule::new("lifetime", H::Type),
    HighlightRule::new("attribute_item", H::Attribute),
    HighlightRule::new("inner_attribute_item", H::Attribute),
    HighlightRule::new("identifier", H::Function).under("function_item"),
    HighlightRule::new("identifier", H::Function).under("function_signature_item"),
    HighlightRule::new("identifier", H::Function).under("call_expression"),
    // `s.len()` is call_expression(field_expression(identifier, field_identifier)).
    HighlightRule::new("field_identifier", H::Function).under("field_expression"),
    HighlightRule::new("identifier", H::Macro).under("macro_invocation"),
    HighlightRule::new("identifier", H::Macro).under("macro_definition"),
    HighlightRule::new("self", H::Keyword),
    HighlightRule::new("super", H::Keyword),
    HighlightRule::new("crate", H::Keyword),
    HighlightRule::new("mutable_specifier", H::Keyword),
];

pub(crate) const GRAMMAR: Grammar = Grammar {
    symbols: SYMBOLS,
    identifiers: IDENTIFIERS,
    imports: IMPORTS,
    highlights: HIGHLIGHTS,
};
