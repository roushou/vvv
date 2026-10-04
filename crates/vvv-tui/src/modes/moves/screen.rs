//! Move: the destination in the title (the plan is its validation), then
//! `→` respellings, `±` structural changes and `!` notices as panels, and a
//! detail panel with the respelling or the hunk.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::NoticeKind;

use super::{MoveMode, MovePanel, MoveRow};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{PanelKind, ReportView};
use crate::render::Pane;
use crate::render::{Fit, Header, Painter, Region, ReviewItem, ReviewList};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::display;
use vvv_engine::protocol::vocabulary::{Mark, Plural};

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
};

static RESPELLINGS_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static STRUCTURAL_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static NOTICES_PANEL: Panel = Panel {
    layer: LIST,
    kind: Some(PanelKind::List),
};

static DETAIL_PANEL: Panel = Panel {
    layer: Layer {
        name: "Move",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
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
};

pub struct MoveView<'a> {
    split: u16,
    view: ReportView,
    mode: &'a MoveMode,
    painter: Painter,
}

impl<'a> MoveView<'a> {
    pub fn new(mode: &'a MoveMode, painter: Painter, split: u16, view: ReportView) -> Self {
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
            &MOVE,
            Self::layout,
            [
                Self::draw_to,
                Self::draw_respellings,
                Self::draw_structural,
                Self::draw_notices,
                Self::draw_detail,
            ],
        )
    }
    fn layout(&self, area: Region) -> Vec<Region> {
        let (top, body) = self.header().areas(area);
        let (left, right) = body.columns(self.split);
        let sizes = [
            self.mode.len(MovePanel::Respellings),
            self.mode.len(MovePanel::Structural),
            self.mode.len(MovePanel::Notices),
        ];
        let active = match self.mode.list() {
            MovePanel::Structural => 1,
            MovePanel::Notices => 2,
            _ => 0,
        };
        let mut regions = vec![top];
        regions.extend(left.review_rows(&sizes, active));
        regions.push(right);
        regions
    }
    fn draw_to(&self, area: Rect, buf: &mut Buffer) {
        self.header().render(area, buf);
    }
    fn draw_respellings(&self, area: Rect, buf: &mut Buffer) {
        self.list_panel(MovePanel::Respellings, area, buf);
    }
    fn draw_structural(&self, area: Rect, buf: &mut Buffer) {
        self.list_panel(MovePanel::Structural, area, buf);
    }
    fn draw_notices(&self, area: Rect, buf: &mut Buffer) {
        self.list_panel(MovePanel::Notices, area, buf);
    }
    fn draw_detail(&self, area: Rect, buf: &mut Buffer) {
        self.detail(area, buf);
    }
    fn header(&self) -> Header<'a> {
        let (mv, t) = (self.mode, self.painter);
        let focused = mv.focus == MovePanel::To;
        let mut bottom = t.path_line(&format!(" {} ", mv.from));
        if let Some(name) = &mv.symbol {
            bottom
                .spans
                .insert(0, Span::styled(format!(" {name} ·"), t.symbol));
        }
        bottom.spans.push(Span::styled("·", t.dim));
        bottom.spans.extend(t.review_state(mv.state()).spans);
        let mut header = Header::new(
            t,
            focused,
            Line::from(Span::styled(
                if mv.symbol.is_some() {
                    " Move declaration "
                } else {
                    " Move file "
                },
                t.title,
            )),
        )
        .line(Line::from(vec![
            Span::styled(" destination: ", t.dim),
            Span::raw(mv.to.clone()),
            t.caret(focused),
        ]))
        .bottom(bottom);
        if let Some(plan) = &mv.plan {
            header = header.right(Line::from(vec![
                Span::styled(Plural(plan.files.len(), "file").to_string(), t.key),
                Span::styled(
                    format!(
                        " · {} manual fix{}",
                        plan.notices.len(),
                        if plan.notices.len() == 1 { "" } else { "es" }
                    ),
                    if plan.notices.is_empty() {
                        t.dim
                    } else {
                        t.warning
                    },
                ),
            ]));
        }
        header
    }

    fn list_panel(&self, panel: MovePanel, area: Rect, buf: &mut Buffer) {
        let (mv, t) = (self.mode, self.painter);
        let (mark, label) = match panel {
            MovePanel::Respellings => (Mark::Import, "Paths rewritten"),
            MovePanel::Structural => (Mark::Structure, "Structure"),
            MovePanel::Notices => (Mark::ByHand, "Manual fixes"),
            _ => return,
        };
        let mut items = Vec::new();
        if let Some(plan) = &mv.plan {
            match panel {
                MovePanel::Respellings => items.extend(plan.respellings.iter().map(|r| {
                    ReviewItem {
                        path: &r.path,
                        line: Some(r.start.line + 1),
                        mark,
                        text: display::Line::new()
                            .and(display::Role::Removed, r.from.clone())
                            .and(display::Role::Plain, " → ")
                            .and(display::Role::Added, r.to.clone()),
                    }
                })),
                MovePanel::Structural => {
                    items.extend(plan.structural.iter().map(|&i| ReviewItem {
                        path: &plan.files[i].path,
                        line: None,
                        mark,
                        text: display::Line::single(display::Role::Plain, plan.structural_label(i)),
                    }))
                }
                MovePanel::Notices => items.extend(plan.notices.iter().map(|n| {
                    let text = match &n.kind {
                        NoticeKind::UnrewritableImport {
                            import,
                            replacement,
                        } => format!("{import} → {replacement}"),
                        NoticeKind::RedundantImport { import } => {
                            format!("{import} · remove redundant import")
                        }
                        NoticeKind::Unreachable { item, from, needs } => {
                            format!("{item} · needs {needs:?} from {from}")
                        }
                    };
                    ReviewItem {
                        path: &n.path,
                        line: Some(n.start.line + 1),
                        mark,
                        text: display::Line::single(display::Role::Warning, text),
                    }
                })),
                _ => {}
            }
        }
        let count = items.len();
        let cursor = mv.cursor(panel).map_or(0, |c| c.index);
        ReviewList {
            painter: t,
            items,
            numbered: self.view == ReportView::Detailed,
            active: mv.list() == panel,
        }
        .pane(
            area,
            Line::from(vec![t.glyph(mark), Span::styled(label, t.title)]),
            mv.focus == panel,
            cursor,
        )
        .right(Line::from(Span::styled(
            format!("{}/{}", if count == 0 { 0 } else { cursor + 1 }, count),
            t.dim,
        )))
        .empty(if mv.busy { "Updating preview" } else { "None" })
        .render(area, buf);
    }

    fn detail(&self, area: Rect, buf: &mut Buffer) {
        let (mv, t) = (self.mode, self.painter);
        let current = mv.current();
        let label = match &current {
            Some(MoveRow::Notice(_)) => "Manual fix",
            Some(MoveRow::Structural(_)) => "Diff",
            _ if mv.diff => "Diff",
            _ => "Source",
        };
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled(label, t.title)),
            mv.focus == MovePanel::Detail,
        );
        if let Some(row) = &current {
            pane = pane.location(format!("{}:{}", row.path(), row.line() + 1));
        }
        let mut prefix = Vec::new();
        match &current {
            Some(MoveRow::Respelling(r)) => {
                for (value, style) in [(&r.from, t.removed), (&r.to, t.added)] {
                    prefix.extend(
                        Fit(value, area.width.saturating_sub(4) as usize)
                            .wrapped()
                            .into_iter()
                            .map(|line| Line::from(Span::styled(format!(" {line}"), style))),
                    );
                }
            }
            Some(MoveRow::Structural(file)) => {
                if let Some(to) = &file.moved_to {
                    prefix.extend(
                        Fit(
                            &format!("moves to {to}"),
                            area.width.saturating_sub(4) as usize,
                        )
                        .wrapped()
                        .into_iter()
                        .map(|line| t.path_line(&format!(" {line}"))),
                    );
                }
            }
            Some(MoveRow::Notice(n)) => {
                let (what, todo) = match &n.kind {
                    NoticeKind::UnrewritableImport {
                        import,
                        replacement,
                    } => (
                        format!("{import} → {replacement}"),
                        "Split this grouped import by hand.",
                    ),
                    NoticeKind::RedundantImport { import } => (
                        import.clone(),
                        "Remove this redundant import from the group by hand.",
                    ),
                    NoticeKind::Unreachable { item, from, needs } => (
                        format!("{item} used from {from}, needs {needs:?}"),
                        "Review and widen visibility by hand.",
                    ),
                };
                for (text, style) in [(what.as_str(), t.warning), (todo, t.dim)] {
                    prefix.extend(
                        Fit(text, area.width.saturating_sub(4) as usize)
                            .wrapped_words()
                            .into_iter()
                            .map(|line| Line::from(Span::styled(format!(" {line}"), style))),
                    );
                }
            }
            None => {}
        }
        pane = pane.prefix(prefix).footer(if mv.busy {
            t.review_state(mv.state())
        } else {
            Line::from(Span::styled(
                if matches!(current, Some(MoveRow::Notice(_))) {
                    " manual fix · excluded "
                } else {
                    " included in apply "
                },
                t.dim,
            ))
        });
        let height = pane.content_height(area);
        let rows = match &current {
            Some(MoveRow::Respelling(r)) if mv.diff => mv
                .plan
                .as_ref()
                .and_then(|p| p.files.iter().find(|f| f.path == r.path))
                .map(|file| t.diff_window(file, Some(r.start.line + 1), mv.detail_scroll, height))
                .unwrap_or_default(),
            Some(MoveRow::Respelling(r)) => mv
                .preview
                .as_ref()
                .filter(|p| p.path == r.path)
                .map(|preview| {
                    let anchor = (r.start.line as usize).saturating_sub(height / 2);
                    let first = anchor
                        .saturating_add(mv.detail_scroll)
                        .min(preview.line_count().saturating_sub(height.max(1)));
                    t.source_window(
                        preview,
                        first,
                        height,
                        area.width.saturating_sub(2) as usize,
                        Some((r.span.start, r.span.end)),
                        Some((r.start.line as usize, r.start.line as usize)),
                    )
                })
                .unwrap_or_default(),
            Some(MoveRow::Structural(file)) => t.diff_window(file, None, mv.detail_scroll, height),
            _ => Vec::new(),
        };
        pane.rows(rows)
            .empty(if current.is_some() {
                ""
            } else {
                "Choose a change to review"
            })
            .render(area, buf);
    }
}
