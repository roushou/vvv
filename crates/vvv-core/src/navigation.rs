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
    pub scope: Span,
    pub excluded: Vec<Span>,
    /// A local starts after its initializer; parameters and generics fill their scope.
    pub visible_from: usize,
    pub namespace: BindingNamespace,
    /// Syntax forces a binding rather than a possible constant pattern.
    /// Older plugin facts remain conservative around unknown expansions.
    #[serde(default)]
    pub explicit: bool,
}
impl LexicalBinding {
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
