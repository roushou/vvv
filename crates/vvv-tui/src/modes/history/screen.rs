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
use vvv_engine::protocol::vocabulary::Mark;
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
                    keys: "u",
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
        self.files().render(area, buf);
    }
    fn header(&self) -> Header<'a> {
        let (h, t) = (self.mode, self.painter);
        let newest = h.entries.last().map(|e| e.id);
        let mut right = vec![Span::styled(
            format!(
                "{} entr{}  ",
                h.entries.len(),
                if h.entries.len() == 1 { "y" } else { "ies" }
            ),
            t.dim,
        )];
        if let Some(id) = newest {
            right.push(t.glyph(Mark::Undo));
            right.push(Span::raw(format!("#{id}")));
        }
        Header::new(t, false, Line::from(Span::styled(" history ", t.title)))
            .right(Line::from(right))
            .line(Line::from(Span::styled(
                "oldest first; only the newest can be undone",
                t.dim,
            )))
    }

    fn entries(&self, width: usize) -> Pane<'a> {
        let (h, t) = (self.mode, self.painter);
        let newest = h.entries.last().map(|e| e.id);
        let now = self.now;
        let rows: Vec<Line> = h
            .entries
            .iter()
            .map(|e| {
                let intent = IntentLine(&e.intent).to_string();
                let tail = format!("  {}", Plural(e.files, "file"));
                let mark = if Some(e.id) == newest { "  ↩" } else { "" };
                let budget = width.saturating_sub(18 + tail.len() + mark.len());
                Line::from(vec![
                    Span::styled(format!("#{:<3} ", e.id), t.title),
                    Span::styled(format!("{:>10}  ", Ago::between(e.at, now)), t.dim),
                    Span::raw(Fit(&intent, budget).to_string()),
                    Span::styled(tail, t.dim),
                    Span::styled(mark, t.key),
                ])
            })
            .collect();
        Pane::new(
            t,
            Line::from(Span::styled("entries", t.title)),
            h.focus == HistoryPanel::Entries,
        )
        .rows(rows)
        .cursor((!h.entries.is_empty()).then_some(h.cursor.index))
        .emphasized(true)
    }

    fn files(&self) -> Pane<'a> {
        let (h, t) = (self.mode, self.painter);
        let mut out: Vec<Line> = Vec::new();
        if let Some(entry) = h.current() {
            for (from, to) in &entry.moves {
                out.push(Line::from(vec![
                    Span::styled(from.short(), t.dim),
                    Span::styled(" → ", t.import),
                    Span::styled(to.short(), t.path),
                ]));
            }
            for path in &entry.paths {
                out.push(Line::from(vec![
                    t.glyph(Mark::Structure),
                    Span::styled(path.short(), t.path),
                ]));
            }
        }
        Pane::new(
            t,
            Line::from(Span::styled("files", t.title)),
            h.focus == HistoryPanel::Files,
        )
        .rows(out)
        .scroll(h.files_scroll)
    }
}
