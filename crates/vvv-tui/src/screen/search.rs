//! The hub: a query, flat result rows (`●` first, `→` dim), and context
//! for the cursor row — what it is, then the source around it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::{Match, Role};

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{MenuTarget, Model, PanelKind, SearchPanel};
use crate::render::Pane;
use crate::render::{Header, Painter, Region};
use vvv_engine::protocol::vocabulary::{Files, Mark};
use vvv_engine::report::{Document, Options};

use Action as A;
use Dispatch::Run;

/// The search screen has no keys of its own: every key is a panel's, or the
/// default for the panel's kind.
const MODE: Layer<Action> = Layer {
    name: "Search",
    bindings: &[],
};

/// The query: a name, a pattern, or filters.
const QUERY: Layer<Action> = Layer {
    name: "Search",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↓",
                    word: "results",
                }),
                help: "results",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Clear),
            when: When::QueryNotEmpty,
            legend: Legend {
                bar: None,
                help: "clear",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Quit),
            when: When::QueryEmpty,
            legend: Legend {
                bar: None,
                help: "quit",
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
            triggers: &[Trigger::Key(Key::ctrl('n'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n ctrl+p",
                    word: "walk",
                }),
                help: "walk the results without leaving the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('p'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n ctrl+p",
                    word: "walk",
                }),
                help: "walk the results without leaving the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "a name, an ast-grep pattern, or filters: symbol:trait name:Foo kind:impl_item lang:rust",
            },
        },
    ],
};

/// The result rows.
const RESULTS: Layer<Action> = Layer {
    name: "Search",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('r'))],
            dispatch: Run(A::Rename),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "r",
                    word: "rename",
                }),
                help: "rename",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('m'))],
            dispatch: Run(A::MoveFile),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "m/M",
                    word: "move file/symbol",
                }),
                help: "move the file / the declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('M'))],
            dispatch: Run(A::MoveSymbol),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "m/M",
                    word: "move file/symbol",
                }),
                help: "move the file / the declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('w'))],
            dispatch: Run(A::Rewrite),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "w",
                    word: "rewrite",
                }),
                help: "rewrite",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('s'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Symbol)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "s",
                    word: "kind",
                }),
                help: "pick a symbol kind",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('L'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Language)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "L",
                    word: "lang",
                }),
                help: "pick a language",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('h'))],
            dispatch: Run(A::History),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "h",
                    word: "history",
                }),
                help: "history",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('u'))],
            dispatch: Run(A::Undo),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "undo the newest apply",
            },
        },
        Keybinding {
            triggers: &[
                Trigger::Key(Key::right()),
                Trigger::Key(Key::enter()),
                Trigger::Key(Key::char('l')),
            ],
            dispatch: Run(A::FocusNth(3)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "→",
                    word: "context",
                }),
                help: "the context panel",
            },
        },
        Keybinding {
            triggers: &[
                Trigger::Key(Key::esc()),
                Trigger::Key(Key::char('/')),
                Trigger::Key(Key::char('i')),
            ],
            dispatch: Run(A::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "query",
                }),
                help: "the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('<')), Trigger::Key(Key::char('>'))],
            dispatch: Run(A::Resize(-5)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "resize",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('q'))],
            dispatch: Run(A::Quit),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "quit",
            },
        },
    ],
};

/// The context for the cursor row.
const CONTEXT: Layer<Action> = Layer {
    name: "Search",
    bindings: &[
        Keybinding {
            triggers: &[
                Trigger::Key(Key::esc()),
                Trigger::Key(Key::left()),
                Trigger::Key(Key::char('h')),
            ],
            dispatch: Run(A::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "←",
                    word: "results",
                }),
                help: "the results",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/')), Trigger::Key(Key::char('i'))],
            dispatch: Run(A::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('<')), Trigger::Key(Key::char('>'))],
            dispatch: Run(A::Resize(-5)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "< >",
                    word: "resize",
                }),
                help: "resize the split",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('q'))],
            dispatch: Run(A::Quit),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "quit",
            },
        },
    ],
};

static QUERY_PANEL: Panel = Panel {
    layer: QUERY,
    kind: Some(PanelKind::Input),
    content: draw_query,
};

static RESULTS_PANEL: Panel = Panel {
    layer: RESULTS,
    kind: Some(PanelKind::List),
    content: draw_results,
};

static CONTEXT_PANEL: Panel = Panel {
    layer: CONTEXT,
    kind: Some(PanelKind::Text),
    content: draw_context,
};

/// The search screen.
pub(crate) static SEARCH: Screen = Screen {
    layer: MODE,
    panels: &[QUERY_PANEL, RESULTS_PANEL, CONTEXT_PANEL],
    layout,
};

fn layout(model: &Model, painter: Painter, area: Region) -> Vec<Region> {
    let (top, body) = SearchView::new(model, painter).header().areas(area);
    let (left, right) = body.columns(model.split);
    vec![top, left, right]
}

fn draw_query(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    SearchView::new(model, painter).header().render(area, buf);
}

fn draw_results(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    SearchView::new(model, painter).results(area, buf);
}

fn draw_context(model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
    SearchView::new(model, painter).context(area, buf);
}

pub struct SearchView<'a> {
    model: &'a Model,
    painter: Painter,
}

impl<'a> SearchView<'a> {
    pub fn new(model: &'a Model, painter: Painter) -> Self {
        Self { model, painter }
    }

    fn header(&self) -> Header<'a> {
        let (m, t) = (self.model, self.painter);
        let s = &m.search;
        let focused = s.focus == SearchPanel::Query;
        let declarations = s.results.declarations().count();
        let uses = s.results.matches.len() - declarations;
        let files = Files::among(s.results.matches.iter().map(|x| x.path.as_path()));
        let mut right = Vec::new();
        if declarations > 0 {
            right.push(t.glyph(Mark::Declaration));
            right.push(Span::raw(format!("{declarations}  ")));
        }
        if uses > 0 {
            right.push(Span::styled("○ ", t.dim));
            right.push(Span::raw(format!("{uses}  ")));
        }
        if files.count() > 0 {
            right.push(Span::styled(files.to_string(), t.dim));
        }
        let placeholder = if s.query.is_empty() && !focused {
            Span::styled("type to search", t.dim)
        } else {
            Span::raw("")
        };
        Header::new(
            t,
            focused,
            Line::from(vec![
                Span::styled(" vvv ", t.title),
                Span::styled(format!("{} ", m.root), t.dim),
            ]),
        )
        .right(Line::from(right))
        .line(Line::from(vec![
            Span::styled("> ", t.key),
            Span::raw(s.query.text().to_owned()),
            t.caret(focused),
            placeholder,
        ]))
    }

    fn results(&self, area: Rect, buf: &mut Buffer) {
        let (m, t) = (self.model, self.painter);
        let s = &m.search;
        // The report's rows, laid out by the view the picker has chosen.
        let document = Document::search(&s.results.matches, &[]);
        let width = area.width.saturating_sub(2) as usize;
        let presentation = m.view.view().present(&document, Options::default(), width);
        let rows: Vec<Line> = presentation.body.iter().map(|r| t.line(&r.line)).collect();
        let selectable: Vec<usize> = presentation
            .body
            .iter()
            .enumerate()
            .filter(|(_, r)| r.source.is_some())
            .map(|(i, _)| i)
            .collect();
        let empty = if s.query.is_empty() {
            ""
        } else if m.status.busy {
            "…"
        } else {
            "∅ no matches"
        };
        Pane::new(
            t,
            Line::from(Span::styled("results", t.title)),
            s.focus == SearchPanel::Results,
        )
        .rows(rows)
        .cursor(selectable.get(s.results.cursor.index).copied())
        .emphasized(true)
        .empty(empty)
        .render(area, buf);
    }

    /// What the cursor row is, then the file around it.
    fn context(&self, area: Rect, buf: &mut Buffer) {
        let (m, t) = (self.model, self.painter);
        let s = &m.search;
        let focused = s.focus == SearchPanel::Context;
        let current = s.results.current();
        let title = current.map_or_else(
            || Line::from(Span::styled("context", t.dim)),
            |c| Line::from(Span::styled(c.path.display().to_string(), t.path)),
        );
        let mut rows: Vec<Line> = Vec::new();
        if let Some(c) = current {
            match (&c.symbol, c.role) {
                (Some(sym), Role::Declaration) => {
                    let mut spans = vec![
                        t.glyph(Mark::Declaration),
                        Span::styled(format!("{} {}", sym.kind, sym.name), t.declaration),
                    ];
                    if let Some(modifier) = sym.modifier() {
                        spans.push(Span::styled(format!("   {modifier}"), t.symbol));
                    }
                    if let Some(address) = &c.address {
                        spans.push(Span::styled(format!("   ◆ {address}"), t.address));
                    }
                    rows.push(Line::from(spans));
                    let uses: Vec<&Match> = s
                        .results
                        .matches
                        .iter()
                        .filter(|x| x.role != Role::Declaration && x.text == sym.name)
                        .collect();
                    if !uses.is_empty() {
                        let files = Files::among(uses.iter().map(|x| x.path.as_path()));
                        rows.push(Line::from(vec![
                            Span::styled("○ ", t.dim),
                            Span::raw(format!("{} uses", uses.len())),
                            Span::styled(format!("   {files}"), t.dim),
                        ]));
                    }
                }
                _ => {
                    if let Some(d) = s.results.declaration_of(c)
                        && let Some(sym) = &d.symbol
                    {
                        let mut spans = vec![
                            t.glyph(Mark::Import),
                            t.glyph(Mark::Declaration),
                            Span::styled(format!("{} {}", sym.kind, sym.name), t.declaration),
                        ];
                        match &d.address {
                            Some(address) => {
                                spans.push(Span::styled(format!("   ◆ {address}"), t.address));
                            }
                            None => spans.push(Span::styled(
                                format!("   {}:{}", d.path.short(), d.start.line + 1),
                                t.dim,
                            )),
                        }
                        rows.push(Line::from(spans));
                    }
                }
            }
            if !rows.is_empty() {
                rows.push(Line::default());
            }
        }
        let head = rows.len();
        let panel = Pane::new(t, title, focused).empty("");
        // The source fills whatever the card leaves.
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(2) as usize;
        if let (Some(c), Some(preview)) = (current, &s.preview)
            && preview.path == c.path
        {
            let height = inner_height.saturating_sub(head);
            let first = s
                .preview_scroll
                .unwrap_or_else(|| m.preview_anchor())
                .min(preview.line_count().saturating_sub(1));
            rows.extend(t.source_window(
                preview,
                first,
                height,
                inner_width,
                Some((c.span.start, c.span.end)),
                Some((c.start.line as usize, c.end.line as usize)),
            ));
        }
        panel.rows(rows).render(area, buf);
    }
}
