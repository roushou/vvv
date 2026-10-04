//! History: the ledger of applies, `↩` on the one undo reverses, and the
//! files the cursor's entry touched.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{HistoryMode, HistoryPanel};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::PanelKind;
use crate::render::Pane;
use crate::render::{Fit, Header, Painter, Region};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::vocabulary::{Ago, IntentLine, Plural};

use Action as A;
use Dispatch::Run;

/// The ledger and the files of the cursor's entry.
const MODE: Layer<Action> = Layer {
    name: "History",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('u')), Trigger::Key(Key::enter())],
            dispatch: Run(A::Undo),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎/u",
                    word: "",
                }),
                help: "undo the newest apply",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc()), Trigger::Key(Key::char('q'))],
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

static HEADER_PANEL: Panel = Panel {
    layer: Layer {
        name: "History",
        bindings: &[],
    },
    kind: None,
};

static ENTRIES_PANEL: Panel = Panel {
    layer: Layer {
        name: "History",
        bindings: &[],
    },
    kind: Some(PanelKind::List),
};

static FILES_PANEL: Panel = Panel {
    layer: Layer {
        name: "History",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
};

/// The history screen.
pub(crate) static HISTORY: Screen = Screen {
    layer: MODE,
    panels: &[HEADER_PANEL, ENTRIES_PANEL, FILES_PANEL],
};

pub(crate) struct HistoryView<'a> {
    mode: &'a HistoryMode,
    painter: Painter,
    split: u16,
    now: u64,
}
impl<'a> HistoryView<'a> {
    pub fn new(mode: &'a HistoryMode, painter: Painter, split: u16, now: u64) -> Self {
        Self {
            mode,
            painter,
            split,
            now,
        }
    }
    pub fn screen(self) -> BoundScreen<Self, 3> {
        BoundScreen::new(
            self,
            &HISTORY,
            Self::layout,
            [Self::draw_header, Self::draw_entries, Self::draw_files],
        )
    }
    fn layout(&self, area: Region) -> Vec<Region> {
        let (top, body) = self.header().areas(area);
        let (left, right) = body.columns(self.split);
        vec![top, left, right]
    }
    fn draw_header(&self, area: Rect, buf: &mut Buffer) {
        self.header().render(area, buf);
    }
    fn draw_entries(&self, area: Rect, buf: &mut Buffer) {
        self.entries(area.width.saturating_sub(2) as usize)
            .render(area, buf);
    }
    fn draw_files(&self, area: Rect, buf: &mut Buffer) {
        self.files(area).render(area, buf);
    }
    fn header(&self) -> Header<'a> {
        let (h, t) = (self.mode, self.painter);
        let newest = h.entries.last().map_or_else(
            || "nothing to undo".to_owned(),
            |e| format!("newest #{} · undo available", e.id),
        );
        Header::new(t, false, Line::from(Span::styled(" History ", t.title)))
            .right(Line::from(Span::styled(
                Plural(h.entries.len(), "entry").to_string(),
                t.dim,
            )))
            .bottom(Line::from(Span::styled(
                format!(" oldest first · {newest} "),
                t.dim,
            )))
    }

    fn entries(&self, width: usize) -> Pane<'a> {
        let (h, t) = (self.mode, self.painter);
        let rows: Vec<Line> = h
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let prefix = format!(
                    "{} #{:<3} ",
                    if i == h.cursor.index { ">" } else { " " },
                    e.id
                );
                let tail = format!(" · {}", Plural(e.files, "file"));
                let budget = width.saturating_sub(
                    Line::from(prefix.as_str()).width() + Line::from(tail.as_str()).width(),
                );
                Line::from(vec![
                    Span::styled(prefix, t.selection_marker(h.focus == HistoryPanel::Entries)),
                    Span::raw(Fit(&IntentLine(&e.intent).to_string(), budget).to_string()),
                    Span::styled(tail, t.dim),
                ])
            })
            .collect();
        let selected = h
            .current()
            .map(|e| {
                format!(
                    " {} · {} ",
                    Ago::between(e.at, self.now),
                    if h.is_newest() {
                        "newest · can undo"
                    } else {
                        "older entry · cannot undo"
                    }
                )
            })
            .unwrap_or_default();
        Pane::new(
            t,
            Line::from(Span::styled("Entries", t.title)),
            h.focus == HistoryPanel::Entries,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{}",
                if h.entries.is_empty() {
                    0
                } else {
                    h.cursor.index + 1
                },
                h.entries.len()
            ),
            t.dim,
        )))
        .footer(Line::from(Span::styled(
            selected,
            if h.is_newest() { t.key } else { t.dim },
        )))
        .rows(rows)
        .cursor((!h.entries.is_empty()).then_some(h.cursor.index))
        .empty("No applied operations")
    }

    fn files(&self, area: Rect) -> Pane<'a> {
        let (h, t) = (self.mode, self.painter);
        let mut out = Vec::new();
        let width = area.width.saturating_sub(3) as usize;
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled("Files", t.title)),
            h.focus == HistoryPanel::Files,
        );
        if let Some(entry) = h.current() {
            pane = pane
                .right(Line::from(Span::styled(
                    format!("#{} · {}", entry.id, Plural(entry.files, "file")),
                    t.dim,
                )))
                .prefix(
                    Fit(&IntentLine(&entry.intent).to_string(), width)
                        .wrapped()
                        .into_iter()
                        .map(|row| Line::from(Span::styled(format!(" {row}"), t.title)))
                        .collect(),
                );
            for (from, to) in &entry.moves {
                for text in [from.as_str().to_owned(), format!("→ {to}")] {
                    out.extend(
                        Fit(&text, width)
                            .wrapped()
                            .into_iter()
                            .map(|row| t.path_line(&format!(" {row}"))),
                    );
                }
            }
            for path in &entry.paths {
                if entry
                    .moves
                    .iter()
                    .any(|(from, to)| path == from || path == to)
                {
                    continue;
                }
                out.extend(
                    Fit(path.as_str(), width)
                        .wrapped()
                        .into_iter()
                        .map(|row| t.path_line(&format!(" {row}"))),
                );
            }
        }
        pane.rows(out)
            .scroll(h.files_scroll)
            .empty("No files in this entry")
    }
}
