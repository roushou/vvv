//! Bounded search recipes, independent of retained source and browsing pages.
use super::{Category, Search, browse::BrowsePage, query::QueryBar};
use serde::{Deserialize, Serialize};
use vvv_engine::RelPath;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRecipe {
    pub query: String,
    pub location: Option<RelPath>,
    pub category: String,
    pub files: String,
}
impl SearchRecipe {
    pub fn capture(search: &Search) -> Option<Self> {
        (matches!(search.page, BrowsePage::Search)
            && !search.stale
            && !search.query.is_empty()
            && search.results.query.is_some()
            && search.query.parse().ok().as_ref() == search.results.query.as_ref())
        .then(|| Self {
            query: search.query.text().to_owned(),
            location: search.locations.selected.clone(),
            category: search.results.category.key().into(),
            files: search.results.files.filter.clone(),
        })
    }
    pub fn valid(&self) -> bool {
        self.query.len() + self.files.len() + self.location.as_ref().map_or(0, |p| p.as_str().len())
            < 8192
            && QueryBar::from(self.query.clone()).parse().is_ok()
            && Category::from_key(&self.category).is_some()
            && vvv_engine::SearchScope {
                paths: self.location.iter().cloned().collect(),
                packages: vec![],
            }
            .validate()
            .is_ok()
    }
    pub fn label(&self) -> String {
        let mut label = format!(
            "{} · in: {} · {}",
            self.query.trim(),
            self.location.as_ref().map_or("workspace", |p| p.as_str()),
            self.category
        );
        if !self.files.is_empty() {
            label.push_str(&format!(" · files: {}", self.files));
        }
        label
    }
    pub fn restore(&self, search: &mut Search) {
        search.query = QueryBar::from(self.query.clone());
        // Recipes are validated on load and selection; location suggestions stay live.
        search
            .locations
            .select(self.location.as_ref().map(|p| p.as_str()))
            .expect("validated recipe");
        search.results.leave();
        search.results.set_location(self.location.clone());
        search.results.files.filter = self.files.clone();
        search.results.files.edit = None;
        search
            .results
            .set_category(Category::from_key(&self.category).expect("validated category"));
        search.expanded = None;
        search.focus = super::SearchPanel::Query;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentSearches {
    entries: Vec<SearchRecipe>,
    #[serde(skip)]
    forgotten: Vec<SearchRecipe>,
}
impl RecentSearches {
    pub fn remember(&mut self, recipe: SearchRecipe) {
        if !recipe.valid() || self.forgotten.contains(&recipe) {
            return;
        }
        self.entries.retain(|old| old != &recipe);
        self.entries.insert(0, recipe);
        self.entries.truncate(24);
    }
    pub fn searched(&mut self, recipe: SearchRecipe) {
        self.forgotten.retain(|r| r != &recipe);
        self.remember(recipe);
    }
    pub fn entries(&self) -> &[SearchRecipe] {
        &self.entries
    }
    pub fn forget(&mut self, recipe: &SearchRecipe) {
        self.entries.retain(|r| r != recipe);
        self.forgotten.retain(|r| r != recipe);
        self.forgotten.push(recipe.clone());
        if self.forgotten.len() > 24 {
            self.forgotten.remove(0);
        }
    }
    pub fn validated(self) -> Self {
        let mut recent = Self::default();
        for recipe in self.entries.into_iter().take(24).rev() {
            recent.remember(recipe);
        }
        recent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_recipes_are_deduplicated_bounded_and_validate_workspace_scopes() {
        let mut recent = RecentSearches::default();
        for i in 0..30 {
            recent.remember(SearchRecipe {
                query: format!("Name{i}"),
                location: None,
                category: "all".into(),
                files: String::new(),
            });
        }
        assert_eq!(recent.entries().len(), 24);
        let recipe = recent.entries()[10].clone();
        recent.remember(recipe.clone());
        assert_eq!(recent.entries().len(), 24);
        assert_eq!(recent.entries()[0], recipe);
        let mut invalid = recipe.clone();
        invalid.location = Some("../outside".into());
        recent.remember(invalid);
        assert_eq!(recent.entries()[0], recipe);
        assert_eq!(recent.validated().entries().len(), 24);
    }
}
