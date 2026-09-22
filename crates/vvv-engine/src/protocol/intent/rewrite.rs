use serde::{Deserialize, Serialize};

use crate::{Selection, Template};
use vvv_core::Query;

/// The rewrite `intent` makes of `matches` already found: a caller that
/// keeps the matches (the picker, showing before and after) searches once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteOf {
    pub intent: RewriteIntent,
    pub matches: Vec<crate::Match>,
}

/// Replace every selected match of `query` with `template`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteIntent {
    pub query: Query,
    pub template: Template,
    #[serde(default, skip_serializing_if = "Selection::is_all")]
    pub selection: Selection,
}

impl RewriteIntent {
    pub fn new(query: Query, template: impl Into<Template>) -> Self {
        Self {
            query,
            template: template.into(),
            selection: Selection::All,
        }
    }

    pub fn selecting(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }
}
