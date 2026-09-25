//! Move: the destination in the title (the plan is its validation), then
//! `→` respellings, `±` structural changes and `!` notices as panels, and a
//! detail panel with the respelling or the hunk.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::NoticeKind;
use vvv_engine::protocol::FileChange;

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{Mode, Model, MoveMode, MovePanel, MoveRow, PanelKind};
use crate::render::Pane;
use crate::render::{Fit, Header, Painter, Region};
use vvv_engine::protocol::vocabulary::Mark;

use Action as A;
use Dispatch::Run;

/// The keys of the whole mode.
const MODE: Layer<Action> = Layer {
    name: "Move",
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

/// The destination.
const DESTINATION: Layer<Action> = Layer {
    name: "Move",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down())],
            dispatch: Run(A::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↓/tab",
                    word: "rows",
                }),
                help: "the rows",
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
                    word: "the destination",
                }),
                help: "the destination; the plan is its validation",
            },
        },
    ],
};

/// The respelling, structural and notice panels.
const LIST: Layer<Action> = Layer {
    name: "Move",
    bindings: &[Keybinding {
        triggers: &[Trigger::Key(Key::char('d'))],
        dispatch: Run(A::Diff),
        when: When::Always,
        legend: Legend {
            bar: Some(Bar {
                keys: "d",
                word: "",
            }),
            help: "hunks instead of source in the detail",
        },
    }],
};

static TO_PANEL: Panel = Panel {
    layer: DESTINATION,
    kind: Some(PanelKind::Input),
    content: draw_to,
};

static RESPELLINGS_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_respellings,
};

static STRUCTURAL_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_structural,
};

static NOTICES_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
    content: draw_notices,
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Move",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
    content: draw_detail,
};

/// The move screen.
pub(crate) static MOVE: Screen = Screen {
    layer: MODE,
    panels: &[
        TO_PANEL,
        RESPELLINGS_PANEL,
        STRUCTURAL_PANEL,
        NOTICES_PANEL,
        DETAIL_PANEL,
    ],
    layout,
};

fn layout(model: &Model, painter: Painter, area: Region) -> Vec<Region> {
    let Mode::Move(mv) = &model.mode else {
        return Vec::new();
    };
    let (top, body) = MoveView::new(model, mv, painter).header().areas(area);
    let (left, right) = body.columns(model.split);
    let sizes = [
        (mv.len(MovePanel::Respellings), 3u16),
        (mv.len(MovePanel::Structural), 2),
        (mv.len(MovePanel::Notices), 1),
    ];
    let mut regions = vec![top];
    regions.extend(left.rows(&sizes));
    regions.push(right);
    regions
}

fn draw_to(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Move(mv) = &model.mode {
        MoveView::new(model, mv, painter).header().render(area, buf);
    }
}

fn draw_respellings(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Move(mv) = &model.mode {
        MoveView::new(model, mv, painter).list_panel(MovePanel::Respellings, area, buf);
    }
}

fn draw_structural(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Move(mv) = &model.mode {
        MoveView::new(model, mv, painter).list_panel(MovePanel::Structural, area, buf);
    }
}

fn draw_notices(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Move(mv) = &model.mode {
        MoveView::new(model, mv, painter).list_panel(MovePanel::Notices, area, buf);
    }
}

fn draw_detail(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::Move(mv) = &model.mode {
        MoveView::new(model, mv, painter).detail(area, buf);
    }
}

pub struct MoveView<'a> {
    model: &'a Model,
    mode: &'a MoveMode,
    painter: Painter,
}

impl<'a> MoveView<'a> {
    pub fn new(model: &'a Model, mode: &'a MoveMode, painter: Painter) -> Self {
        Self {
            model,
            mode,
            painter,
        }
    }

    fn header(&self) -> Header<'a> {
        let (mv, t) = (self.mode, self.painter);
        let focused = mv.focus == MovePanel::To;
        let what = match &mv.symbol {
            Some(name) => format!("{name}  {}", mv.from.short()),
            None => mv.from.short(),
        };
        let mut header = Header::new(
            t,
            focused,
            Line::from(vec![
                Span::styled(" move ", t.title),
                Span::styled(what, t.path),
                Span::styled(" → ", t.import),
                Span::raw(mv.to.clone()),
                t.caret(focused),
                Span::raw(" "),
            ]),
        );
        let mut line = Vec::new();
        match (&mv.plan, &mv.error) {
            (Some(_), _) => {}
            (None, Some(error)) => line.push(Span::styled(format!("✗ {error}"), t.error)),
            (None, None) if mv.busy => line.push(Span::styled("…", t.dim)),
            (None, None) => line.push(Span::styled("type a destination", t.dim)),
        }
        if !line.is_empty() {
            header = header.line(Line::from(line));
        }
        header
    }

    fn respelling_rows(&self, width: usize) -> Vec<Line<'static>> {
        let Some(plan) = &self.mode.plan else {
            return Vec::new();
        };
        let view = self.model.view.view();
        plan.respellings
            .iter()
            .map(|r| self.painter.line(&view.respelling(r, width).line))
            .collect()
    }

    fn structural_rows(&self, width: usize) -> Vec<Line<'static>> {
        let t = self.painter;
        let Some(plan) = &self.mode.plan else {
            return Vec::new();
        };
        plan.structural
            .iter()
            .map(|&i| {
                Line::from(vec![
                    t.glyph(Mark::Structure),
                    Span::raw(Fit(&plan.structural_label(i), width.saturating_sub(2)).to_string()),
                ])
            })
            .collect()
    }

    fn notice_rows(&self, width: usize) -> Vec<Line<'static>> {
        let Some(plan) = &self.mode.plan else {
            return Vec::new();
        };
        let view = self.model.view.view();
        plan.notices
            .iter()
            .map(|n| self.painter.line(&view.notice(n, width).line))
            .collect()
    }

    fn list_panel(&self, panel: MovePanel, area: Rect, buf: &mut Buffer) {
        let (mv, t) = (self.mode, self.painter);
        let width = area.width.saturating_sub(2) as usize;
        let (mark, word, rows) = match panel {
            MovePanel::Respellings => {
                (Mark::Import, "paths rewritten", self.respelling_rows(width))
            }
            MovePanel::Structural => (
                Mark::Structure,
                Mark::Structure.word(),
                self.structural_rows(width),
            ),
            MovePanel::Notices => (Mark::ByHand, Mark::ByHand.word(), self.notice_rows(width)),
            MovePanel::To | MovePanel::Detail => return,
        };
        let n = rows.len();
        let title = Line::from(vec![
            t.glyph(mark),
            Span::styled(format!("{word} {n}"), t.title),
        ]);
        let cursor = mv.cursor(panel).map(|c| c.index);
        Pane::new(t, title, mv.focus == panel)
            .rows(rows)
            .cursor((n > 0).then_some(cursor.unwrap_or(0)))
            .emphasized(mv.list() == panel)
            .empty(if mv.busy { "…" } else { "∅" })
            .render(area, buf);
    }

    /// A file's hunks, coloured by line kind.
    fn hunks(&self, file: &FileChange) -> Vec<Line<'static>> {
        file.diff
            .lines()
            .map(|line| self.painter.line(&line))
            .collect()
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (mv, t) = (self.mode, self.painter);
        let focused = mv.focus == MovePanel::Detail;
        let current = mv.current();
        let title = current.as_ref().map_or_else(
            || Line::from(Span::styled("detail", t.dim)),
            |row| Line::from(Span::styled(row.path().short(), t.path)),
        );
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(2) as usize;
        let mut rows: Vec<Line> = Vec::new();
        match &current {
            Some(MoveRow::Respelling(r)) => {
                rows.push(Line::from(vec![
                    Span::styled("  ", t.dim),
                    Span::styled(r.from.clone(), t.dim),
                ]));
                rows.push(Line::from(vec![
                    t.glyph(Mark::Import),
                    Span::styled(r.to.clone(), t.added),
                ]));
                rows.push(Line::default());
                let head = rows.len();
                if mv.diff {
                    if let Some(file) = mv
                        .plan
                        .as_ref()
                        .and_then(|p| p.files.iter().find(|f| f.path == r.path))
                    {
                        rows.extend(self.hunks(file).into_iter().skip(mv.detail_scroll));
                    }
                } else if let Some(preview) = &mv.preview
                    && preview.path == r.path
                {
                    let height = inner_height.saturating_sub(head);
                    let anchor = (r.start.line as usize).saturating_sub(height / 2);
                    let first =
                        (anchor + mv.detail_scroll).min(preview.line_count().saturating_sub(1));
                    rows.extend(t.source_window(
                        preview,
                        first,
                        height,
                        inner_width,
                        Some((r.span.start, r.span.end)),
                        Some((r.start.line as usize, r.start.line as usize)),
                    ));
                }
            }
            Some(MoveRow::Structural(file)) => {
                if let Some(to) = &file.moved_to {
                    rows.push(Line::from(vec![
                        t.glyph(Mark::Structure),
                        Span::styled(file.path.short(), t.dim),
                        Span::styled(" → ", t.import),
                        Span::styled(to.short(), t.path),
                    ]));
                    rows.push(Line::default());
                }
                rows.extend(self.hunks(file).into_iter().skip(mv.detail_scroll));
            }
            Some(MoveRow::Notice(n)) => {
                let (what, todo) = match &n.kind {
                    NoticeKind::UnrewritableImport {
                        import,
                        replacement,
                    } => (
                        format!("{import} → {replacement}"),
                        "inside a grouped import vvv cannot split here; write it by hand",
                    ),
                    NoticeKind::RedundantImport { import } => (
                        import.clone(),
                        "now names a declaration in this file; remove it from the group by hand",
                    ),
                    NoticeKind::Unreachable { item, from, needs } => (
                        format!("{item}  used from {from}, needs {needs:?}"),
                        "widening past what vvv writes on its own; your call",
                    ),
                };
                rows.push(Line::from(vec![t.glyph(Mark::ByHand), Span::raw(what)]));
                rows.push(Line::from(Span::styled(todo, t.dim)));
            }
            None => {}
        }
        Pane::new(t, title, focused)
            .rows(rows)
            .empty("")
            .render(area, buf);
    }
}
