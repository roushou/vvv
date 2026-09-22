use std::path::Path;
use std::sync::Arc;

use super::{Language, LanguageId};

/// The set of languages an engine knows about. Filled at startup; read-only after.
#[derive(Clone, Default)]
pub struct LanguageRegistry {
    languages: Vec<Arc<dyn Language>>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, language: impl Language + 'static) -> Self {
        self.languages.push(Arc::new(language));
        self
    }

    pub fn get(&self, id: &LanguageId) -> Option<Arc<dyn Language>> {
        self.languages.iter().find(|l| &l.id() == id).cloned()
    }

    pub fn for_path(&self, path: &Path) -> Option<Arc<dyn Language>> {
        let ext = path.extension()?.to_str()?;
        self.languages
            .iter()
            .find(|l| l.extensions().contains(&ext))
            .cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Language>> {
        self.languages.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.languages.is_empty()
    }
}

impl std::fmt::Debug for LanguageRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.languages.iter().map(|l| l.id()))
            .finish()
    }
}
