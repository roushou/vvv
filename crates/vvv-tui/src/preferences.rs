//! Workspace preferences are data; the terminal session owns their storage.
use crate::model::{Model, ReportView};
use crate::modes::search::recall::RecentSearches;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Preferences {
    version: u8,
    root: String,
    split: u16,
    detailed: bool,
    definition: bool,
    recent: RecentSearches,
}
impl Preferences {
    pub fn capture(model: &Model) -> Self {
        let mut recent = model.search.recent.clone();
        if !model.status.busy
            && let Some(recipe) = crate::modes::search::recall::SearchRecipe::capture(&model.search)
        {
            recent.remember(recipe);
        }
        Self {
            version: 1,
            root: model.root.clone(),
            split: model.split,
            detailed: model.view == ReportView::Detailed,
            definition: model.search.definition_tab,
            recent,
        }
    }
    pub fn decode(bytes: &[u8], root: &str) -> Option<Self> {
        let preferences: Self = serde_json::from_slice(bytes).ok()?;
        (preferences.version == 1
            && preferences.root == root
            && (20..=80).contains(&preferences.split))
        .then_some(preferences)
    }
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("preferences contain only serializable data")
    }
    pub fn restore(self, model: &mut Model) {
        model.split = self.split;
        model.view = if self.detailed {
            ReportView::Detailed
        } else {
            ReportView::Compact
        };
        model.search.definition_tab = self.definition;
        model.search.recent = self.recent.validated();
        for recipe in model.search.recent.entries() {
            if let Some(path) = &recipe.location {
                model.search.locations.observe_path(path);
            }
        }
    }
}
