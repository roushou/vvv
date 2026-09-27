//! Pure selection and question transitions.
use super::{Menu, MenuTarget, Overlay};
use crate::modes::search::{Search, query::Filter};
use vvv_engine::report::{Detailed, Options, Source, View};
use vvv_engine::{RelPath, SymbolKind};
impl Overlay {
    fn report_sites(&self) -> Vec<Source> {
        let Self::Report { report, .. } = self else {
            return Vec::new();
        };
        Detailed
            .present(report, Options::default(), usize::MAX)
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect()
    }
    pub fn report_moved(&mut self, by: i32) {
        let last = self.report_sites().len().saturating_sub(1) as i32;
        if let Self::Report { cursor, .. } = self {
            *cursor = (*cursor as i32 + by).clamp(0, last) as usize;
        }
    }
    pub fn report_site(&self) -> Option<(RelPath, u32)> {
        let Self::Report { cursor, .. } = self else {
            return None;
        };
        self.report_sites()
            .get(*cursor)
            .map(|site| (site.path.clone(), site.line))
    }
    pub fn help_scrolled(&mut self, by: i32) -> bool {
        if let Self::Help { scroll, .. } = self {
            *scroll = (*scroll as i32 + by).max(0) as usize;
            true
        } else {
            false
        }
    }
}
impl Menu {
    pub fn for_target(target: MenuTarget, languages: &[String], search: &Search) -> Self {
        match target {
            MenuTarget::Symbol => {
                let values = SymbolKind::ALL
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect();
                let current = search.query.filter(Filter::Symbol).map(str::to_owned);
                Self::new(target, values, current.as_deref())
            }
            MenuTarget::Language => {
                let current = search.query.filter(Filter::Lang).map(str::to_owned);
                Self::new(target, languages.to_vec(), current.as_deref())
            }
            MenuTarget::Relation => Self::relations(search.results.relation),
        }
    }
    pub fn chosen(&self) -> (MenuTarget, Option<String>) {
        (self.target, self.current().value.clone())
    }
}
