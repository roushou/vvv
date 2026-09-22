//! What in tree-sitter-typescript's grammar counts as a declaration or an
//! identifier, and what its modifiers mean.

use vvv_core::HighlightKind as H;
use vvv_core::ModifierAt::Parent;
use vvv_core::ReachKind as R;
use vvv_core::SymbolKind::{self, *};
use vvv_core::{
    Grammar, HighlightRule, ImportGrammar, ImportRule, PathSyntax, ReExportRule, Semantics,
    SymbolRule, VisibilityRule,
};

/// Comments (JSDoc included) and decorators above a declaration belong to it.
const LEADING: &[&str] = &["comment", "decorator"];

/// A top-level declaration: `export` wraps it in an `export_statement`.
const fn decl(node: &'static str, kind: SymbolKind) -> SymbolRule {
    SymbolRule::new(node, "name", kind)
        .leading(LEADING)
        .visibility(Parent("export_statement"))
}

const SYMBOLS: &[SymbolRule] = &[
    decl("function_declaration", Function),
    decl("generator_function_declaration", Function),
    SymbolRule::new("method_definition", "name", Method).leading(LEADING),
    SymbolRule::new("method_signature", "name", Method).leading(LEADING),
    SymbolRule::new("abstract_method_signature", "name", Method).leading(LEADING),
    decl("class_declaration", Class),
    decl("abstract_class_declaration", Class),
    decl("interface_declaration", Interface),
    decl("type_alias_declaration", TypeAlias),
    decl("enum_declaration", Enum),
    SymbolRule::new("enum_assignment", "name", Variant).leading(LEADING),
    SymbolRule::self_named("property_identifier", Variant).under("enum_body"),
    decl("variable_declarator", Variable),
    SymbolRule::new("public_field_definition", "name", Field).leading(LEADING),
    SymbolRule::new("property_signature", "name", Field).leading(LEADING),
];

pub(crate) const SEMANTICS: Semantics = Semantics {
    // `import { X } from './b'` hides which names were taken, so a file import
    // is treated as opening the module.
    import_scopes_names: true,
    addressable: &[Function, Class, Interface, TypeAlias, Enum, Variable],
    // A file is a module: nothing crosses it without `export`.
    visibility: &[
        VisibilityRule::exact("export", R::Everyone),
        VisibilityRule::exact("export default", R::Everyone),
    ],
    default_visibility: R::Declaring,
};

const IDENTIFIERS: &[&str] = &[
    "identifier",
    "type_identifier",
    "property_identifier",
    "shorthand_property_identifier",
    "shorthand_property_identifier_pattern",
];

const IMPORTS: ImportGrammar = ImportGrammar::new(
    PathSyntax::Posix,
    &[
        ImportRule::field("import_statement", "source"),
        ImportRule::field("export_statement", "source"),
        ImportRule::pattern("import($PATH)", "PATH"),
        ImportRule::pattern("require($PATH)", "PATH"),
    ],
)
.reexports(ReExportRule::Statement("export_statement"));

const HIGHLIGHTS: &[HighlightRule] = &[
    HighlightRule::new("comment", H::Comment),
    HighlightRule::new("string", H::String),
    HighlightRule::new("template_string", H::String),
    HighlightRule::new("regex", H::String),
    HighlightRule::new("number", H::Number),
    HighlightRule::new("true", H::Number),
    HighlightRule::new("false", H::Number),
    HighlightRule::new("null", H::Number),
    HighlightRule::new("undefined", H::Number),
    HighlightRule::new("type_identifier", H::Type),
    HighlightRule::new("predefined_type", H::Type),
    HighlightRule::new("decorator", H::Attribute),
    HighlightRule::new("identifier", H::Function).under("function_declaration"),
    HighlightRule::new("identifier", H::Function).under("call_expression"),
    HighlightRule::new("property_identifier", H::Function).under("method_definition"),
    HighlightRule::new("property_identifier", H::Function).under("method_signature"),
    HighlightRule::new("this", H::Keyword),
    HighlightRule::new("super", H::Keyword),
];

pub(crate) const GRAMMAR: Grammar = Grammar {
    symbols: SYMBOLS,
    identifiers: IDENTIFIERS,
    imports: IMPORTS,
    highlights: HIGHLIGHTS,
};
