//! The query bar: the CLI's search grammar plus filter words.
//!
//! `Language` is a name search, `fn $N($$$) { $$$ }` a structural pattern,
//! and `symbol:trait`, `name:Foo`, `kind:impl_item`, `lang:rust` may appear
//! anywhere in the line. Everything else is the pattern. The bar is the one
//! source of truth for the search; menus edit it rather than storing filters
//! elsewhere.

use vvv_engine::{LanguageId, Query, QueryError, SymbolKind};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryBar {
    text: String,
}

/// A filter word's key, with the short form it also accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Symbol,
    Name,
    Kind,
    Lang,
}

impl Filter {
    fn keys(self) -> [&'static str; 2] {
        match self {
            Self::Symbol => ["symbol", "s"],
            Self::Name => ["name", "n"],
            Self::Kind => ["kind", "k"],
            Self::Lang => ["lang", "l"],
        }
    }

    fn parse(word: &str) -> Option<(Self, &str)> {
        let (key, value) = word.split_once(':')?;
        [Self::Symbol, Self::Name, Self::Kind, Self::Lang]
            .into_iter()
            .find(|f| f.keys().contains(&key))
            .map(|f| (f, value))
    }
}

impl QueryBar {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn push(&mut self, c: char) {
        self.text.push(c);
    }

    pub fn pop(&mut self) {
        self.text.pop();
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The current value of a filter word, if present.
    pub fn filter(&self, filter: Filter) -> Option<&str> {
        self.text.split_whitespace().find_map(|w| {
            Filter::parse(w)
                .filter(|(f, _)| *f == filter)
                .map(|(_, v)| v)
        })
    }

    /// Replace (or with `None`, remove) a filter word, keeping the rest.
    pub fn set_filter(&mut self, filter: Filter, value: Option<&str>) {
        let mut words: Vec<String> = self
            .text
            .split_whitespace()
            .filter(|w| Filter::parse(w).is_none_or(|(f, _)| f != filter))
            .map(str::to_owned)
            .collect();
        if let Some(value) = value {
            words.push(format!("{}:{value}", filter.keys()[0]));
        }
        self.text = words.join(" ");
        if !self.text.is_empty() {
            self.text.push(' ');
        }
    }

    /// Filter words out, pattern in; `Err` for an unknown symbol kind.
    pub fn parse(&self) -> Result<Query, QueryParseError> {
        let mut pattern: Vec<&str> = Vec::new();
        let mut kind = None;
        let mut symbol = None;
        let mut name = None;
        let mut language = None;
        for word in self.text.split_whitespace() {
            match Filter::parse(word) {
                Some((Filter::Symbol, v)) => {
                    symbol = Some(
                        v.parse::<SymbolKind>()
                            .map_err(|e| QueryParseError::Filter(e.to_string()))?,
                    );
                }
                Some((Filter::Name, v)) => name = Some(v.to_owned()),
                Some((Filter::Kind, v)) => kind = Some(v.to_owned()),
                Some((Filter::Lang, v)) => language = Some(LanguageId::from(v)),
                None => pattern.push(word),
            }
        }
        let pattern = (!pattern.is_empty()).then(|| pattern.join(" "));
        Query::builder()
            .pattern(pattern)
            .kind(kind)
            .symbol(symbol)
            .name(name)
            .language(language)
            .build()
            .map_err(QueryParseError::Query)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum QueryParseError {
    Query(QueryError),
    Filter(String),
}

impl std::fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(e) => write!(f, "{e}"),
            Self::Filter(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(text: &str) -> QueryBar {
        QueryBar {
            text: text.to_owned(),
        }
    }

    #[test]
    fn bare_word_is_a_pattern() {
        assert_eq!(bar("Language").parse().unwrap(), Query::pattern("Language"));
    }

    #[test]
    fn filters_come_out_of_the_pattern() {
        let q = bar("symbol:method name:new lang:rust").parse().unwrap();
        assert_eq!(q.symbol(), Some(SymbolKind::Method));
        assert_eq!(q.name(), Some("new"));
        assert_eq!(q.language().map(|l| l.as_str()), Some("rust"));
        assert_eq!(q.pattern_str(), None);
    }

    #[test]
    fn pattern_and_filters_mix() {
        let q = bar("fn $N($$$) { $$$ } k:function_item s:method")
            .parse()
            .unwrap();
        assert_eq!(q.pattern_str(), Some("fn $N($$$) { $$$ }"));
        assert_eq!(q.kind_str(), Some("function_item"));
        assert_eq!(q.symbol(), Some(SymbolKind::Method));
    }

    #[test]
    fn empty_and_bad_filters_are_errors() {
        assert!(matches!(bar("").parse(), Err(QueryParseError::Query(_))));
        assert!(matches!(
            bar("s:nope").parse(),
            Err(QueryParseError::Filter(_))
        ));
    }

    #[test]
    fn set_filter_edits_the_line_in_place() {
        let mut q = bar("Foo s:struct lang:rust");
        q.set_filter(Filter::Symbol, Some("trait"));
        assert_eq!(q.text(), "Foo lang:rust symbol:trait ");
        assert_eq!(q.filter(Filter::Symbol), Some("trait"));
        q.set_filter(Filter::Lang, None);
        assert_eq!(q.text(), "Foo symbol:trait ");
        q.set_filter(Filter::Symbol, None);
        assert_eq!(q.text(), "Foo ");
    }
}
