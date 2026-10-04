//! Everything a language can say about one file from one parse.
//!
//! Plain data: the declarations, the import paths, the highlights, and every
//! identifier token with its text interned. One parse serves every question
//! a command asks of a file, and a model can keep facts between questions.

use serde::{Deserialize, Serialize};

use crate::highlight::Highlight;
use crate::import::ImportRef;
use crate::symbol::Symbol;
use crate::text::{Span, SpanError};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Facts {
    #[serde(default)]
    pub patterns: Vec<crate::PatternScope>,
    #[serde(default)]
    pub pattern_constructors: Vec<crate::PatternConstructor>,
    #[serde(default)]
    pub import_scopes: Vec<crate::ImportScope>,
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
    #[serde(flatten)]
    interned: TokenTable,
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

/// Invalid evidence from a plugin or serialized facts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FactsError {
    #[error("token {token} refers to missing {table} entry {index}")]
    InvalidToken {
        token: usize,
        table: &'static str,
        index: usize,
    },
    #[error(transparent)]
    Span(#[from] SpanError),
}

/// Interned identifier texts and node kinds, with references validated on input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(try_from = "TokenTableData")]
struct TokenTable {
    names: Vec<String>,
    kinds: Vec<String>,
    tokens: Vec<Token>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct TokenTableData {
    names: Vec<String>,
    kinds: Vec<String>,
    tokens: Vec<Token>,
}

impl TryFrom<TokenTableData> for TokenTable {
    type Error = FactsError;

    fn try_from(data: TokenTableData) -> Result<Self, Self::Error> {
        for (token, value) in data.tokens.iter().enumerate() {
            for (table, index, len) in [
                ("names", value.name as usize, data.names.len()),
                ("kinds", value.kind as usize, data.kinds.len()),
            ] {
                if index >= len {
                    return Err(FactsError::InvalidToken {
                        token,
                        table,
                        index,
                    });
                }
            }
        }
        Ok(Self {
            names: data.names,
            kinds: data.kinds,
            tokens: data.tokens,
        })
    }
}

impl TokenTable {
    fn push(&mut self, name: &str, kind: &str, span: Span) {
        let name = u32::try_from(Self::intern(&mut self.names, name))
            .expect("identifier table exceeds u32 capacity");
        let kind = u16::try_from(Self::intern(&mut self.kinds, kind))
            .expect("node kind table exceeds u16 capacity");
        self.tokens.push(Token { name, kind, span });
    }

    fn intern(table: &mut Vec<String>, text: &str) -> usize {
        match table.iter().position(|value| value == text) {
            Some(index) => index,
            None => {
                table.push(text.to_owned());
                table.len() - 1
            }
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &str, Span)> + '_ {
        self.tokens.iter().map(|token| {
            (
                self.names[token.name as usize].as_str(),
                self.kinds[token.kind as usize].as_str(),
                token.span,
            )
        })
    }

    fn named<'a>(&'a self, name: &str) -> impl Iterator<Item = (Span, &'a str)> + 'a {
        let id = self
            .names
            .iter()
            .position(|value| value == name)
            .and_then(|index| u32::try_from(index).ok());
        self.tokens
            .iter()
            .filter(move |token| Some(token.name) == id)
            .map(|token| (token.span, self.kinds[token.kind as usize].as_str()))
    }
}

impl Facts {
    /// Check every source coordinate before these facts enter an engine snapshot.
    pub fn validate_in(&self, source: &str) -> Result<(), FactsError> {
        let binding = |binding: &crate::ImportBinding| -> Result<(), SpanError> {
            binding.span.validate_in(source)?;
            if let Some(modifier) = &binding.visibility {
                modifier.span.validate_in(source)?;
            }
            Ok(())
        };
        for symbol in self
            .symbols
            .iter()
            .chain(self.lexical.iter().map(|binding| &binding.symbol))
        {
            symbol.validate_in(source)?;
        }
        for span in self
            .navigation
            .iter()
            .chain(&self.navigation_types)
            .chain(&self.lexical_tokens)
            .chain(&self.non_named_exports)
            .copied()
            .chain(self.interned.tokens.iter().map(|token| token.span))
            .chain(self.highlights.iter().map(|highlight| highlight.span))
        {
            span.validate_in(source)?;
        }
        for import in &self.imports {
            import.span.validate_in(source)?;
            if let Some(group) = &import.group {
                for span in [group.item, group.list, group.statement] {
                    span.validate_in(source)?;
                }
            }
        }
        for item in &self.import_bindings {
            binding(item)?;
        }
        for scope in &self.import_scopes {
            scope.span.validate_in(source)?;
            for item in &scope.imports {
                binding(item)?;
            }
            for span in &scope.aliases {
                span.validate_in(source)?;
            }
        }
        for scope in &self.module_scopes {
            scope.span.validate_in(source)?;
            if let Some(span) = scope.declaration {
                span.validate_in(source)?;
            }
            for item in &scope.imports {
                binding(item)?;
            }
            for declaration in &scope.declarations {
                declaration.name_span.validate_in(source)?;
            }
        }
        for lexical in &self.lexical {
            lexical.scope.validate_in(source)?;
            for span in lexical.excluded.iter().chain(&lexical.uninitialized) {
                span.validate_in(source)?;
            }
        }
        for uncertainty in &self.scope_uncertainties {
            uncertainty.scope.validate_in(source)?;
            uncertainty.invocation.validate_in(source)?;
        }
        for import in &self.named_imports {
            import.name_span.validate_in(source)?;
            import.alias_span.validate_in(source)?;
        }
        for import in &self.qualified_imports {
            import.span.validate_in(source)?;
        }
        for signature in &self.signatures {
            signature.name_span.validate_in(source)?;
            signature.span.validate_in(source)?;
        }
        for call in &self.calls {
            call.span.validate_in(source)?;
            call.callee.validate_in(source)?;
            if let Some(span) = call.owner {
                span.validate_in(source)?;
            }
        }
        for pieces in &self.declaration_pieces {
            pieces.declaration.validate_in(source)?;
            pieces.scope.validate_in(source)?;
            for companion in &pieces.companions {
                companion.span.validate_in(source)?;
            }
        }
        for constructor in &self.pattern_constructors {
            constructor.name_span.validate_in(source)?;
            if let Some(span) = constructor.owner {
                span.validate_in(source)?;
            }
        }
        for pattern in &self.patterns {
            pattern.span.validate_in(source)?;
            pattern.scope.validate_in(source)?;
            for span in &pattern.excluded {
                span.validate_in(source)?;
            }
            for reference in &pattern.references {
                reference.span.validate_in(source)?;
            }
            for alternative in &pattern.alternatives {
                for branch in &alternative.branches {
                    for item in branch {
                        item.span.validate_in(source)?;
                    }
                }
            }
        }
        Ok(())
    }

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
    ///
    /// # Panics
    /// Panics if the intern tables exceed `u32` names or `u16` node kinds.
    pub fn push_token(&mut self, name: &str, kind: &str, span: Span) {
        self.interned.push(name, kind, span);
    }

    pub fn tokens(&self) -> impl Iterator<Item = (&str, &str, Span)> + '_ {
        self.interned.iter()
    }

    /// Every token spelling `name`, in source order, with its node kind.
    pub fn tokens_named<'a>(&'a self, name: &str) -> impl Iterator<Item = (Span, &'a str)> + 'a {
        self.interned.named(name)
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
    fn serialized_tokens_require_both_interned_entries() {
        let mut facts = Facts::default();
        facts.push_token("é", "identifier", Span::new(0, 2));
        let encoded = serde_json::to_value(&facts).unwrap();
        assert_eq!(
            serde_json::from_value::<Facts>(encoded.clone()).unwrap(),
            facts
        );
        for field in ["name", "kind"] {
            let mut invalid = encoded.clone();
            invalid["tokens"][0][field] = 1.into();
            assert!(serde_json::from_value::<Facts>(invalid).is_err(), "{field}");
        }
        facts.validate_in("é").unwrap();
        assert!(matches!(
            facts.validate_in(""),
            Err(FactsError::Span(SpanError::OutOfBounds { .. }))
        ));
    }

    #[test]
    fn tokens_are_interned_and_found_by_name() {
        let mut facts = Facts::default();
        facts.push_token("foo", "identifier", Span::new(0, 3));
        facts.push_token("bar", "identifier", Span::new(4, 7));
        facts.push_token("foo", "type_identifier", Span::new(8, 11));
        assert_eq!(facts.interned.names.len(), 2);
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
