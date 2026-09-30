//! Everything a language can say about one file from one parse.
//!
//! Plain data: the declarations, the import paths, the highlights, and every
//! identifier token with its text interned. One parse serves every question
//! a command asks of a file, and a model can keep facts between questions.

use serde::{Deserialize, Serialize};

use crate::highlight::Highlight;
use crate::import::ImportRef;
use crate::symbol::Symbol;
use crate::text::Span;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Facts {
    /// Explicit module ownership for navigation; mutation addresses are unchanged.
    #[serde(default)]
    pub module_scopes: Vec<crate::ModuleScope>,
    #[serde(default)]
    pub declaration_pieces: Vec<crate::DeclarationPieces>,
    /// Supported declaration headers; missing entries mean extraction is unsupported.
    #[serde(default)]
    pub signatures: Vec<crate::DeclarationSignature>,
    #[serde(default)]
    pub calls: Vec<crate::CallSite>,
    /// Whether this plugin supplies call classification.
    #[serde(default)]
    pub calls_supported: bool,
    /// Tokens whose context permits module/type resolution without local inference.
    #[serde(default)]
    pub navigation: Vec<Span>,
    /// Navigation tokens in the type namespace.
    #[serde(default)]
    pub navigation_types: Vec<Span>,
    #[serde(default)]
    pub lexical: Vec<crate::LexicalBinding>,
    /// Unknown macro-generated bindings in navigation scopes.
    #[serde(default)]
    pub scope_uncertainties: Vec<crate::ScopeUncertainty>,
    /// Tokens in structurally supported lexical contexts; scope uncertainty is separate.
    #[serde(default)]
    pub lexical_tokens: Vec<Span>,
    #[serde(default)]
    pub named_imports: Vec<crate::NamedImport>,
    #[serde(default)]
    pub named_modules: bool,
    #[serde(default)]
    pub qualified_imports: Vec<crate::QualifiedImport>,
    /// Declarations exported under a different name (for example a default export).
    #[serde(default)]
    pub non_named_exports: Vec<Span>,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<ImportRef>,
    #[serde(default)]
    pub import_bindings: Vec<crate::ImportBinding>,
    pub highlights: Vec<Highlight>,
    /// Interned identifier texts, indexed by [`Token::name`].
    names: Vec<String>,
    /// Interned node kinds, indexed by [`Token::kind`].
    kinds: Vec<String>,
    /// Every identifier token, in source order.
    tokens: Vec<Token>,
}

/// One identifier token: which name it spells, what the grammar calls it,
/// where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Token {
    name: u32,
    kind: u16,
    pub span: Span,
}

impl Facts {
    pub fn new(symbols: Vec<Symbol>, imports: Vec<ImportRef>, highlights: Vec<Highlight>) -> Self {
        Self {
            symbols,
            imports,
            highlights,
            ..Self::default()
        }
    }

    /// Navigation-only declarations do not change search or mutation scope.
    pub fn navigation_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter().chain(
            self.lexical
                .iter()
                .map(|binding| &binding.symbol)
                .filter(|s| {
                    !self
                        .symbols
                        .iter()
                        .any(|d| d.name_span == s.name_span && d.kind == s.kind)
                }),
        )
    }

    /// Record an identifier token. Tokens are expected in source order.
    pub fn push_token(&mut self, name: &str, kind: &str, span: Span) {
        let name = Self::intern(&mut self.names, name);
        let kind = Self::intern(&mut self.kinds, kind);
        self.tokens.push(Token {
            name: name as u32,
            kind: kind as u16,
            span,
        });
    }

    fn intern(table: &mut Vec<String>, text: &str) -> usize {
        match table.iter().position(|t| t == text) {
            Some(i) => i,
            None => {
                table.push(text.to_owned());
                table.len() - 1
            }
        }
    }

    pub fn tokens(&self) -> impl Iterator<Item = (&str, &str, Span)> + '_ {
        self.tokens.iter().map(|t| {
            (
                self.names[t.name as usize].as_str(),
                self.kinds[t.kind as usize].as_str(),
                t.span,
            )
        })
    }

    /// Every token spelling `name`, in source order, with its node kind.
    pub fn tokens_named<'a>(&'a self, name: &str) -> impl Iterator<Item = (Span, &'a str)> + 'a {
        let id = self.names.iter().position(|n| n == name).map(|i| i as u32);
        self.tokens
            .iter()
            .filter(move |t| Some(t.name) == id)
            .map(|t| (t.span, self.kinds[t.kind as usize].as_str()))
    }

    /// The declaration whose node or name is exactly `span`.
    pub fn symbol_at(&self, span: Span) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.span == span || s.name_span == span)
    }

    /// The innermost declaration whose extent contains `offset`.
    pub fn enclosing(&self, offset: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.extent.start <= offset && offset < s.extent.end)
            .min_by_key(|s| s.extent.end - s.extent.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolKind;

    #[test]
    fn tokens_are_interned_and_found_by_name() {
        let mut facts = Facts::default();
        facts.push_token("foo", "identifier", Span::new(0, 3));
        facts.push_token("bar", "identifier", Span::new(4, 7));
        facts.push_token("foo", "type_identifier", Span::new(8, 11));
        assert_eq!(facts.names.len(), 2);
        assert_eq!(
            facts.tokens_named("foo").collect::<Vec<_>>(),
            [
                (Span::new(0, 3), "identifier"),
                (Span::new(8, 11), "type_identifier")
            ]
        );
        assert_eq!(facts.tokens_named("nope").count(), 0);
    }

    #[test]
    fn enclosing_is_the_innermost_extent() {
        let outer = Symbol::plain(SymbolKind::Module, "m", Span::new(4, 5), Span::new(0, 40));
        let mut inner = Symbol::plain(
            SymbolKind::Function,
            "f",
            Span::new(13, 14),
            Span::new(10, 20),
        );
        inner.extent = Span::new(6, 20); // a doc comment before it
        let facts = Facts::new(vec![outer, inner], vec![], vec![]);
        assert_eq!(facts.enclosing(7).map(|s| s.name.as_str()), Some("f"));
        assert_eq!(facts.enclosing(30).map(|s| s.name.as_str()), Some("m"));
        assert_eq!(facts.enclosing(41), None);
    }
}
