//! The selected declaration's source and independent scroll position.

use std::ops::Range;

use vvv_engine::{
    Failure, Match, NavigationOutcome, NavigationQuery, NavigationReply, Symbol, SymbolKind,
    SymbolRef,
};

use crate::model::FilePreview;

#[derive(Debug, Clone, Default)]
pub struct Body {
    pub preview: Option<FilePreview>,
    pub scroll: usize,
    pub inspection: super::inspection::Inspection,
    /// The declaration paired with the displayed source. A pending selection
    /// must not replace this metadata before its file arrives.
    shown: Option<Match>,
    container: Option<SymbolRef>,
    ticket: u64,
    selected: Option<(u64, NavigationQuery)>,
    pending: bool,
    pub message: Option<String>,
    pub viewport: Option<usize>,
    pub target: Option<SymbolRef>,
}

impl Body {
    pub fn declaration(&self) -> Option<&Match> {
        self.shown.as_ref()
    }

    pub fn clear(&mut self) {
        let ticket = self.ticket.wrapping_add(1);
        *self = Self {
            ticket,
            viewport: self.viewport,
            ..Self::default()
        };
    }

    pub fn select(&mut self, occurrence: Option<&Match>, revision: u64) {
        let Some(occurrence) = occurrence else {
            self.clear();
            self.message = Some("Select a source row".into());
            return;
        };
        let query = match (&occurrence.symbol, &occurrence.content) {
            (Some(symbol), Some(content)) => NavigationQuery {
                origin: vvv_engine::NavigationOrigin::Symbol {
                    symbol: SymbolRef {
                        language: occurrence.language.clone(),
                        declaration: vvv_engine::SourceAnchor {
                            path: occurrence.path.clone(),
                            content: content.clone(),
                            span: symbol.extent,
                        },
                        name_span: symbol.name_span,
                        kind: symbol.kind,
                    },
                },
                selection: vvv_engine::Selection::All,
            },
            _ => occurrence
                .anchor()
                .map(NavigationQuery::occurrence)
                .unwrap_or_else(|| NavigationQuery::at(occurrence.path.clone(), occurrence.start)),
        };
        let selected = (revision, query);
        if self.selected.as_ref() == Some(&selected) {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.selected = Some(selected);
        self.pending = true;
    }

    pub fn query(&self) -> Option<NavigationQuery> {
        self.selected.as_ref().map(|(_, q)| q.clone())
    }

    /// Re-key restored state so replies issued on a previous page cannot match.
    pub fn next_ticket(&self) -> u64 {
        self.ticket.wrapping_add(1)
    }

    pub fn reticket(&mut self, ticket: u64) {
        self.ticket = ticket;
    }

    pub fn install(&mut self, query: NavigationQuery, reply: NavigationReply) {
        self.ticket = self.ticket.wrapping_add(1);
        self.selected = Some((0, query.clone()));
        self.pending = true;
        self.resolved(self.ticket, &query, Ok(reply));
    }

    pub fn pending(&self) -> Option<(u64, NavigationQuery)> {
        self.pending.then(|| {
            (
                self.ticket,
                self.selected.as_ref().expect("pending selection").1.clone(),
            )
        })
    }

    pub fn resolved(
        &mut self,
        ticket: u64,
        query: &NavigationQuery,
        reply: Result<NavigationReply, Failure>,
    ) {
        if !self.pending
            || self.ticket != ticket
            || self.selected.as_ref().is_none_or(|(_, q)| q != query)
        {
            return;
        }
        self.pending = false;
        match reply {
            Ok(NavigationReply {
                outcome:
                    NavigationOutcome::Resolved {
                        target, preview, ..
                    },
                ..
            }) => {
                self.target = Some(target);
                let same = self.container.as_ref() == Some(&preview.container);
                let changed_selection = self.shown.as_ref().is_none_or(|d| {
                    d.path != preview.declaration.path || d.span != preview.declaration.span
                });
                self.container = Some(preview.container);
                self.preview = Some(FilePreview::new(preview.source));
                self.shown = Some(preview.declaration);
                self.message = None;
                if !same {
                    self.scroll = 0;
                }
                if let Some(d) = &self.shown
                    && let Some(symbol) = self.symbol(d)
                    && let Some(preview) = &self.preview
                {
                    self.inspection.sync(preview, symbol.span);
                }
                // Reveal a newly selected variant. Ordinary same-target replies
                // leave the user's scroll untouched.
                if changed_selection
                    && let Some(height) = self.viewport.filter(|height| *height > 0)
                    && let Some(d) = &self.shown
                    && d.symbol
                        .as_ref()
                        .is_some_and(|s| s.kind == SymbolKind::Variant)
                    && let (Some(lines), Some(selected)) = (
                        self.lines(d),
                        self.preview
                            .as_ref()
                            .and_then(|p| p.lines_in(preview.selection)),
                    )
                {
                    let row = selected.start.saturating_sub(lines.start);
                    if row < self.scroll || row >= self.scroll.saturating_add(height) {
                        self.scroll = row.saturating_sub(height.saturating_sub(1));
                    }
                }
            }
            other => {
                self.target = None;
                self.preview = None;
                self.shown = None;
                self.container = None;
                self.scroll = 0;
                self.inspection = Default::default();
                self.message = Some(match other {
                    Ok(NavigationReply {
                        outcome: NavigationOutcome::Ambiguous { .. },
                        ..
                    }) => "Several definitions match".into(),
                    Ok(NavigationReply {
                        outcome: NavigationOutcome::Unavailable { reason },
                        ..
                    }) => reason.message().into(),
                    Err(failure) => failure.message,
                    _ => unreachable!(),
                });
            }
        }
    }

    /// A variant is previewed in its enum; all other declarations show themselves.
    pub fn symbol<'a>(&'a self, declaration: &'a Match) -> Option<&'a Symbol> {
        if let Some(container) = &self.container {
            return self
                .preview
                .as_ref()?
                .symbols
                .iter()
                .find(|s| s.name_span == container.name_span && s.kind == container.kind);
        }
        declaration.symbol.as_ref()
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
                identifiers: vec![],
                path: "a.rs".into(),
                text: text.into(),
                highlights: vec![],
                symbols: vec![],
            })),
            scroll: 0,
            shown: None,
            ..Body::default()
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
