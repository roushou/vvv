//! The selected declaration's source and independent scroll position.

use std::ops::Range;

use vvv_engine::{Match, Symbol, SymbolKind};

use crate::model::FilePreview;

#[derive(Debug, Default)]
pub struct Body {
    pub preview: Option<FilePreview>,
    pub scroll: usize,
    /// The declaration paired with the displayed source. A pending selection
    /// must not replace this metadata before its file arrives.
    shown: Option<Match>,
}

impl Body {
    pub fn declaration(&self) -> Option<&Match> {
        self.shown.as_ref()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Follow selections available in the loaded file immediately. Otherwise
    /// retain the complete displayed definition until its replacement arrives.
    pub fn select(&mut self, declaration: Option<&Match>) {
        match declaration {
            Some(declaration)
                if self
                    .preview
                    .as_ref()
                    .is_some_and(|p| p.path == declaration.path) =>
            {
                self.shown = Some(declaration.clone());
                self.scroll = 0;
            }
            None => {
                self.shown = None;
                self.scroll = 0;
            }
            Some(_) => {}
        }
    }

    pub fn received(&mut self, preview: FilePreview, declaration: &Match) {
        if preview.path != declaration.path {
            return;
        }
        if self.shown.as_ref() != Some(declaration) {
            self.scroll = 0;
        }
        self.preview = Some(preview);
        self.shown = Some(declaration.clone());
    }

    /// A variant is previewed in its enum; all other declarations show themselves.
    pub fn symbol<'a>(&'a self, declaration: &'a Match) -> Option<&'a Symbol> {
        let symbol = declaration.symbol.as_ref()?;
        if symbol.kind == SymbolKind::Variant
            && let Some(preview) = &self.preview
            && preview.path == declaration.path
            && let Some(parent) = preview.enclosing(symbol.span, SymbolKind::Enum)
        {
            return Some(parent);
        }
        Some(symbol)
    }

    /// Only show a valid declaration range from its own file.
    pub fn lines(&self, declaration: &Match) -> Option<Range<usize>> {
        let preview = self.preview.as_ref()?;
        if preview.path != declaration.path {
            return None;
        }
        preview.lines_in(self.symbol(declaration)?.span)
    }

    pub fn scroll_by(&mut self, by: i32) {
        let max = self
            .shown
            .as_ref()
            .and_then(|d| self.lines(d))
            .map_or(0, |lines| lines.len().saturating_sub(1));
        self.scroll = self.scroll.saturating_add_signed(by as isize).min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_engine::{Span, SymbolKind};

    #[test]
    fn ranges_are_bounded_and_utf8_safe() {
        let text = "// é\nstruct A {}\nstruct B {};";
        let body = Body {
            preview: Some(FilePreview::new(vvv_engine::File {
                path: "a.rs".into(),
                text: text.into(),
                highlights: vec![],
                symbols: vec![],
            })),
            scroll: 0,
            shown: None,
        };
        let mut declaration =
            crate::fixtures::decl("a.rs", 1, SymbolKind::Struct, "A", "struct A {}");
        for (span, expected) in [
            (Span::new(6, 17), Some(1..2)),
            (Span::new(6, 18), Some(1..2)),
            (Span::new(4, 17), None),
            (Span::new(6, 100), None),
            (Span::new(6, 6), None),
        ] {
            declaration.symbol.as_mut().unwrap().span = span;
            assert_eq!(body.lines(&declaration), expected);
        }
    }
}
