//! Restrictions shared by the search border and its clearing menu.
use super::{Category, Search, query::Filter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restriction {
    Location,
    Symbol,
    Language,
    Syntax,
    Category,
    Files,
}

impl Restriction {
    pub const ALL: &[Self] = &[
        Self::Location,
        Self::Symbol,
        Self::Language,
        Self::Syntax,
        Self::Category,
        Self::Files,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Location => "in",
            Self::Symbol => "symbol",
            Self::Language => "lang",
            Self::Syntax => "node",
            Self::Category => "category",
            Self::Files => "files",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Location => "Location",
            Self::Symbol => "Symbol kind",
            Self::Language => "Language",
            Self::Syntax => "Node kind",
            Self::Category => "Category",
            Self::Files => "File filter",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.key() == key)
    }

    pub fn value(self, search: &Search) -> Option<String> {
        match self {
            Self::Location => search.locations.selected.as_ref().map(ToString::to_string),
            Self::Symbol => search.query.filter(Filter::Symbol).map(str::to_owned),
            Self::Language => search.query.filter(Filter::Lang).map(str::to_owned),
            Self::Syntax => search.query.filter(Filter::Kind).map(str::to_owned),
            Self::Category => (search.results.category != Category::All)
                .then(|| search.results.category.label().to_lowercase()),
            Self::Files => (!search.results.files.filter.trim().is_empty())
                .then(|| search.results.files.filter.clone()),
        }
    }

    /// Clear one restriction, returning whether the engine search must rerun.
    pub fn clear(self, search: &mut Search) -> bool {
        match self {
            Self::Location => {
                search.locations.selected = None;
                search.results.set_location(None);
                !search.results.is_anchored() || !search.results.relation.is_references()
            }
            Self::Symbol | Self::Language | Self::Syntax => {
                search.query.set_filter(
                    match self {
                        Self::Symbol => Filter::Symbol,
                        Self::Language => Filter::Lang,
                        _ => Filter::Kind,
                    },
                    None,
                );
                if self == Self::Symbol {
                    search.results.set_category(Category::All);
                }
                true
            }
            Self::Category => {
                search.results.set_category(Category::All);
                if search.query.filter(Filter::Symbol).is_some() {
                    search.query.set_filter(Filter::Symbol, None);
                    true
                } else {
                    false
                }
            }
            Self::Files => {
                let selected = search.results.current().map(|m| m.id.clone());
                search.results.files.filter.clear();
                search.results.files.edit = None;
                search.results.restore_selection(selected);
                false
            }
        }
    }
}
