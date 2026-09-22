use vvv_core::RelPath;

use serde::{Deserialize, Serialize};

use crate::{ReferencesQuery, Selection};
use vvv_core::{LanguageId, SymbolKind};

/// Rename the declaration(s) called `name` and every identifier that spells it.
///
/// Resolution is syntactic: every occurrence of the identifier in files of
/// the declaring language is an occurrence. The `selection` is how a human or
/// agent excludes the ones that are not the same symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameIntent {
    pub name: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
    #[serde(default, skip_serializing_if = "Selection::is_all")]
    pub selection: Selection,
}

impl RenameIntent {
    pub fn new(name: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            to: to.into(),
            symbol: None,
            language: None,
            declared_in: None,
            selection: Selection::All,
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

    pub fn selecting(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }

    /// The question a rename asks before it plans.
    pub fn references(&self) -> ReferencesQuery {
        ReferencesQuery {
            name: self.name.clone(),
            symbol: self.symbol,
            language: self.language.clone(),
            declared_in: self.declared_in.clone(),
        }
    }
}
