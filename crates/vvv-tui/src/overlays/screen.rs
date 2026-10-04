//! The small questions in front of a screen: pick a value, answer yes or no,
//! or read the key list. Each is a [`Screen`] with one panel that draws the
//! box; the globals are checked before its keys.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use super::{Confirm, Menu};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::render::{Fit, Painter};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::vocabulary::Plural;
use vvv_engine::report::{Detailed, Document, Options, View};

use Action as A;
use Dispatch::Run;

/// Picking one value from a list.
const MENU: Layer<Action> = Layer {
    name: "Menus and questions",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::MenuChoose),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "",
                }),
                help: "choose",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
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
            triggers: &[Trigger::Key(Key::ctrl('x'))],
            dispatch: Run(A::MenuClear),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+x",
                    word: "clear",
                }),
                help: "clear this restriction and close the picker",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Run(A::Clear),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+u",
                    word: "text",
                }),
                help: "clear picker text",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::ctrl('n'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::ctrl('p'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "move",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "erase filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "type to filter; look in also accepts a relative path",
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
            triggers: &[Trigger::Key(Key::page_down())],
            dispatch: Run(A::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "next help page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up())],
            dispatch: Run(A::Page(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "previous help page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home())],
            dispatch: Run(A::Top),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "first help page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end())],
            dispatch: Run(A::Bottom),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "last help page",
            },
        },
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
};

static CONFIRM_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
};

static HELP_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
};

static REPORT_PANEL: Panel = Panel {
    layer: Layer {
        name: "Menus and questions",
        bindings: &[],
    },
    kind: None,
};

pub(crate) static MENU_SCREEN: Screen = Screen {
    layer: MENU,
    panels: &[MENU_PANEL],
};

pub(crate) static CONFIRM_SCREEN: Screen = Screen {
    layer: CONFIRM,
    panels: &[CONFIRM_PANEL],
};
pub(crate) static HELP_SCREEN: Screen = Screen {
    layer: HELP,
    panels: &[HELP_PANEL],
};
pub(crate) static REPORT_SCREEN: Screen = Screen {
    layer: REPORT,
    panels: &[REPORT_PANEL],
};

/// A list to pick one value from.
pub struct MenuBox<'a> {
    menu: &'a Menu,
    painter: Painter,
}

impl<'a> MenuBox<'a> {
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(self, &MENU_SCREEN, |_, area| area.full(), [Self::draw])
    }
    pub fn new(menu: &'a Menu, painter: Painter) -> Self {
        Self { menu, painter }
    }
}

impl MenuBox<'_> {
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let items = self.menu.shown();
        let width = items
            .iter()
            .map(|i| Line::from(i.label.as_str()).width() + 14)
            .max()
            .unwrap_or(50)
            .clamp(66, 92)
            .min(area.width as usize) as u16;
        let content_width = width.saturating_sub(2) as usize;
        let label_width = content_width.saturating_sub(12).max(1);
        let mut choices = Vec::new();
        let mut selection = 0..0;
        for (i, item) in items.iter().enumerate() {
            let start = choices.len();
            let wrapped = Fit(&item.label, label_width).wrapped();
            let last = wrapped.len().saturating_sub(1);
            for (row, label) in wrapped.into_iter().enumerate() {
                let applied = self.menu.target != super::MenuTarget::Filters
                    && item.value == self.menu.selected;
                let mut spans = vec![
                    Span::styled(
                        if i == self.menu.cursor && row == 0 {
                            "> "
                        } else {
                            "  "
                        },
                        self.painter.selection_marker(true),
                    ),
                    Span::styled(
                        if applied && row == 0 { "✓ " } else { "  " },
                        self.painter.key,
                    ),
                    Span::raw(label),
                ];
                if row == last
                    && let Some(count) = item.count
                {
                    let count = count.to_string();
                    let used = Line::from(spans.clone()).width();
                    spans.push(Span::raw(
                        " ".repeat(content_width.saturating_sub(used + count.len() + 1)),
                    ));
                    spans.push(Span::styled(format!("{count} "), self.painter.dim));
                }
                let line = Line::from(spans);
                choices.push(if i == self.menu.cursor {
                    self.painter.selected_line(line, true, content_width)
                } else {
                    line
                });
            }
            if i == self.menu.cursor {
                selection = start..choices.len();
            }
        }
        if choices.is_empty() {
            choices.push(Line::from(Span::styled(
                " No choices match",
                self.painter.dim,
            )));
        }
        let height =
            (choices.len().saturating_add(4).min(u16::MAX as usize) as u16).min(area.height);
        let boxed = area.centered(Constraint::Length(width), Constraint::Length(height));
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(self.painter.focused)
            .title(Span::styled(
                format!(" {} ", self.menu.title()),
                self.painter.title,
            ))
            .title_top(
                Line::from(Span::styled(
                    format!(
                        " {}{} ",
                        Plural(items.len(), "choice"),
                        if self.menu.counted {
                            " · loaded hits"
                        } else {
                            ""
                        }
                    ),
                    self.painter.dim,
                ))
                .right_aligned(),
            );
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        let mut rows = vec![
            Line::from(vec![
                Span::styled("> ", self.painter.key),
                Span::raw(Fit(&self.menu.filter, content_width.saturating_sub(4)).to_string()),
                self.painter.caret(true),
            ]),
            Line::default(),
        ];
        let visible = inner.height.saturating_sub(2) as usize;
        let offset = selection.end.saturating_sub(visible);
        rows.extend(choices.into_iter().skip(offset).take(visible));
        Paragraph::new(rows).render(inner, buf);
    }
}

/// A yes/no question.
pub struct ConfirmBox<'a> {
    confirm: &'a Confirm,
    painter: Painter,
}

impl<'a> ConfirmBox<'a> {
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(self, &CONFIRM_SCREEN, |_, area| area.full(), [Self::draw])
    }
    pub fn new(confirm: &'a Confirm, painter: Painter) -> Self {
        Self { confirm, painter }
    }
}

impl ConfirmBox<'_> {
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let width = (self.confirm.question.len() as u16 + 6)
            .max(30)
            .min(area.width);
        let boxed = area.centered(Constraint::Length(width), Constraint::Length(3));
        Clear.render(boxed, buf);
        let block = Block::bordered().border_style(self.painter.warning);
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        Paragraph::new(self.confirm.question.as_str()).render(inner, buf);
    }
}

pub struct HelpBox<'a> {
    title: &'a str,
    sections: &'a [(&'static str, Vec<crate::keymap::Row<'static, Action>>)],
    scroll: usize,
    painter: Painter,
}

impl<'a> HelpBox<'a> {
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(self, &HELP_SCREEN, |_, area| area.full(), [Self::draw])
    }
    pub fn new(
        title: &'a str,
        sections: &'a [(&'static str, Vec<crate::keymap::Row<'static, Action>>)],
        scroll: usize,
        painter: Painter,
    ) -> Self {
        Self {
            title,
            sections,
            scroll,
            painter,
        }
    }

    /// What the marks mean; the keys come from the view.
    const MARKS: &'static [(&'static str, &'static str)] = &[
        ("● → ↗", "declaration · import · re-export"),
        ("✓ ? ✗", "will · unsure · will not    ▪ ▫ ticked or not"),
        ("± ! ∅", "structural edit · by hand · nothing"),
    ];

    /// (key, what) rows; an empty key starts a section. The marks first,
    /// then every key that works where the user was, by where it comes from.
    fn rows(&self) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = Vec::new();
        for (title, section) in self.sections.iter() {
            rows.push((String::new(), (*title).to_owned()));
            for row in section {
                rows.push((row.labels.clone(), row.legend.help.to_owned()));
            }
        }
        rows.push((String::new(), "Marks".into()));
        rows.extend(Self::MARKS.iter().map(|(k, w)| ((*k).into(), (*w).into())));
        rows
    }
}

impl HelpBox<'_> {
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let width = 96.min(width).saturating_sub(2) as usize;
        let gutter = self
            .rows()
            .iter()
            .map(|(k, _)| Line::from(k.as_str()).width())
            .max()
            .unwrap_or(0)
            .min(width / 3)
            .max(1);
        let mut lines = Vec::new();
        for (key, what) in self.rows() {
            if key.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(" {what}"),
                    self.painter.title,
                )));
            } else {
                let keys = Fit(&key, gutter).wrapped_words();
                let words = Fit(&what, width.saturating_sub(gutter + 2).max(1)).wrapped_words();
                for i in 0..keys.len().max(words.len()) {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{:>gutter$}  ", keys.get(i).map_or("", String::as_str)),
                            self.painter.key,
                        ),
                        Span::raw(words.get(i).cloned().unwrap_or_default()),
                    ]));
                }
            }
        }
        lines
    }
    pub fn scroll_limit(&self, width: u16, height: u16) -> usize {
        self.lines(width)
            .len()
            .saturating_sub(height.saturating_sub(2) as usize)
    }
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let lines = self.lines(area.width);
        let boxed = area.centered(
            Constraint::Length(96.min(area.width)),
            Constraint::Length((lines.len() as u16 + 2).min(area.height)),
        );
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(self.painter.focused)
            .title(Span::styled(
                format!(" Help · {} ", self.title),
                self.painter.title,
            ));
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        let scroll = self
            .scroll
            .min(lines.len().saturating_sub(inner.height as usize));
        Paragraph::new(
            lines
                .into_iter()
                .skip(scroll)
                .take(inner.height as usize)
                .collect::<Vec<_>>(),
        )
        .render(inner, buf);
    }
}

pub struct ReportBox<'a> {
    report: &'a Document,
    cursor: usize,
    painter: Painter,
}

impl<'a> ReportBox<'a> {
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(self, &REPORT_SCREEN, |_, area| area.full(), [Self::draw])
    }
    pub fn new(report: &'a Document, cursor: usize, painter: Painter) -> Self {
        Self {
            report,
            cursor,
            painter,
        }
    }
}

impl ReportBox<'_> {
    fn draw(&self, area: Rect, buf: &mut Buffer) {
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
        if let Some(row) = cursor {
            lines[row] =
                painter.selected_line(std::mem::take(&mut lines[row]), true, inner.width as usize);
        }
        // Keep the cursor row in view.
        let visible = inner.height as usize;
        let offset = cursor.map_or(0, |c| c.saturating_sub(visible.saturating_sub(1)));
        let shown: Vec<Line> = lines.into_iter().skip(offset).collect();
        Paragraph::new(shown).render(inner, buf);
    }
}

/// Filtering uses ordinary text; arrow keys walk the surviving occurrences.
const NAVIGATION: Layer<Action> = Layer {
    name: "Navigation choices",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::MenuChoose),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "follow",
                }),
                help: "follow",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
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
            triggers: &[Trigger::Key(Key::down())],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "select a choice",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up())],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "select a choice",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down())],
            dispatch: Run(A::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "page down",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up())],
            dispatch: Run(A::Page(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "page up",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home())],
            dispatch: Run(A::Top),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "first choice",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end())],
            dispatch: Run(A::Bottom),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "last choice",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "erase filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "filter choices",
            },
        },
    ],
};
pub static NAVIGATION_SCREEN: Screen = Screen {
    layer: NAVIGATION,
    panels: &[Panel {
        layer: NAVIGATION,
        kind: None,
    }],
};
pub struct NavigationBox<'a> {
    picker: &'a super::navigation::NavigationPicker,
    painter: Painter,
}
impl<'a> NavigationBox<'a> {
    pub fn new(picker: &'a super::navigation::NavigationPicker, painter: Painter) -> Self {
        Self { picker, painter }
    }
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(
            self,
            &NAVIGATION_SCREEN,
            |_, area| area.full(),
            [Self::draw],
        )
    }
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let boxed = area.centered(
            Constraint::Percentage(90),
            Constraint::Length(18.min(area.height)),
        );
        Clear.render(boxed, buf);
        let block = Block::bordered()
            .border_style(self.painter.focused)
            .title(format!(" {} ", self.picker.title));
        let inner = block.inner(boxed);
        block.render(boxed, buf);
        let visible = self.picker.visible();
        let height = inner.height.saturating_sub(1) as usize;
        let start = self
            .picker
            .cursor
            .index
            .saturating_sub(height.saturating_sub(1));
        let mut lines = vec![Line::from(format!(
            "Filter: {}  ({} {})",
            self.picker.filter,
            visible.len(),
            if visible.len() == 1 {
                "choice"
            } else {
                "choices"
            }
        ))];
        lines.extend(
            visible
                .iter()
                .enumerate()
                .skip(start)
                .take(height)
                .map(|(i, item)| {
                    let line = Line::from(item.label.as_str());
                    if i == self.picker.cursor.index {
                        self.painter.selected_line(line, true, inner.width as usize)
                    } else {
                        line
                    }
                }),
        );
        Paragraph::new(lines).render(inner, buf);
    }
}

const PLACES: Layer<Action> = Layer {
    name: "Places",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "open",
                }),
                help: "restore a browsing page or run a recent search",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "cancel",
                }),
                help: "close without changing your place",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::tab()), Trigger::Key(Key::back_tab())],
            dispatch: Run(A::PlacesTab),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "tab",
                    word: "trail/recent",
                }),
                help: "switch browsing trail / recent searches",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::ctrl('n'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "select a place",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::ctrl('p'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "move",
                }),
                help: "select a place",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::home())],
            dispatch: Run(A::Top),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "first place",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::end())],
            dispatch: Run(A::Bottom),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "last place",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down())],
            dispatch: Run(A::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "next page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up())],
            dispatch: Run(A::Page(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "previous page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Run(A::Clear),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+u",
                    word: "text",
                }),
                help: "clear place filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('d'))],
            dispatch: Run(A::ForgetSearch),
            when: When::PlacesRecent,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+d",
                    word: "forget",
                }),
                help: "forget selected recent search",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('l'))],
            dispatch: Run(A::ResetLayout),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+l",
                    word: "layout",
                }),
                help: "reset split, report view and preview choice",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "edit place filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "filter queries and complete paths",
            },
        },
    ],
};
pub static PLACES_SCREEN: Screen = Screen {
    layer: PLACES,
    panels: &[Panel {
        layer: PLACES,
        kind: None,
    }],
};
pub struct PlacesBox<'a> {
    places: &'a super::places::Places,
    painter: Painter,
}
impl<'a> PlacesBox<'a> {
    pub fn new(places: &'a super::places::Places, painter: Painter) -> Self {
        Self { places, painter }
    }
    pub fn screen(self) -> BoundScreen<Self, 1> {
        BoundScreen::new(self, &PLACES_SCREEN, |_, area| area.full(), [Self::draw])
    }
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let boxed = area.centered(
            Constraint::Percentage(94),
            Constraint::Length(20.min(area.height)),
        );
        Clear.render(boxed, buf);
        let p = self.places;
        let visible = p.visible();
        let width = boxed.width.saturating_sub(4) as usize;
        let mut rows = Vec::new();
        let mut cursor = None;
        for (i, item) in visible.iter().enumerate() {
            if i == p.cursor.index {
                cursor = Some(rows.len());
            }
            rows.extend(
                Fit(
                    &format!(
                        "{}{}",
                        if i == p.cursor.index { "> " } else { "  " },
                        item.label
                    ),
                    width,
                )
                .wrapped()
                .into_iter()
                .map(|row| {
                    let line = Line::from(row);
                    if i == p.cursor.index {
                        self.painter.selected_line(
                            line,
                            true,
                            boxed.width.saturating_sub(2) as usize,
                        )
                    } else {
                        line
                    }
                }),
            );
        }
        let mut title = vec![Span::styled("Places · ", self.painter.title)];
        title.push(Span::styled(
            format!("Trail {}", p.trail.len()),
            if p.recent {
                self.painter.dim
            } else {
                self.painter.key
            },
        ));
        title.push(Span::styled(" / ", self.painter.dim));
        title.push(Span::styled(
            format!("Recent {}", p.searches.len()),
            if p.recent {
                self.painter.key
            } else {
                self.painter.dim
            },
        ));
        crate::render::Pane::new(self.painter, Line::from(title), true)
            .right(Line::from(format!("{} shown", visible.len())))
            .prefix(vec![Line::from(vec![
                Span::styled("Find: ", self.painter.key),
                Span::raw(p.filter.clone()),
                self.painter.caret(true),
            ])])
            .rows(rows)
            .cursor(cursor)
            .empty(if p.recent {
                "No recent searches match."
            } else {
                "No places match."
            })
            .render(boxed, buf);
    }
}
