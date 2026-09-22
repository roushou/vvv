use ast_grep_core::matcher::{KindMatcher, KindMatcherError};
use ast_grep_core::{Language, Pattern};
use vvv_core::{Query, SearchError};

/// The structural half of a [`Query`], resolved against one grammar.
pub(crate) enum CompiledQuery {
    Pattern(Pattern),
    Kind(KindMatcher),
    PatternOfKind { pattern: Pattern, kind: u16 },
}

impl CompiledQuery {
    /// `None` when the query has no structural part.
    pub(crate) fn compile<L: Language>(
        query: &Query,
        lang: &L,
    ) -> Result<Option<Self>, SearchError> {
        let pattern = query
            .pattern_str()
            .map(|p| {
                Pattern::try_new(p, lang.clone()).map_err(|e| SearchError::Pattern(e.to_string()))
            })
            .transpose()?;
        let kind = query
            .kind_str()
            .map(|k| {
                KindMatcher::try_new(k, lang.clone())
                    .map_err(|e: KindMatcherError| SearchError::Kind(e.to_string()))
            })
            .transpose()?;

        Ok(match (pattern, kind) {
            (Some(pattern), None) => Some(Self::Pattern(pattern)),
            (None, Some(kind)) => Some(Self::Kind(kind)),
            (Some(pattern), Some(_)) => Some(Self::PatternOfKind {
                pattern,
                kind: lang.kind_to_id(query.kind_str().unwrap_or_default()),
            }),
            (None, None) => None,
        })
    }
}
