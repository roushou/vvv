//! The hub: a query, flat result rows (`●` first, `→` dim), and context
//! for the cursor row — what it is, then the source around it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::Role;
use vvv_engine::{Answer, Confidence, Impact, Occurrence};

use super::{Panel, Screen};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{MenuTarget, Model, PanelKind, Relation, SearchPanel};
use crate::render::Pane;
use crate::render::{Header, Painter, Region};
use vvv_engine::protocol::vocabulary::{Files, Mark};
use vvv_engine::report::{Document, Options, View};

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
            triggers: &[Trigger::Key(Key::char('o'))],
            dispatch: Run(A::Jump),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "o",
                    word: "declaration",
                }),
                help: "jump to the declaration the row names",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('R'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Relation)),
            when: When::Anchored,
            legend: Legend {
                bar: Some(Bar {
                    keys: "R",
                    word: "relation",
                }),
                help: "what to show about the entered declaration",
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
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
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
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "scope",
                }),
                help: "enter the declaration's scope: its judged references",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/')), Trigger::Key(Key::char('i'))],
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
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "leave the declaration's scope, then the query",
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
        let mut right = Vec::new();
        if let Some(subject) = &s.results.subject {
            // Entered a declaration: the right names it and counts the
            // verdicts, the way `references` groups them.
            let kind = subject.symbol.map_or(String::new(), |k| format!("{k} "));
            right.push(t.glyph(Mark::Declaration));
            right.push(Span::styled(
                format!("{kind}{}  ", subject.name),
                t.declaration,
            ));
            match s.results.relation {
                relation if relation.is_impact() => match &s.results.impact {
                    Some(i) => {
                        right.push(t.glyph(Mark::ImportedBy));
                        right.push(Span::raw(format!("{}  ", i.consumers.len())));
                        let files = Files::among(i.consumers.iter().map(|c| c.path.as_path()));
                        right.push(Span::styled(files.to_string(), t.dim));
                    }
                    None => right.push(Span::styled("…", t.dim)),
                },
                Relation::Definition => right.push(Span::styled("definition", t.dim)),
                Relation::Deps => right.push(Span::styled("imports", t.dim)),
                _ => match &s.results.references {
                    Some(r) => {
                        for (confidence, mark) in [
                            (Confidence::Resolved, Mark::Safe),
                            (Confidence::Unresolved, Mark::Unverified),
                            (Confidence::Other, Mark::Other),
                        ] {
                            let n = r
                                .occurrences
                                .iter()
                                .filter(|o| o.confidence == confidence)
                                .count();
                            right.push(t.glyph(mark));
                            right.push(Span::raw(format!("{n}  ")));
                        }
                        right.push(Span::styled(
                            Files::among(r.occurrences.iter().map(|o| o.m.path.as_path()))
                                .to_string(),
                            t.dim,
                        ));
                    }
                    None => right.push(Span::styled("…", t.dim)),
                },
            }
        } else {
            let declarations = s.results.declarations().count();
            let uses = s.results.matches.len() - declarations;
            let files = Files::among(s.results.matches.iter().map(|x| x.path.as_path()));
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
        let width = area.width.saturating_sub(2) as usize;
        let view = m.view.view();

        // The rows: the search's hits, or, once a declaration is entered,
        // one read-only answer about it.
        let (rows, selectable): (Vec<Line>, Vec<usize>) = if s.results.is_anchored() {
            match s.results.relation {
                Relation::Impact => s
                    .results
                    .impact
                    .as_ref()
                    .map_or_else(|| (Vec::new(), Vec::new()), |i| Self::impact_rows(t, i)),
                Relation::Definition => s.results.definition.as_ref().map_or_else(
                    || (Vec::new(), Vec::new()),
                    |e| Self::present(t, &*view, &Answer::Explain(e.clone()), width),
                ),
                Relation::Deps => s.results.deps.as_ref().map_or_else(
                    || (Vec::new(), Vec::new()),
                    |d| Self::present(t, &*view, &Answer::Deps(d.clone()), width),
                ),
                _ => match &s.results.references {
                    Some(r) => {
                        let confidence = s.results.relation.confidence();
                        let shown: Vec<&Occurrence> = r
                            .occurrences
                            .iter()
                            .filter(|o| confidence.is_none_or(|c| o.confidence == c))
                            .collect();
                        let mut rows: Vec<Line> = Vec::new();
                        let mut selectable: Vec<usize> = Vec::new();
                        let mut group: Option<Confidence> = None;
                        for (i, o) in shown.iter().enumerate() {
                            if group != Some(o.confidence) {
                                group = Some(o.confidence);
                                rows.push(Self::verdict_head(t, &r.occurrences, o.confidence));
                            }
                            selectable.push(rows.len());
                            rows.push(t.line(&view.relation(o, i + 1, width).line));
                        }
                        (rows, selectable)
                    }
                    None => (Vec::new(), Vec::new()),
                },
            }
        } else {
            // The report's rows, laid out by the view the picker has chosen.
            let document = Document::search(&s.results.matches, &[]);
            let presentation = view.present(&document, Options::default(), width);
            let rows: Vec<Line> = presentation.body.iter().map(|r| t.line(&r.line)).collect();
            let selectable: Vec<usize> = presentation
                .body
                .iter()
                .enumerate()
                .filter(|(_, r)| r.source.is_some())
                .map(|(i, _)| i)
                .collect();
            (rows, selectable)
        };
        let empty = if s.query.is_empty() {
            ""
        } else if m.status.busy {
            "…"
        } else {
            "∅ no matches"
        };
        let title = match &s.results.subject {
            Some(subject) => {
                let kind = subject.symbol.map_or(String::new(), |k| format!("{k} "));
                Line::from(vec![
                    t.glyph(Mark::Declaration),
                    Span::styled(format!(" {kind}{}", subject.name), t.title),
                ])
            }
            None => Line::from(Span::styled("results", t.title)),
        };
        Pane::new(t, title, s.focus == SearchPanel::Results)
            .rows(rows)
            .cursor(selectable.get(s.results.cursor.index).copied())
            .emphasized(true)
            .empty(empty)
            .render(area, buf);
    }

    /// A report answer as rows: what `Document`/`View` compose, with the
    /// rows that name a source selectable.
    fn present(
        t: Painter,
        view: &dyn View,
        answer: &Answer,
        width: usize,
    ) -> (Vec<Line<'static>>, Vec<usize>) {
        let document = Document::of(answer, Options::default());
        let presentation = view.present(&document, Options::default(), width);
        let rows: Vec<Line> = presentation.body.iter().map(|r| t.line(&r.line)).collect();
        let selectable: Vec<usize> = presentation
            .body
            .iter()
            .enumerate()
            .filter(|(_, r)| r.source.is_some())
            .map(|(i, _)| i)
            .collect();
        (rows, selectable)
    }

    /// One verdict's heading in the anchored list: `✓ safe 12  3 files`.
    fn verdict_head(
        t: Painter,
        occurrences: &[Occurrence],
        confidence: Confidence,
    ) -> Line<'static> {
        let mark = Mark::from(confidence);
        let group: Vec<&Occurrence> = occurrences
            .iter()
            .filter(|o| o.confidence == confidence)
            .collect();
        Line::from(vec![
            t.glyph(mark),
            Span::styled(format!("{} {}", mark.word(), group.len()), t.title),
            Span::styled(
                format!(
                    "  {}",
                    Files::among(group.iter().map(|o| o.m.path.as_path()))
                ),
                t.dim,
            ),
        ])
    }

    /// The impact view: a heading per depth, then a row per consumer module,
    /// each a selectable site in the consumer's file.
    fn impact_rows(t: Painter, impact: &Impact) -> (Vec<Line<'static>>, Vec<usize>) {
        let mut rows: Vec<Line<'static>> = Vec::new();
        let mut selectable: Vec<usize> = Vec::new();
        let mut depth: Option<u32> = None;
        for consumer in &impact.consumers {
            if depth != Some(consumer.depth) {
                depth = Some(consumer.depth);
                let count = impact
                    .consumers
                    .iter()
                    .filter(|c| c.depth == consumer.depth)
                    .count();
                rows.push(Line::from(vec![
                    t.glyph(Mark::ImportedBy),
                    Span::styled(format!("imported by {count}"), t.title),
                    Span::styled(format!("   depth {}", consumer.depth), t.dim),
                ]));
            }
            selectable.push(rows.len());
            let mut spans = vec![Span::raw("  "), Span::styled(consumer.path.short(), t.path)];
            if consumer.depth > 1 {
                spans.push(Span::styled(format!("   via {}", consumer.through), t.dim));
            }
            rows.push(Line::from(spans));
        }
        (rows, selectable)
    }

    /// What the cursor row is, then the file around it.
    fn context(&self, area: Rect, buf: &mut Buffer) {
        let (m, t) = (self.model, self.painter);
        let s = &m.search;
        let focused = s.focus == SearchPanel::Context;
        let current = s.results.current();
        let consumer = s.results.current_consumer();
        let site = s.results.current_site();
        let title = site.as_ref().map_or_else(
            || Line::from(Span::styled("context", t.dim)),
            |(path, _)| Line::from(Span::styled(path.short(), t.path)),
        );
        let mut rows: Vec<Line> = Vec::new();
        if let Some(consumer) = consumer {
            rows.push(Line::from(vec![
                t.glyph(Mark::ImportedBy),
                Span::styled(format!(" depth {}  ", consumer.depth), t.declaration),
                Span::styled(consumer.module.to_string(), t.address),
            ]));
        } else if let Some(c) = current {
            match (&c.symbol, c.role) {
                // The results row already says what it is; the source below
                // is the detail. Only a use needs a card, to name what it
                // resolves to.
                (Some(_), Role::Declaration) => {}
                _ => {
                    let declaration = if s.results.is_anchored() {
                        s.results.anchored_declarations().first()
                    } else {
                        s.results.declaration_of(c)
                    };
                    if let Some(d) = declaration
                        && let Some(sym) = &d.symbol
                    {
                        let mut spans = vec![
                            t.glyph(Mark::Import),
                            t.glyph(Mark::Declaration),
                            Span::styled(format!("{} {}", sym.kind, sym.name), t.declaration),
                        ];
                        spans.push(Span::styled(
                            format!("   {}:{}", d.path.short(), d.start.line + 1),
                            t.dim,
                        ));
                        rows.push(Line::from(spans));
                    }
                }
            }
        }
        if !rows.is_empty() {
            rows.push(Line::default());
        }
        let head = rows.len();
        let panel = Pane::new(t, title, focused).empty("");
        // The source fills whatever the card leaves.
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(2) as usize;
        if let (Some((path, _)), Some(preview)) = (site.as_ref(), &s.preview)
            && preview.path == *path
        {
            let height = inner_height.saturating_sub(head);
            let first = s
                .preview_scroll
                .unwrap_or_else(|| m.preview_anchor())
                .min(preview.line_count().saturating_sub(1));
            let highlight = current
                .filter(|c| c.path == *path)
                .map(|c| (c.span.start, c.span.end));
            let span_lines = current
                .filter(|c| c.path == *path)
                .map(|c| (c.start.line as usize, c.end.line as usize));
            rows.extend(t.source_window(
                preview,
                first,
                height,
                inner_width,
                highlight,
                span_lines,
            ));
        }
        panel.rows(rows).render(area, buf);
    }
}
