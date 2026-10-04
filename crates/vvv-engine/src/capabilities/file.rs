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
        self.execute_in(engine)
    }

    pub(crate) fn execute_in(self, engine: &crate::Engine) -> Result<File, EngineError> {
        let path = self.path.as_path();
        // Always read current contents, even within a session's trusted walk.
        // Cached parser output proves nothing about the filesystem by itself.
        let file = engine.workspace().load(path)?;
        let content = file.content_id();
        let normalized = RelPath::from(file.path());
        if let Some(cached) = engine
            .file_previews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&normalized, &content)
        {
            return Ok(cached);
        }
        let (highlights, symbols, identifiers) = match engine.languages().for_path(path) {
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
        let preview = File {
            path: normalized,
            text: file.text().to_owned(),
            highlights,
            symbols,
            identifiers,
        };
        engine
            .file_previews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(&preview, content);
        Ok(preview)
    }
}

/// Parsed file snapshots, independent of graph trust. Keys require a fresh read
/// and a complete content digest; edits and deletes cannot hit an old snapshot.
pub(crate) struct FileCache {
    entries: std::collections::VecDeque<CachedFile>,
    bytes: usize,
    max_bytes: usize,
    max_entries: usize,
}

struct CachedFile {
    file: File,
    content: crate::ContentId,
    bytes: usize,
}

impl Default for FileCache {
    fn default() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
            bytes: 0,
            max_bytes: 64 * 1024 * 1024,
            max_entries: 32,
        }
    }
}

impl FileCache {
    fn get(&mut self, path: &RelPath, content: &crate::ContentId) -> Option<File> {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.file.path == path)?;
        let entry = self.entries.remove(index)?;
        if &entry.content != content {
            self.bytes -= entry.bytes;
            return None;
        }
        let file = entry.file.clone();
        self.entries.push_back(entry);
        Some(file)
    }

    fn insert(&mut self, file: &File, content: crate::ContentId) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.file.path == file.path)
            && let Some(entry) = self.entries.remove(index)
        {
            self.bytes -= entry.bytes;
        }
        let mut charge = FileCharge::default();
        if serde_json::to_writer(&mut charge, file).is_err() {
            return;
        }
        let bytes = charge.0.saturating_mul(4).saturating_add(256);
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        while self.bytes.saturating_add(bytes) > self.max_bytes
            || self.entries.len() >= self.max_entries
        {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= entry.bytes;
        }
        self.bytes += bytes;
        self.entries.push_back(CachedFile {
            file: file.clone(),
            content,
            bytes,
        });
    }
}

/// Conservative serialized-payload accounting without allocating another copy.
#[derive(Default)]
struct FileCharge(usize);

impl std::io::Write for FileCharge {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_least_recent_files_and_skips_oversized_text_and_metadata() {
        let mut cache = FileCache {
            max_bytes: 4_096,
            max_entries: 2,
            ..FileCache::default()
        };
        let file = |path: &str, text: &str| File {
            path: path.into(),
            text: text.into(),
            highlights: vec![],
            symbols: vec![],
            identifiers: vec![],
        };
        let content = crate::ContentId::of("small");
        cache.insert(&file("a.p", "small"), content.clone());
        cache.insert(&file("b.p", "small"), content.clone());
        assert!(cache.get(&"a.p".into(), &content).is_some());
        cache.insert(&file("c.p", "small"), content.clone());
        assert!(cache.get(&"b.p".into(), &content).is_none());
        assert!(cache.get(&"a.p".into(), &content).is_some());
        let large = file("large.p", &"x".repeat(4_096));
        cache.insert(&large, crate::ContentId::of(&large.text));
        assert!(
            cache
                .get(&large.path, &crate::ContentId::of(&large.text))
                .is_none()
        );
        let mut metadata = file("metadata.p", "small");
        metadata.identifiers = (0..100)
            .map(|_| crate::SourceAnchor {
                path: metadata.path.clone(),
                content: content.clone(),
                span: Span::new(0, 1),
            })
            .collect();
        cache.insert(&metadata, content.clone());
        assert!(cache.get(&metadata.path, &content).is_none());
        assert!(cache.get(&"a.p".into(), &content).is_some());
        assert!(cache.get(&"c.p".into(), &content).is_some());
        cache.max_entries = 32;
        for i in 0..32 {
            cache.insert(&file(&format!("{i}.p"), &"x".repeat(100)), content.clone());
            assert!(cache.bytes <= cache.max_bytes);
        }
        assert!(
            cache.entries.len() < cache.max_entries,
            "the byte budget also evicts"
        );
    }
}
