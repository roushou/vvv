//! History: the ledger of applies, `↩` on the one undo reverses, and the
//! files the cursor's entry touched.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{HistoryMode, HistoryPanel, Mode, Model, PanelKind};
use crate::render::Pane;
use crate::render::{Fit, Header, Painter, Region};
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
    content: draw_header,
};

static ENTRIES_PANEL: Panel = Panel {
    layer: Layer {
        name: "History",
        bindings: &[],
    },
    kind: Some(PanelKind::List),
    content: draw_entries,
};

static FILES_PANEL: Panel = Panel {
    layer: Layer {
        name: "History",
        bindings: &[],
    },
    kind: Some(PanelKind::Text),
    content: draw_files,
};

/// The history screen.
pub(crate) static HISTORY: Screen = Screen {
    layer: MODE,
    panels: &[HEADER_PANEL, ENTRIES_PANEL, FILES_PANEL],
    layout,
};

fn layout(model: &Model, painter: Painter, area: Region) -> Vec<Region> {
    let Mode::History(h) = &model.mode else {
        return Vec::new();
    };
    let (top, body) = header(h, painter).areas(area);
    let (left, right) = body.columns(model.split);
    vec![top, left, right]
}

fn draw_header(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::History(h) = &model.mode {
        header(h, painter).render(area, buf);
    }
}

fn draw_entries(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::History(h) = &model.mode {
        let width = area.width.saturating_sub(2) as usize;
        entries(h, painter, width).render(area, buf);
    }
}

fn draw_files(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Mode::History(h) = &model.mode {
        files(h, painter).render(area, buf);
    }
}

fn header<'a>(h: &'a HistoryMode, t: Painter) -> Header<'a> {
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

fn entries<'a>(h: &'a HistoryMode, t: Painter, width: usize) -> Pane<'a> {
    let newest = h.entries.last().map(|e| e.id);
    let now = Ago::now();
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

fn files<'a>(h: &'a HistoryMode, t: Painter) -> Pane<'a> {
    let mut out: Vec<Line> = Vec::new();
    if let Some(entry) = h.current() {
        for (from, to) in &entry.moves {
            out.push(Line::from(vec![
                Span::styled(from.display().to_string(), t.dim),
                Span::styled(" → ", t.import),
                Span::styled(to.display().to_string(), t.path),
            ]));
        }
        for path in &entry.paths {
            out.push(Line::from(vec![
                t.glyph(Mark::Structure),
                Span::styled(path.display().to_string(), t.path),
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
