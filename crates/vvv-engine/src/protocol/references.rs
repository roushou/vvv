use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use vvv_core::{LanguageId, SymbolKind};

/// The declaration(s) called `name`, and every identifier that spells it,
/// each judged against the declaration meant. What `rename` gathers before
/// it plans, and what `references` answers on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferencesQuery {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
}

impl ReferencesQuery {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            symbol: None,
            language: None,
            declared_in: None,
        }
    }

    pub fn declared_in(mut self, path: impl Into<RelPath>) -> Self {
        self.declared_in = Some(path.into());
        self
    }

    pub fn of_symbol(mut self, symbol: SymbolKind) -> Self {
        self.symbol = Some(symbol);
        self
    }

    pub fn in_language(mut self, language: impl Into<LanguageId>) -> Self {
        self.language = Some(language.into());
        self
    }
}
