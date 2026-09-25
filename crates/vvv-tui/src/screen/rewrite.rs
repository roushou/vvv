//! Rewrite: the search's pattern and the template in the title, one ticked
//! row per match, and a detail panel showing the match's file diff, scrolled
//! to the hunk holding it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::{CaptureValue, Match};

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{Mode, Model, PanelKind, RewriteMode, RewritePanel};
use crate::render::Pane;
use crate::render::{Header, Painter, Region};
use vvv_engine::protocol::vocabulary::{Mark, Plural};

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
    content: draw_template,
};

static MATCHES_PANEL: Panel = Panel {
    layer: MATCHES,
    kind: Some(PanelKind::List),
    content: draw_matches,
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Rewrite",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
    content: draw_detail,
};

/// The rewrite screen.
pub(crate) static REWRITE: Screen = Screen {
    layer: MODE,
    panels: &[TEMPLATE_PANEL, MATCHES_PANEL, DETAIL_PANEL],
    layout,
};

fn layout(model: &Model, painter: Painter, area: Region) -> Vec<Region> {
    let Mode::Rewrite(rw) = &model.mode else {
        return Vec::new();
    };
    let (top, body) = RewriteView::new(model, rw, painter).header().areas(area);
    let (left, right) = body.columns(model.split);
    vec![top, left, right]
}

fn draw_template(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rewrite(rw) = &model.mode {
        RewriteView::new(model, rw, painter)
            .header()
            .render(area, buf);
    }
}

fn draw_matches(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rewrite(rw) = &model.mode {
        RewriteView::new(model, rw, painter).matches(area, buf);
    }
}

fn draw_detail(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rewrite(rw) = &model.mode {
        RewriteView::new(model, rw, painter).detail(area, buf);
    }
}

pub struct RewriteView<'a> {
    model: &'a Model,
    mode: &'a RewriteMode,
    painter: Painter,
}

impl<'a> RewriteView<'a> {
    pub fn new(model: &'a Model, mode: &'a RewriteMode, painter: Painter) -> Self {
        Self {
            model,
            mode,
            painter,
        }
    }

    fn header(&self) -> Header<'a> {
        let (rw, t) = (self.mode, self.painter);
        let focused = rw.focus == RewritePanel::Template;
        let ticked = rw.ticks.len();
        let right = vec![
            t.glyph(Mark::Rewrite),
            Span::raw(format!("{}  ", rw.matches.len())),
            Span::styled(
                format!("{} {ticked}  ", Mark::Ticked),
                if ticked > 0 { t.tick } else { t.dim },
            ),
            Span::styled(Plural(rw.files(), "file").to_string(), t.dim),
        ];
        let pattern = rw
            .query
            .pattern_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "(declarations)".to_owned());
        let mut template = vec![
            Span::styled("template  ", t.dim),
            Span::raw(rw.template.clone()),
            t.caret(focused),
        ];
        if let Some(error) = &rw.error {
            template.push(Span::styled(format!("   ✗ {error}"), t.error));
        } else if rw.busy {
            template.push(Span::styled("   …", t.dim));
        }
        Header::new(
            t,
            focused,
            Line::from(vec![Span::styled(" rewrite ", t.title)]),
        )
        .right(Line::from(right))
        .line(Line::from(vec![
            Span::styled("pattern   ", t.dim),
            Span::raw(pattern),
        ]))
        .line(Line::from(template))
    }

    /// The row the picker's view lays out: the compact view shows the template
    /// preview, the detailed one the terminal's numbered match.
    fn row(&self, m: &Match, ordinal: usize, width: usize) -> Line<'static> {
        let row = self
            .model
            .view
            .view()
            .rewrite(m, ordinal, self.mode.is_ticked(m), width);
        self.painter.line(&row.line)
    }

    fn matches(&self, area: Rect, buf: &mut Buffer) {
        let (rw, t) = (self.mode, self.painter);
        let width = area.width.saturating_sub(2) as usize;
        let rows: Vec<Line> = rw
            .matches
            .iter()
            .enumerate()
            .map(|(i, m)| self.row(m, i + 1, width))
            .collect();
        let title = Line::from(vec![
            t.glyph(Mark::Rewrite),
            Span::styled(
                format!("{} {}", Mark::Rewrite.word(), rw.matches.len()),
                t.title,
            ),
        ]);
        Pane::new(t, title, rw.focus == RewritePanel::Matches)
            .rows(rows)
            .cursor((!rw.matches.is_empty()).then_some(rw.cursor.index))
            .emphasized(true)
            .render(area, buf);
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (rw, t) = (self.mode, self.painter);
        let focused = rw.focus == RewritePanel::Detail;
        let current = rw.current();
        let title = current.map_or_else(
            || Line::from(Span::styled("detail", t.dim)),
            |m| {
                Line::from(Span::styled(
                    format!("{}:{}", m.path.display(), m.start.line + 1),
                    t.path,
                ))
            },
        );
        let mut rows: Vec<Line> = Vec::new();
        if let Some(m) = current
            && !m.captures.is_empty()
        {
            for (name, value) in &m.captures {
                let text = match value {
                    CaptureValue::Single(c) => c.text.clone(),
                    CaptureValue::Multiple(cs) => cs
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                };
                rows.push(Line::from(vec![
                    Span::styled(format!("${name} "), t.symbol),
                    Span::styled("= ", t.dim),
                    Span::raw(text),
                ]));
            }
            rows.push(Line::default());
        }
        if let Some(m) = current
            && let Some(file) = rw.changes.iter().find(|f| f.path == m.path)
        {
            rows.extend(
                file.diff
                    .lines_from(m.start.line + 1)
                    .skip(rw.detail_scroll)
                    .map(|line| t.line(&line)),
            );
        }
        Pane::new(t, title, focused)
            .rows(rows)
            .empty("")
            .render(area, buf);
    }
}
