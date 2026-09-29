//! File preview with syntax colouring and declarations from one source snapshot.
use crate::EngineError;
use serde::{Deserialize, Serialize};
use vvv_core::{Highlight, RelPath, Span, Symbol, SymbolKind};

/// One file as it is now, with its syntax colouring: what a picker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileQuery {
    pub path: RelPath,
}

/// One file as it is, with syntax colouring and declaration ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct File {
    pub path: RelPath,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<Highlight>,
    /// Declarations from the same source snapshot as the text and highlights.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
    /// Versioned identifier occurrences from this exact source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifiers: Vec<crate::SourceAnchor>,
}

impl File {
    /// The nearest declaration of `kind` strictly enclosing this source range.
    pub fn enclosing(&self, span: Span, kind: SymbolKind) -> Option<&Symbol> {
        if span.is_empty() || self.text.get(span.start..span.end).is_none() {
            return None;
        }
        self.symbols
            .iter()
            .filter(|symbol| {
                symbol.kind == kind
                    && self.text.get(symbol.span.start..symbol.span.end).is_some()
                    && symbol.span != span
                    && symbol.span.start <= span.start
                    && span.end <= symbol.span.end
            })
            .min_by_key(|symbol| symbol.span.end - symbol.span.start)
    }
}

/// One file as it is now, coloured by its language when one claims it.
impl FileQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<File, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine.workspace(), engine.languages())
    }

    pub(crate) fn execute_in(
        self,
        workspace: &crate::Workspace,
        languages: &vvv_core::LanguageRegistry,
    ) -> Result<File, EngineError> {
        let path = self.path.as_path();
        let file = workspace.load(path)?;
        let (highlights, symbols, identifiers) = match languages.for_path(path) {
            Some(language) => {
                let facts = language
                    .facts(file.text())
                    .map_err(|source| EngineError::Search {
                        path: path.into(),
                        source,
                    })?;
                let identifiers = facts
                    .tokens()
                    .map(|(_, _, span)| crate::SourceAnchor {
                        path: file.path().into(),
                        content: file.content_id(),
                        span,
                    })
                    .collect();
                (facts.highlights, facts.symbols, identifiers)
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
        Ok(File {
            path: file.path().into(),
            text: file.text().to_owned(),
            highlights,
            symbols,
            identifiers,
        })
    }
}
