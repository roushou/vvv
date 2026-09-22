//! The small questions in front of a screen: pick a value, answer yes or no,
//! or read the key list. Each is a [`Screen`] with one panel that draws the
//! box; the globals are checked before its keys.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{Confirm, Menu, Model, Overlay};
use crate::render::{Painter, Region};
use vvv_engine::report::{Detailed, Document, Options, View};

use Action as A;
use Dispatch::Run;

/// Picking one value from a list.
const MENU: Layer<Action> = Layer {
    name: "Menus and questions",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter()), Trigger::Key(Key::char(' '))],
            dispatch: Run(A::MenuChoose),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "choose",
                }),
                help: "choose",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc()), Trigger::Key(Key::char('q'))],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "cancel",
                }),
                help: "cancel",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "move",
            },
        },
    ],
};

/// A yes/no question; anything but yes is no.
const CONFIRM: Layer<Action> = Layer {
    name: "Menus and questions",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('y')), Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "y",
                    word: "yes",
                }),
                help: "yes",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('n'))],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "n",
                    word: "no",
                }),
                help: "no",
            },
        },
        Keybinding {
            triggers: &[Trigger::Any],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "n",
                    word: "no",
                }),
                help: "no",
            },
        },
    ],
};

/// The key list itself; anything closes it.
const HELP: Layer<Action> = Layer {
    name: "Menus and questions",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Scroll(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "scroll",
                }),
                help: "scroll",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Scroll(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "scroll",
                }),
                help: "scroll",
            },
        },
        Keybinding {
            triggers: &[Trigger::Any],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "any key",
                    word: "close",
                }),
                help: "close",
            },
        },
    ],
};

/// The report itself: `j`/`k` walk its source rows, `e` opens one, anything
/// else closes it.
const REPORT: Layer<Action> = Layer {
    name: "Menus and questions",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "row",
                }),
                help: "walk the source rows",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "row",
                }),
                help: "walk the source rows",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('e'))],
            dispatch: Run(A::Edit),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "e",
                    word: "editor",
                }),
                help: "open the row's source",
            },
        },
        Keybinding {
            triggers: &[Trigger::Any],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "any key",
                    word: "close",
                }),
                help: "close",
            },
        },
    ],
};

static MENU_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
    content: draw_menu,
};

static CONFIRM_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
    content: draw_confirm,
};

static HELP_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
    content: draw_help,
};

static REPORT_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
    content: draw_report,
};

pub(crate) static MENU_SCREEN: Screen = Screen {
    layer: MENU,
    panels: &[MENU_PANEL],
    layout: full,
};

pub(crate) static CONFIRM_SCREEN: Screen = Screen {
    layer: CONFIRM,
    panels: &[CONFIRM_PANEL],
    layout: full,
};

pub(crate) static HELP_SCREEN: Screen = Screen {
    layer: HELP,
    panels: &[HELP_PANEL],
    layout: full,
};

pub(crate) static REPORT_SCREEN: Screen = Screen {
    layer: REPORT,
    panels: &[REPORT_PANEL],
    layout: full,
};

fn full(_model: &Model, _painter: Painter, area: Region) -> Vec<Region> {
    vec![area]
}

fn draw_menu(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Some(Overlay::Menu(menu)) = &model.overlay {
        MenuBox::new(menu, painter).render(area, buf);
    }
}

fn draw_confirm(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Some(Overlay::Confirm(confirm)) = &model.overlay {
        ConfirmBox::new(confirm, painter).render(area, buf);
    }
}

fn draw_help(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Some(Overlay::Help {
        screen,
        focus,
        scroll,
    }) = &model.overlay
    {
        HelpBox::new(screen, *focus, *scroll, painter).render(area, buf);
    }
}

fn draw_report(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    if let Some(Overlay::Report { report, cursor }) = &model.overlay {
        ReportBox::new(report, *cursor, painter).render(area, buf);
    }
}

/// A list to pick one value from.
pub struct MenuBox<'a> {
    menu: &'a Menu,
    painter: Painter,
}

impl<'a> MenuBox<'a> {
    pub fn new(menu: &'a Menu, painter: Painter) -> Self {
        Self { menu, painter }
    }
}

impl Widget for MenuBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let height = (self.menu.items.len() as u16 + 2).min(area.height);
        let boxed = area.centered(Constraint::Length(30), Constraint::Length(height));
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(self.painter.focused)
            .title(Span::styled(
                format!(" {} ", self.menu.title()),
                self.painter.title,
            ));
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        let lines: Vec<Line> = self
            .menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let line = Line::from(format!(" {} ", item.label));
                if i == self.menu.cursor {
                    line.style(self.painter.cursor)
                } else {
                    line
                }
            })
            .collect();
        Paragraph::new(lines).render(inner, buf);
    }
}

/// A yes/no question.
pub struct ConfirmBox<'a> {
    confirm: &'a Confirm,
    painter: Painter,
}

impl<'a> ConfirmBox<'a> {
    pub fn new(confirm: &'a Confirm, painter: Painter) -> Self {
        Self { confirm, painter }
    }
}

impl Widget for ConfirmBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = (self.confirm.question.len() as u16 + 6)
            .max(30)
            .min(area.width);
        let boxed = area.centered(Constraint::Length(width), Constraint::Length(3));
        Clear.render(boxed, buf);
        let block = Block::bordered().border_style(self.painter.warning);
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        Paragraph::new(Line::from(vec![
            Span::raw(format!("{}  ", self.confirm.question)),
            Span::styled("y", self.painter.key),
            Span::styled("/", self.painter.dim),
            Span::styled("n", self.painter.key),
        ]))
        .render(inner, buf);
    }
}

pub struct HelpBox<'a> {
    screen: &'a Screen,
    focus: usize,
    scroll: usize,
    painter: Painter,
}

impl<'a> HelpBox<'a> {
    pub fn new(screen: &'a Screen, focus: usize, scroll: usize, painter: Painter) -> Self {
        Self {
            screen,
            focus,
            scroll,
            painter,
        }
    }

    /// What the marks mean; the keys come from the view.
    const MARKS: &'static [(&'static str, &'static str)] = &[
        ("● → ↗", "declaration · import · re-export"),
        ("✓ ? ✗", "will · unsure · will not    ▪ ▫ ticked or not"),
        (
            "± ! ◆ ∅",
            "structural edit · by hand · module address · nothing",
        ),
    ];

    /// (key, what) rows; an empty key starts a section. The marks first,
    /// then every key that works where the user was, by where it comes from.
    fn rows(&self) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = vec![(String::new(), "Marks".to_owned())];
        rows.extend(
            Self::MARKS
                .iter()
                .map(|(k, w)| ((*k).to_owned(), (*w).to_owned())),
        );
        for (title, section) in self.screen.sections(self.focus) {
            rows.push((String::new(), title.to_owned()));
            for row in section {
                rows.push((row.spelled.clone(), row.legend.help.to_owned()));
            }
        }
        rows
    }
}

impl Widget for HelpBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = self.rows();
        let height = (rows.len() as u16 + 2).min(area.height);
        let boxed = area.centered(
            Constraint::Length(96.min(area.width)),
            Constraint::Length(height),
        );
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(self.painter.focused)
            .title(Span::styled(" keys ", self.painter.title));
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        // Never scroll past the last page.
        let scroll = self
            .scroll
            .min(rows.len().saturating_sub(inner.height as usize));
        let lines: Vec<Line> = rows
            .into_iter()
            .skip(scroll)
            .map(|(key, what)| {
                if key.is_empty() {
                    Line::from(Span::styled(format!(" {what}"), self.painter.title))
                } else {
                    Line::from(vec![
                        Span::styled(format!("{key:>16}  "), self.painter.key),
                        Span::raw(what),
                    ])
                }
            })
            .collect();
        Paragraph::new(lines).render(inner, buf);
    }
}

/// The result of an apply, laid out as the lines every renderer reads.
pub struct ReportBox<'a> {
    report: &'a Document,
    cursor: usize,
    painter: Painter,
}

impl<'a> ReportBox<'a> {
    pub fn new(report: &'a Document, cursor: usize, painter: Painter) -> Self {
        Self {
            report,
            cursor,
            painter,
        }
    }
}

impl Widget for ReportBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let painter = self.painter;
        let presentation = Detailed.present(self.report, Options::default(), usize::MAX);
        // The rows a cursor can stand on, and the one it is on.
        let selectable: Vec<usize> = presentation
            .body
            .iter()
            .enumerate()
            .filter(|(_, row)| row.source.is_some())
            .map(|(i, _)| i)
            .collect();
        let cursor = selectable.get(self.cursor).copied();
        let mut lines: Vec<Line> = presentation
            .body
            .iter()
            .map(|r| painter.line(&r.line))
            .collect();
        if let Some(row) = cursor {
            lines[row] = std::mem::take(&mut lines[row]).style(painter.cursor);
        }
        if !presentation.notes.is_empty() {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.extend(presentation.notes.iter().map(|r| painter.line(&r.line)));
        }
        let height = (lines.len() as u16 + 2).min(area.height);
        let boxed = area.centered(
            Constraint::Length(96.min(area.width)),
            Constraint::Length(height),
        );
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(painter.focused)
            .title(Span::styled(" report ", painter.title));
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        // Keep the cursor row in view.
        let visible = inner.height as usize;
        let offset = cursor.map_or(0, |c| c.saturating_sub(visible.saturating_sub(1)));
        let shown: Vec<Line> = lines.into_iter().skip(offset).collect();
        Paragraph::new(shown).render(inner, buf);
    }
}
