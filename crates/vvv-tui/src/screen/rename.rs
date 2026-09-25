//! Rename: the new name in the title, a panel per verdict with a checkbox
//! per site, and a detail panel with the reason and the plan's diff for the
//! row's file, or the source around it when the plan does not touch it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::{Confidence, Occurrence};

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{Mode, Model, PanelKind, RenameMode, RenamePanel};
use crate::render::Pane;
use crate::render::{Header, Painter, Region};
use vvv_engine::protocol::display;
use vvv_engine::protocol::vocabulary::{Files, Mark};

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
            dispatch: Run(A::FocusNth(2)),
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
    content: draw_name,
};

static UNSURE_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_unsure,
};

static SURE_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_sure,
};

static OTHER_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_other,
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Rename",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
    content: draw_detail,
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
    layout,
};

fn layout(model: &Model, painter: Painter, area: Region) -> Vec<Region> {
    let Mode::Rename(r) = &model.mode else {
        return Vec::new();
    };
    let (top, body) = RenameView::new(model, r, painter).header().areas(area);
    let (left, right) = body.columns(model.split);
    let sizes = [
        (r.rows(Confidence::Unresolved).len(), 3),
        (r.rows(Confidence::Resolved).len(), 2),
        (r.rows(Confidence::Other).len(), 1),
    ];
    let mut regions = vec![top];
    regions.extend(left.rows(&sizes));
    regions.push(right);
    regions
}

fn draw_name(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rename(r) = &model.mode {
        RenameView::new(model, r, painter)
            .header()
            .render(area, buf);
    }
}

fn draw_unsure(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rename(r) = &model.mode {
        RenameView::new(model, r, painter).verdict_panel(RenamePanel::Unsure, area, buf);
    }
}

fn draw_sure(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rename(r) = &model.mode {
        RenameView::new(model, r, painter).verdict_panel(RenamePanel::Sure, area, buf);
    }
}

fn draw_other(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rename(r) = &model.mode {
        RenameView::new(model, r, painter).verdict_panel(RenamePanel::Other, area, buf);
    }
}

fn draw_detail(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Rename(r) = &model.mode {
        RenameView::new(model, r, painter).detail(area, buf);
    }
}

pub struct RenameView<'a> {
    model: &'a Model,
    mode: &'a RenameMode,
    painter: Painter,
}

impl<'a> RenameView<'a> {
    pub fn new(model: &'a Model, mode: &'a RenameMode, painter: Painter) -> Self {
        Self {
            model,
            mode,
            painter,
        }
    }

    fn header(&self) -> Header<'a> {
        let (r, t) = (self.mode, self.painter);
        let focused = r.focus == RenamePanel::Name;
        let right = t.line(
            &display::Line::counts(&[
                (
                    Mark::from(Confidence::Resolved),
                    r.rows(Confidence::Resolved).len(),
                ),
                (
                    Mark::from(Confidence::Unresolved),
                    r.rows(Confidence::Unresolved).len(),
                ),
                (
                    Mark::from(Confidence::Other),
                    r.rows(Confidence::Other).len(),
                ),
            ])
            .and(display::Role::Plain, "  "),
        );
        let kind = r.target.symbol.map(|k| format!("{k} ")).unwrap_or_default();
        let mut header = Header::new(
            t,
            focused,
            Line::from(vec![
                Span::styled(" rename ", t.title),
                t.glyph(Mark::Declaration),
                Span::styled(format!("{kind}{}", r.target.name), t.declaration),
                Span::styled(" → ", t.import),
                if r.name.is_empty() {
                    Span::styled("new name", t.dim)
                } else {
                    Span::styled(r.name.clone(), t.title)
                },
                t.caret(focused),
                Span::raw(" "),
            ]),
        )
        .right(right);
        let mut line = Vec::new();
        match r.declarations.first() {
            Some(d) => {
                if let Some(address) = &d.address {
                    line.push(Span::styled(format!("◆ {address}   "), t.address));
                }
                line.push(Span::styled(
                    format!("{}:{}", d.path.display(), d.start.line + 1),
                    t.path,
                ));
                if r.declarations.len() > 1 {
                    line.push(Span::styled(
                        format!("   +{} more declarations", r.declarations.len() - 1),
                        t.warning,
                    ));
                }
            }
            None if r.busy => line.push(Span::styled("…", t.dim)),
            None => line.push(Span::styled("∅ by name", t.dim)),
        }
        if let Some(error) = &r.error {
            line.push(Span::styled(format!("   ✗ {error}"), t.error));
        }
        header = header.line(Line::from(line));
        header
    }

    /// The row the picker's view lays out: the compact view shows
    /// `▪ ↗ short:line  text`, the detailed one the terminal's numbered row.
    fn row(&self, o: &Occurrence, ordinal: usize, width: usize) -> Line<'static> {
        let row = self
            .model
            .view
            .view()
            .occurrence(o, ordinal, self.mode.is_ticked(o), width);
        self.painter.line(&row.line)
    }

    fn verdict_panel(&self, panel: RenamePanel, area: Rect, buf: &mut Buffer) {
        let (r, t) = (self.mode, self.painter);
        let confidence = panel.confidence().expect("a verdict panel");
        let rows = r.rows(confidence);
        let width = area.width.saturating_sub(2) as usize;
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .map(|(i, o)| self.row(o, i + 1, width))
            .collect();
        // The panel is scoped, so its title says what the verdict means.
        let mark = Mark::from(confidence);
        let mut title = vec![
            t.glyph(mark),
            Span::styled(format!("{} {}", mark.word(), rows.len()), t.title),
        ];
        if !rows.is_empty() {
            title.push(Span::styled(
                format!(
                    "  {}",
                    Files::among(rows.iter().map(|o| o.m.path.as_path()))
                ),
                t.dim,
            ));
            let ticked = r.ticked(confidence);
            title.push(Span::styled(
                format!("  {} {ticked}", Mark::Ticked),
                if ticked > 0 { t.tick } else { t.dim },
            ));
        }
        let cursor = r.cursor(panel).map(|c| c.index);
        Pane::new(t, Line::from(title), r.focus == panel)
            .rows(lines)
            .cursor((!rows.is_empty()).then_some(cursor.unwrap_or(0)))
            .emphasized(r.list() == panel)
            .empty(if r.busy { "…" } else { "∅" })
            .render(area, buf);
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (r, t) = (self.mode, self.painter);
        let focused = r.focus == RenamePanel::Detail;
        let current = r.current();
        let title = current.map_or_else(
            || Line::from(Span::styled("detail", t.dim)),
            |o| {
                Line::from(Span::styled(
                    format!("{}:{}", o.m.path.display(), o.m.start.line + 1),
                    t.path,
                ))
            },
        );
        let mut rows: Vec<Line> = Vec::new();
        if let Some(o) = current {
            let mark = Mark::from(o.reason);
            rows.push(Line::from(vec![
                t.glyph(mark),
                Span::styled(mark.word(), t.title),
                Span::styled(format!("   {}", mark.meaning()), t.dim),
            ]));
            rows.push(Line::default());
        }
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(2) as usize;
        // The plan's diff for the row's file, scrolled to its hunk; the
        // source itself when the plan does not edit this site.
        let diff = current.and_then(|o| {
            let file = r.file(o)?;
            (!file.diff.hunks().is_empty()).then_some((file, o.m.start.line + 1))
        });
        if let Some((file, line)) = diff {
            rows.extend(
                file.diff
                    .lines_from(line)
                    .skip(r.detail_scroll)
                    .map(|line| t.line(&line)),
            );
        } else if let (Some(preview), Some(o)) = (&r.preview, current)
            && preview.path == o.m.path
        {
            let height = inner_height.saturating_sub(rows.len());
            let anchor = (o.m.start.line as usize).saturating_sub(height / 2);
            let first = (anchor + r.detail_scroll).min(preview.line_count().saturating_sub(1));
            rows.extend(t.source_window(
                preview,
                first,
                height,
                inner_width,
                Some((o.m.span.start, o.m.span.end)),
                Some((o.m.start.line as usize, o.m.end.line as usize)),
            ));
        }
        Pane::new(t, title, focused)
            .rows(rows)
            .empty("")
            .render(area, buf);
    }
}
