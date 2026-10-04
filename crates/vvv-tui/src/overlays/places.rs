//! A searchable browsing trail and recent search recipes.
use crate::model::Cursor;
use crate::modes::search::{Search, recall::SearchRecipe};

#[derive(Debug, Clone)]
pub enum Place {
    Trail(i32),
    Recent(SearchRecipe),
}
#[derive(Debug, Clone)]
pub struct PlaceItem {
    pub label: String,
    pub place: Place,
}
#[derive(Debug, Clone)]
pub struct Places {
    pub recent: bool,
    pub filter: String,
    pub caret: crate::input::Caret,
    pub cursor: Cursor,
    pub trail: Vec<PlaceItem>,
    pub searches: Vec<PlaceItem>,
}
impl Places {
    pub fn new(search: &Search) -> Self {
        let trail: Vec<_> = search
            .trail
            .locations(search)
            .into_iter()
            .map(|(step, label)| PlaceItem {
                label: if step == 0 {
                    format!("● {label}")
                } else {
                    format!("  {label}")
                },
                place: Place::Trail(step),
            })
            .collect();
        let index = trail
            .iter()
            .position(|i| matches!(i.place, Place::Trail(0)))
            .unwrap_or(0);
        Self {
            recent: false,
            filter: String::new(),
            caret: Default::default(),
            cursor: Cursor { index },
            trail,
            searches: search
                .recent
                .entries()
                .iter()
                .map(|r| PlaceItem {
                    label: r.label(),
                    place: Place::Recent(r.clone()),
                })
                .collect(),
        }
    }
    pub fn tab(&mut self) {
        self.recent = !self.recent;
        self.filter.clear();
        self.caret.reset();
        self.cursor.index = 0;
    }
    pub fn visible(&self) -> Vec<&PlaceItem> {
        let words: Vec<_> = self
            .filter
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        (if self.recent {
            &self.searches
        } else {
            &self.trail
        })
        .iter()
        .filter(|i| {
            let label = i.label.to_lowercase();
            words.iter().all(|word| label.contains(word))
        })
        .collect()
    }
    pub fn chosen(&self) -> Option<Place> {
        self.visible()
            .get(self.cursor.index)
            .map(|i| i.place.clone())
    }
    pub fn moved(&mut self, by: i32) {
        self.cursor.move_by(by, self.visible().len());
    }
    pub fn input(&mut self, action: crate::action::Action) {
        match action {
            crate::action::Action::Input(c) => self.filter.push(c),
            crate::action::Action::Backspace => {
                self.filter.pop();
            }
            crate::action::Action::Clear => self.filter.clear(),
            _ => {}
        }
        self.cursor.index = 0;
    }
    pub fn forget(&mut self, recipe: &SearchRecipe) {
        self.searches
            .retain(|i| !matches!(&i.place, Place::Recent(r) if r == recipe));
        self.cursor.clamp(self.visible().len());
    }
}
