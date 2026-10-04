//! Rewrite: the search's pattern and the template in the title, one ticked
//! row per match, and a detail panel showing the match's file diff, scrolled
//! to the hunk holding it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::CaptureValue;

use super::{RewriteMode, RewritePanel};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{PanelKind, ReportView};
use crate::render::Pane;
use crate::render::{Header, Painter, Region, ReviewItem, ReviewList};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::vocabulary::Plural;

use Action as A;
use Dispatch::Run;

/// The keys of the whole mode.
const MODE: Layer<Action> = Layer {
    name: "Rewrite",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "",
                }),
                help: "what the status bar says",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "back",
                }),
                help: "back",
            },
        },
    ],
};

/// The template.
const TEMPLATE: Layer<Action> = Layer {
    name: "Rewrite",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down())],
            dispatch: Run(A::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↓/tab",
                    word: "matches",
                }),
                help: "the matches",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "erase",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Run(A::Clear),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+u",
                    word: "clear",
                }),
                help: "clear the input and its preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "typing",
                    word: "the template",
                }),
                help: "the template; rows follow",
            },
        },
    ],
};

/// The matches, ticked per row.
const MATCHES: Layer<Action> = Layer {
    name: "Rewrite",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char(' '))],
            dispatch: Run(A::Toggle),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "space",
                    word: "toggle",
                }),
                help: "tick the row",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('a'))],
            dispatch: Run(A::ToggleAll),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "a",
                    word: "all",
                }),
                help: "tick every row",
            },
        },
    ],
};

static TEMPLATE_PANEL: Panel = Panel {
    layer: TEMPLATE,
    kind: Some(PanelKind::Input),
};

static MATCHES_PANEL: Panel = Panel {
    layer: MATCHES,
    kind: Some(PanelKind::List),
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Rewrite",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
};

/// The rewrite screen.
pub(crate) static REWRITE: Screen = Screen {
    layer: MODE,
    panels: &[TEMPLATE_PANEL, MATCHES_PANEL, DETAIL_PANEL],
};

pub struct RewriteView<'a> {
    split: u16,
    view: ReportView,
    mode: &'a RewriteMode,
    painter: Painter,
}

impl<'a> RewriteView<'a> {
    pub fn new(mode: &'a RewriteMode, painter: Painter, split: u16, view: ReportView) -> Self {
        Self {
            split,
            view,
            mode,
            painter,
        }
    }

    pub fn screen(self) -> BoundScreen<Self, 3> {
        BoundScreen::new(
            self,
            &REWRITE,
            Self::layout,
            [Self::draw_template, Self::draw_matches, Self::draw_detail],
        )
    }
    fn layout(&self, area: Region) -> Vec<Region> {
        let (top, body) = self.header().areas(area);
        let (left, right) = body.columns(self.split);
        vec![top, left, right]
    }
    fn draw_template(&self, area: Rect, buf: &mut Buffer) {
        self.header().render(area, buf);
    }
    fn draw_matches(&self, area: Rect, buf: &mut Buffer) {
        self.matches(area, buf);
    }
    fn draw_detail(&self, area: Rect, buf: &mut Buffer) {
        self.detail(area, buf);
    }
    fn header(&self) -> Header<'a> {
        let (rw, t) = (self.mode, self.painter);
        let focused = rw.focus == RewritePanel::Template;
        let pattern = rw.query.pattern_str().unwrap_or("(declarations)");
        let mut bottom = Line::from(Span::styled(format!(" pattern: {pattern} ·"), t.dim));
        bottom.spans.extend(t.review_state(rw.state()).spans);
        Header::new(t, focused, Line::from(Span::styled(" Rewrite ", t.title)))
            .right(Line::from(Span::styled(
                format!(
                    "{}/{} selected · {}",
                    rw.ticks.len(),
                    rw.matches.len(),
                    Plural(rw.files(), "file")
                ),
                t.key,
            )))
            .line(Line::from(vec![
                Span::styled(" template: ", t.dim),
                if rw.template.is_empty() {
                    Span::styled("replacement", t.dim)
                } else {
                    Span::raw(rw.template.clone())
                },
                t.caret(focused),
            ]))
            .bottom(bottom)
    }

    fn matches(&self, area: Rect, buf: &mut Buffer) {
        let (rw, t) = (self.mode, self.painter);
        let title = Line::from(Span::styled("Matches", t.title));
        ReviewList {
            painter: t,
            items: rw
                .matches
                .iter()
                .map(|m| ReviewItem::matched(m, rw.is_ticked(m)))
                .collect(),
            numbered: self.view == ReportView::Detailed,
            active: true,
        }
        .pane(
            area,
            title,
            rw.focus == RewritePanel::Matches,
            rw.cursor.index,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{}",
                if rw.matches.is_empty() {
                    0
                } else {
                    rw.cursor.index + 1
                },
                rw.matches.len()
            ),
            t.dim,
        )))
        .footer(Line::from(Span::styled(
            format!(" {}/{} selected ", rw.ticks.len(), rw.matches.len()),
            t.tick,
        )))
        .empty("No matches")
        .render(area, buf);
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (rw, t) = (self.mode, self.painter);
        let current = rw.current();
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled("Diff", t.title)),
            rw.focus == RewritePanel::Detail,
        );
        let mut captures = Vec::new();
        if let Some(m) = current {
            pane = pane.location(format!("{}:{}", m.path, m.start.line + 1));
            for (name, value) in &m.captures {
                let text = match value {
                    CaptureValue::Single(c) => c.text.clone(),
                    CaptureValue::Multiple(cs) => cs
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                };
                captures.push(Line::from(vec![
                    Span::styled(format!(" ${name} "), t.symbol),
                    Span::styled("= ", t.dim),
                    Span::raw(text),
                ]));
            }
            pane = pane.footer(if rw.busy {
                t.review_state(rw.state())
            } else {
                Line::from(Span::styled(
                    if rw.is_ticked(m) {
                        " selected "
                    } else {
                        " excluded from apply "
                    },
                    if rw.is_ticked(m) { t.tick } else { t.dim },
                ))
            });
        }
        pane = pane.prefix(captures);
        let rows = current
            .and_then(|m| {
                rw.changes.iter().find(|f| f.path == m.path).map(|file| {
                    t.diff_window(
                        file,
                        Some(m.start.line + 1),
                        rw.detail_scroll,
                        pane.content_height(area),
                    )
                })
            })
            .unwrap_or_default();
        pane.rows(rows)
            .empty(if rw.busy {
                "Updating preview"
            } else if rw.error.is_some() {
                "Fix the template to preview changes"
            } else {
                "No changes for this match"
            })
            .render(area, buf);
    }
}
