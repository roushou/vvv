//! Lexical and imported bindings used by navigation, independent of mutation scope.

use crate::{ImportBinding, ModulePath, Name, Span, Symbol, SymbolKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BindingNamespace {
    Type,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LexicalBinding {
    pub symbol: Symbol,
    /// Lexical owner, including synthetic successful-condition scopes.
    pub scope: Span,
    pub excluded: Vec<Span>,
    /// Regions where the binding owns its name but is not yet initialized.
    /// They prevent falling through to an outer binding in a temporal dead zone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uninitialized: Vec<Span>,
    /// Start of lexical ownership: delayed for sequential bindings, or the scope
    /// start for parameters, generics and block-owned lexical declarations.
    pub visible_from: usize,
    pub namespace: BindingNamespace,
    /// Syntax forces a binding rather than a possible constant pattern.
    /// Older plugin facts remain conservative around unknown expansions.
    #[serde(default)]
    pub explicit: bool,
}

impl LexicalBinding {
    pub fn initialized(&self, span: Span) -> bool {
        !self
            .uninitialized
            .iter()
            .any(|region| region.contains(&span))
    }

    pub fn visible(&self, name: &str, span: Span, namespace: BindingNamespace) -> bool {
        self.symbol.name == name
            && self.namespace == namespace
            && self.scope.contains(&span)
            && self.visible_from <= span.start
            && !self.excluded.iter().any(|s| s.contains(&span))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamedImport {
    pub local: String,
    pub imported: String,
    pub module: Option<ModulePath>,
    pub name_span: Span,
    pub alias_span: Span,
    pub reexport: bool,
    pub type_only: bool,
}

/// A simple binding. Complex patterns remain unsupported rather than guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingRule {
    pub node: &'static str,
    /// `None` marks a binding form whose enclosing scope cannot be confirmed.
    pub name: Option<&'static str>,
    pub scopes: &'static [&'static str],
    pub kind: SymbolKind,
    pub namespace: BindingNamespace,
    pub after: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedImportRule {
    /// Field holding the imported name, or the first direct identifier child.
    pub name: Option<&'static str>,
    /// A fixed export name for default or namespace imports.
    pub imported: Option<&'static str>,
    pub node: &'static str,
    pub statement: &'static str,
    pub reexport: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QualifiedImport {
    pub span: Span,
    pub binding: String,
    pub member: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedImportRule {
    pub node: &'static str,
    pub object: &'static str,
    pub member: &'static str,
}

/// A declaration directly owned by a navigation module, with visibility evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModuleDeclaration {
    pub name_span: Span,
    pub restriction: Option<ModulePath>,
}

/// File-relative module ownership used only by navigation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModuleScope {
    pub path: Vec<Name>,
    pub span: Span,
    /// The inline module's name; absent for the file root.
    pub declaration: Option<Span>,
    pub declarations: Vec<ModuleDeclaration>,
    pub imports: Vec<ImportBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleScopeRule {
    pub node: &'static str,
    pub name: &'static str,
    pub body: &'static str,
}

/// Block-wide named imports for navigation, independent of mutation bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportScope {
    pub span: Span,
    pub imports: Vec<ImportBinding>,
    #[serde(default)]
    pub aliases: Vec<Span>,
}

/// A macro whose expansion may introduce items throughout a block and locals
/// after the invocation. This is navigation evidence, never mutation scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ScopeUncertainty {
    pub scope: Span,
    pub invocation: Span,
}

/// Macro positions whose expansion is required to be an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroScopeRule {
    pub node: &'static str,
    pub scopes: &'static [&'static str],
    pub expression_containers: &'static [&'static str],
    pub expression_fields: &'static [(&'static str, &'static str)],
}

/// How a pattern occurrence can use a name, independent of any parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PatternRole {
    Identifier,
    Constant,
    Constructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConstructorShape {
    Unit,
    Tuple,
    Record,
}

/// Constructor syntax on a declaration; an enum variant names its owning enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PatternConstructor {
    pub name_span: Span,
    pub owner: Option<Span>,
    pub shape: ConstructorShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PatternReference {
    pub span: Span,
    pub path: ModulePath,
    pub role: PatternRole,
    pub shape: Option<ConstructorShape>,
}

/// One possible binding site and its explicit syntactic mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PatternBinding {
    pub name: String,
    pub span: Span,
    pub by_ref: bool,
    pub mutable: bool,
}

/// Direct alternatives; nested alternatives have their own constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PatternAlternatives {
    pub branches: Vec<Vec<PatternBinding>>,
}

/// A supported pattern and the region reached by its possible bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PatternScope {
    pub span: Span,
    pub scope: Span,
    pub excluded: Vec<Span>,
    pub visible_from: usize,
    pub references: Vec<PatternReference>,
    pub alternatives: Vec<PatternAlternatives>,
}

impl PatternScope {
    pub fn contains(&self, span: Span) -> bool {
        self.span.contains(&span)
            || (self.scope.contains(&span)
                && self.visible_from <= span.start
                && !self
                    .excluded
                    .iter()
                    .any(|excluded| excluded.contains(&span)))
    }
}

#[cfg(test)]
mod initialization_tests {
    use super::*;

    #[test]
    fn initialization_regions_preserve_ownership_and_older_serialized_facts() {
        let mut binding = LexicalBinding {
            symbol: Symbol::plain(SymbolKind::Variable, "x", Span::new(5, 6), Span::new(5, 9)),
            scope: Span::new(0, 20),
            excluded: vec![],
            uninitialized: vec![],
            visible_from: 0,
            namespace: BindingNamespace::Value,
            explicit: true,
        };
        let serialized = serde_json::to_value(&binding).unwrap();
        assert!(serialized.get("uninitialized").is_none());
        let older: LexicalBinding = serde_json::from_value(serialized).unwrap();
        assert!(older.initialized(Span::new(2, 3)));
        binding.uninitialized.push(Span::new(0, 5));
        assert!(binding.visible("x", Span::new(2, 3), BindingNamespace::Value));
        assert!(!binding.initialized(Span::new(2, 3)));
        assert!(binding.initialized(Span::new(10, 11)));
        assert_eq!(
            serde_json::from_value::<LexicalBinding>(serde_json::to_value(&binding).unwrap())
                .unwrap(),
            binding
        );
    }
}
