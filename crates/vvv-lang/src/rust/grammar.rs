//! What in tree-sitter-rust's grammar counts as a declaration or an
//! identifier, and what its modifiers mean.

use vvv_core::HighlightKind as H;
use vvv_core::ReachKind as R;
use vvv_core::SignatureRule;
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
        .name_inner("type")
        .companion_of(&[Struct, Enum, TypeAlias]),
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
.module_bindings("source_file")
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
    macro_scopes: Some(vvv_core::MacroScopeRule {
        node: "macro_invocation",
        scopes: &["block"],
        expression_containers: &["arguments", "tuple_expression", "array_expression"],
        expression_fields: &[
            ("let_declaration", "value"),
            ("binary_expression", "left"),
            ("binary_expression", "right"),
            ("assignment_expression", "right"),
            ("call_expression", "function"),
            ("field_expression", "value"),
            ("index_expression", "index"),
        ],
    }),
    binding_markers: &["mutable_specifier"],
    import_scopes: &["block"],
    module_scopes: Some(vvv_core::ModuleScopeRule {
        node: "mod_item",
        name: "name",
        body: "body",
    }),
    signatures: &[
        SignatureRule::header("function_item", "body"),
        SignatureRule::whole("function_signature_item"),
        SignatureRule::header("struct_item", "body").body_kind("field_declaration_list"),
        SignatureRule::header("union_item", "body"),
        SignatureRule::header("enum_item", "body"),
        SignatureRule::header("trait_item", "body"),
        SignatureRule::header("impl_item", "body"),
        SignatureRule::header("mod_item", "body"),
        SignatureRule::whole("type_item"),
        SignatureRule::whole("field_declaration"),
    ],
    navigation_values: &["identifier"],
    calls: &[vvv_core::CallRule {
        node: "call_expression",
        callee: "function",
    }],
    callees: &[
        vvv_core::CalleeRule {
            node: "generic_function",
            field: "function",
            kind: None,
        },
        vvv_core::CalleeRule {
            node: "scoped_identifier",
            field: "name",
            kind: Some(vvv_core::CallKind::Direct),
        },
        vvv_core::CalleeRule {
            node: "field_expression",
            field: "field",
            kind: Some(vvv_core::CallKind::Member),
        },
    ],
    anonymous_callables: &["closure_expression"],
    pattern_constructors: &[("tuple_struct_pattern", "type")],
    pattern_containers: &[
        "tuple_struct_pattern",
        "tuple_pattern",
        "slice_pattern",
        "reference_pattern",
        "mut_pattern",
        "closure_parameters",
    ],
    qualified_imports: &[],
    bindings: &[
        vvv_core::BindingRule {
            node: "closure_expression",
            name: Some("parameters"),
            scopes: &["closure_expression"],
            kind: Parameter,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "type_parameter",
            name: Some("name"),
            scopes: &[
                "function_item",
                "function_signature_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "impl_item",
                "type_item",
            ],
            kind: TypeParameter,
            namespace: vvv_core::BindingNamespace::Type,
            after: false,
        },
        vvv_core::BindingRule {
            node: "parameter",
            name: Some("pattern"),
            scopes: &[
                "function_item",
                "function_signature_item",
                "closure_expression",
            ],
            kind: Parameter,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "let_declaration",
            name: Some("pattern"),
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: true,
        },
        vvv_core::BindingRule {
            node: "type_item",
            name: Some("name"),
            scopes: &["block"],
            kind: TypeAlias,
            namespace: vvv_core::BindingNamespace::Type,
            after: false,
        },
        vvv_core::BindingRule {
            node: "struct_item",
            name: Some("name"),
            scopes: &["block"],
            kind: Struct,
            namespace: vvv_core::BindingNamespace::Type,
            after: false,
        },
        vvv_core::BindingRule {
            node: "enum_item",
            name: Some("name"),
            scopes: &["block"],
            kind: Enum,
            namespace: vvv_core::BindingNamespace::Type,
            after: false,
        },
        vvv_core::BindingRule {
            node: "function_item",
            name: Some("name"),
            scopes: &["block"],
            kind: Function,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "use_declaration",
            name: None,
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "extern_crate_declaration",
            name: None,
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "struct_item",
            name: None,
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "const_item",
            name: None,
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "static_item",
            name: None,
            scopes: &["block"],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
        vvv_core::BindingRule {
            node: "const_parameter",
            name: None,
            scopes: &[
                "function_item",
                "struct_item",
                "enum_item",
                "impl_item",
                "trait_item",
                "type_item",
            ],
            kind: Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        },
    ],
    lexical_qualified: &[
        "field_expression",
        "scoped_identifier",
        "scoped_type_identifier",
        "lifetime",
    ],
    lexical_boundaries: &[
        "type_item",
        "const_item",
        "static_item",
        "function_item",
        "struct_item",
        "enum_item",
        "impl_item",
        "trait_item",
        "mod_item",
    ],
    lexical_containers: &["impl_item", "trait_item"],
    lexical_barriers: &[
        "match_arm",
        "for_expression",
        "if_expression",
        "while_expression",
        "macro_invocation",
        "mod_item",
    ],
    named_imports: &[],
    named_modules: false,
    non_named_exports: &[],
    symbols: SYMBOLS,
    identifiers: IDENTIFIERS,
    imports: IMPORTS,
    highlights: HIGHLIGHTS,
    navigation_types: &["type_identifier"],
    navigation_barriers: &["mod_item", "macro_invocation"],
    navigation_bindings: &[],
};
