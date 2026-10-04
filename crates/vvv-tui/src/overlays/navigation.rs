//! Versioned occurrence and declaration choices; cancelling has no side effects.
use crate::model::{Cursor, FilePreview};
use vvv_engine::{DefinitionCandidate, NavigationQuery, Selection, SourceAnchor};

#[derive(Debug, Clone)]
pub struct NavigationPicker {
    pub title: &'static str,
    pub filter: String,
    pub caret: crate::input::Caret,
    pub cursor: Cursor,
    pub items: Vec<NavigationItem>,
}
#[derive(Debug, Clone)]
pub struct NavigationItem {
    pub label: String,
    pub query: NavigationQuery,
}
impl NavigationPicker {
    pub fn identifiers(
        preview: &FilePreview,
        anchors: impl IntoIterator<Item = SourceAnchor>,
        line: usize,
    ) -> Self {
        let mut items = Vec::new();
        let mut cursor = Cursor::default();
        let mut nearest = usize::MAX;
        for anchor in anchors {
            let Some(name) = preview.text.get(anchor.span.start..anchor.span.end) else {
                continue;
            };
            let Some(lines) = preview.lines_in(anchor.span) else {
                continue;
            };
            let row = lines.start;
            let Some((start, end)) = preview.line_span(row) else {
                continue;
            };
            let column = preview.text[start..anchor.span.start].chars().count() + 1;
            if row.abs_diff(line) < nearest {
                nearest = row.abs_diff(line);
                cursor.index = items.len();
            }
            items.push(NavigationItem {
                label: format!(
                    "{name}  {}:{}  {}",
                    row + 1,
                    column,
                    preview.text[start..end].trim()
                ),
                query: NavigationQuery::occurrence(anchor),
            });
        }
        Self {
            title: "Follow identifier",
            filter: String::new(),
            caret: Default::default(),
            cursor,
            items,
        }
    }
    pub fn candidates(query: NavigationQuery, candidates: Vec<DefinitionCandidate>) -> Self {
        let items = candidates
            .into_iter()
            .map(|candidate| {
                let d = candidate.declaration;
                let name = d
                    .symbol
                    .as_ref()
                    .map_or(d.text.as_str(), |s| s.name.as_str());
                NavigationItem {
                    label: format!(
                        "{} {name}  {}:{}:{}",
                        candidate.target.kind.as_str(),
                        d.path,
                        d.start.line + 1,
                        d.start.column + 1
                    ),
                    query: query.clone().select(Selection::ids([d.id])),
                }
            })
            .collect();
        Self {
            title: "Choose definition",
            filter: String::new(),
            caret: Default::default(),
            cursor: Cursor::default(),
            items,
        }
    }
    pub fn visible(&self) -> Vec<&NavigationItem> {
        let filter = self.filter.to_lowercase();
        self.items
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&filter))
            .collect()
    }
    pub fn chosen(&self) -> Option<NavigationQuery> {
        self.visible()
            .get(self.cursor.index)
            .map(|i| i.query.clone())
    }
    pub fn moved(&mut self, by: i32) {
        self.cursor.move_by(by, self.visible().len());
    }
    pub fn input(&mut self, character: Option<char>) {
        if let Some(c) = character {
            self.filter.push(c);
        } else {
            self.filter.pop();
        }
        self.cursor = Cursor::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_columns_are_display_coordinates_but_choices_keep_byte_anchors() {
        let text = "é: Beta";
        let anchor = SourceAnchor {
            path: "a.rs".into(),
            content: vvv_engine::ContentId::of(text),
            span: vvv_engine::Span::new(4, 8),
        };
        let source = FilePreview::new(vvv_engine::File {
            path: "a.rs".into(),
            text: text.into(),
            highlights: vec![],
            symbols: vec![],
            identifiers: vec![anchor.clone()],
        });
        let picker = NavigationPicker::identifiers(&source, [anchor.clone()], 0);
        assert!(picker.items[0].label.starts_with("Beta  1:4"));
        assert_eq!(
            picker.chosen().unwrap(),
            NavigationQuery::occurrence(anchor)
        );
    }
}
