//! Rename: the new name in the title, a panel per verdict with a checkbox
//! per site, and a detail panel with the reason and the plan's diff for the
//! row's file, or the source around it when the plan does not touch it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::Confidence;

use super::{RenameMode, RenamePanel};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{PanelKind, ReportView};
use crate::render::Pane;
use crate::render::{Fit, Header, Painter, Region, ReviewItem, ReviewList};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::vocabulary::{Files, Mark, Plural};

use Action as A;
use Dispatch::Run;

/// The keys of the whole mode.
const MODE: Layer<Action> = Layer {
    name: "Rename",
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

/// The new name.
const NAME: Layer<Action> = Layer {
    name: "Rename",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down())],
            dispatch: Run(A::FocusNext),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↓/tab",
                    word: "sites",
                }),
                help: "the sites",
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
        // An identifier cannot hold `?`.
        Keybinding {
            triggers: &[Trigger::Key(Key::char('?'))],
            dispatch: Run(A::Help),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "keys",
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
                    word: "the new name",
                }),
                help: "the new name; rows follow",
            },
        },
    ],
};

/// The verdict panels, ticked per site.
const LIST: Layer<Action> = Layer {
    name: "Rename",
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
                    word: "all here",
                }),
                help: "tick every row of the panel",
            },
        },
    ],
};

static NAME_PANEL: Panel = Panel {
    layer: NAME,
    kind: Some(PanelKind::Input),
};

static UNSURE_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static SURE_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static OTHER_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Rename",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
};

/// The rename screen.
pub(crate) static RENAME: Screen = Screen {
    layer: MODE,
    panels: &[
        NAME_PANEL,
        UNSURE_PANEL,
        SURE_PANEL,
        OTHER_PANEL,
        DETAIL_PANEL,
    ],
};

pub struct RenameView<'a> {
    split: u16,
    view: ReportView,
    mode: &'a RenameMode,
    painter: Painter,
}

impl<'a> RenameView<'a> {
    pub fn new(mode: &'a RenameMode, painter: Painter, split: u16, view: ReportView) -> Self {
        Self {
            split,
            view,
            mode,
            painter,
        }
    }

    pub fn screen(self) -> BoundScreen<Self, 5> {
        BoundScreen::new(
            self,
            &RENAME,
            Self::layout,
            [
                Self::draw_name,
                Self::draw_unsure,
                Self::draw_sure,
                Self::draw_other,
                Self::draw_detail,
            ],
        )
    }
    fn layout(&self, area: Region) -> Vec<Region> {
        let (top, body) = self.header().areas(area);
        let (left, right) = body.columns(self.split);
        let sizes = [
            self.mode.rows(Confidence::Unresolved).len(),
            self.mode.rows(Confidence::Resolved).len(),
            self.mode.rows(Confidence::Other).len(),
        ];
        let active = match self.mode.list() {
            RenamePanel::Sure => 1,
            RenamePanel::Other => 2,
            _ => 0,
        };
        let mut regions = vec![top];
        regions.extend(left.review_rows(&sizes, active));
        regions.push(right);
        regions
    }
    fn draw_name(&self, area: Rect, buf: &mut Buffer) {
        self.header().render(area, buf);
    }
    fn draw_unsure(&self, area: Rect, buf: &mut Buffer) {
        self.verdict_panel(RenamePanel::Unsure, area, buf);
    }
    fn draw_sure(&self, area: Rect, buf: &mut Buffer) {
        self.verdict_panel(RenamePanel::Sure, area, buf);
    }
    fn draw_other(&self, area: Rect, buf: &mut Buffer) {
        self.verdict_panel(RenamePanel::Other, area, buf);
    }
    fn draw_detail(&self, area: Rect, buf: &mut Buffer) {
        self.detail(area, buf);
    }

    fn header(&self) -> Header<'a> {
        let (r, t) = (self.mode, self.painter);
        let focused = r.focus == RenamePanel::Name;
        let mut bottom = r.declarations.first().map_or_else(
            || Line::from(Span::styled(" by name ", t.dim)),
            |d| t.path_line(&format!(" {}:{} ", d.path, d.start.line + 1)),
        );
        if r.declarations.len() > 1 {
            bottom.spans.push(Span::styled(
                format!("· {} · ", Plural(r.declarations.len(), "declaration")),
                t.warning,
            ));
        } else {
            bottom.spans.push(Span::styled("·", t.dim));
        }
        bottom.spans.extend(t.review_state(r.state()).spans);
        Header::new(
            t,
            focused,
            Line::from(Span::styled(
                format!(
                    " Rename{} ",
                    r.target
                        .symbol
                        .map_or(String::new(), |kind| format!(" {kind}"))
                ),
                t.title,
            )),
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{} selected · {}",
                r.ticks.len(),
                r.occurrences.len(),
                Plural(r.files(), "file")
            ),
            t.key,
        )))
        .line(Line::from(vec![
            Span::styled(format!(" {} → ", r.target.name), t.symbol),
            if r.name.is_empty() {
                Span::styled("new name", t.dim)
            } else {
                Span::raw(r.name.clone())
            },
            t.caret(focused),
        ]))
        .bottom(bottom)
    }

    fn verdict_panel(&self, panel: RenamePanel, area: Rect, buf: &mut Buffer) {
        let (r, t) = (self.mode, self.painter);
        let confidence = panel.confidence().expect("a verdict panel");
        let rows = r.rows(confidence);
        let label = match panel {
            RenamePanel::Unsure => "Unverified",
            RenamePanel::Sure => "Safe",
            _ => "Other",
        };
        let title = Line::from(vec![
            t.glyph(Mark::from(confidence)),
            Span::styled(label, t.title),
        ]);
        let cursor = r.cursor(panel).map_or(0, |c| c.index);
        let count = rows.len();
        let files = Files::among(rows.iter().map(|o| o.m.path.as_path()));
        ReviewList {
            painter: t,
            items: rows
                .iter()
                .map(|o| ReviewItem::matched(&o.m, r.is_ticked(o)))
                .collect(),
            numbered: self.view == ReportView::Detailed,
            active: r.list() == panel,
        }
        .pane(area, title, r.focus == panel, cursor)
        .right(Line::from(Span::styled(
            format!(
                "{}/{} · {files}",
                if count == 0 { 0 } else { cursor + 1 },
                count
            ),
            t.dim,
        )))
        .footer(Line::from(Span::styled(
            format!(" {}/{} selected ", r.ticked(confidence), count),
            t.tick,
        )))
        .empty(if r.busy {
            "Updating preview"
        } else {
            "No occurrences"
        })
        .render(area, buf);
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (r, t) = (self.mode, self.painter);
        let current = r.current();
        let diff = current
            .and_then(|o| r.file(o))
            .filter(|file| !file.diff.hunks().is_empty());
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled(
                if diff.is_some() { "Diff" } else { "Source" },
                t.title,
            )),
            r.focus == RenamePanel::Detail,
        );
        if let Some(o) = current {
            pane = pane.location(format!("{}:{}", o.m.path, o.m.start.line + 1));
            let mark = Mark::from(o.reason);
            let explanation = format!("{} {} · {}", mark.glyph(), mark.word(), mark.meaning());
            pane = pane.prefix(
                Fit(&explanation, area.width.saturating_sub(3) as usize)
                    .wrapped_words()
                    .into_iter()
                    .map(|line| Line::from(Span::styled(format!(" {line}"), t.dim)))
                    .collect(),
            );
            pane = pane.footer(if r.busy {
                t.review_state(r.state())
            } else {
                Line::from(Span::styled(
                    if r.is_ticked(o) {
                        " selected "
                    } else {
                        " excluded from apply "
                    },
                    if r.is_ticked(o) { t.tick } else { t.dim },
                ))
            });
        }
        let height = pane.content_height(area);
        let rows = if let (Some(file), Some(o)) = (diff, current) {
            t.diff_window(file, Some(o.m.start.line + 1), r.detail_scroll, height)
        } else if let (Some(preview), Some(o)) = (&r.preview, current) {
            if preview.path == o.m.path {
                let anchor = (o.m.start.line as usize).saturating_sub(height / 2);
                let first = anchor
                    .saturating_add(r.detail_scroll)
                    .min(preview.line_count().saturating_sub(height.max(1)));
                t.source_window(
                    preview,
                    first,
                    height,
                    area.width.saturating_sub(2) as usize,
                    Some((o.m.span.start, o.m.span.end)),
                    Some((o.m.start.line as usize, o.m.end.line as usize)),
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        pane.rows(rows)
            .empty(if current.is_some() {
                "Loading source"
            } else {
                "Choose an occurrence to review"
            })
            .render(area, buf);
    }
}
