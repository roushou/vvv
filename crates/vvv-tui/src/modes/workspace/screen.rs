//! Bound workspace file, outline, and source panes.
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::PanelKind;
use crate::modes::search::SearchPanel;
use crate::modes::workspace::WorkspaceBrowse;
use crate::problem::Problem;
use crate::render::{CodeWindow, Fit, Header, Painter, Pane, Region};
use crate::screen::{BoundScreen, Panel, Screen};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};
use vvv_engine::protocol::vocabulary::Plural;

const MODE: Layer<Action> = Layer {
    name: "Workspace",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Dispatch::Run(Action::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "back",
                }),
                help: "return to the preceding browsing page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('b'))],
            dispatch: Dispatch::Run(Action::Workspace),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+b",
                    word: "search",
                }),
                help: "return to the previous search intact",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::alt_left())],
            dispatch: Dispatch::Run(Action::BrowseBack),
            when: When::BrowseBack,
            legend: Legend {
                bar: Some(Bar {
                    keys: "alt+←",
                    word: "back",
                }),
                help: "previous browsing page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::alt_right())],
            dispatch: Dispatch::Run(Action::BrowseForward),
            when: When::BrowseForward,
            legend: Legend {
                bar: Some(Bar {
                    keys: "alt+→",
                    word: "forward",
                }),
                help: "next browsing page",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('r'))],
            dispatch: Dispatch::Run(Action::Refresh),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+r",
                    word: "refresh",
                }),
                help: "refresh workspace files and the opened source",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('o'))],
            dispatch: Dispatch::Run(Action::Places),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "open the browsing trail",
            },
        },
    ],
};
const INPUT: Layer<Action> = Layer {
    name: "Workspace filter",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "fuzzy-filter workspace paths locally",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Dispatch::Run(Action::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "files",
                }),
                help: "focus workspace files",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('n'))],
            dispatch: Dispatch::Run(Action::File(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n/ctrl+p",
                    word: "files",
                }),
                help: "cycle workspace files while filtering",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('p'))],
            dispatch: Dispatch::Run(Action::File(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n/ctrl+p",
                    word: "files",
                }),
                help: "cycle workspace files while filtering",
            },
        },
    ],
};
const FILES: Layer<Action> = Layer {
    name: "Workspace files",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Dispatch::Run(Action::FocusNth(3)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "outline",
                }),
                help: "browse the opened file's declarations",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/'))],
            dispatch: Dispatch::Run(Action::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "filter",
                }),
                help: "edit the fuzzy workspace file filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('p'))],
            dispatch: Dispatch::Run(Action::FocusNth(4)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "p",
                    word: "source",
                }),
                help: "focus the source preview",
            },
        },
    ],
};
const OUTLINE: Layer<Action> = Layer {
    name: "Workspace outline",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Dispatch::Run(Action::Follow),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "definition",
                }),
                help: "follow the selected declaration globally",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('p'))],
            dispatch: Dispatch::Run(Action::FocusNth(4)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "p",
                    word: "source",
                }),
                help: "focus the source preview",
            },
        },
    ],
};
const SOURCE: Layer<Action> = Layer {
    name: "Workspace source",
    bindings: &[Keybinding {
        triggers: &[Trigger::Key(Key::enter()), Trigger::Key(Key::char('o'))],
        dispatch: Dispatch::Run(Action::Follow),
        when: When::Always,
        legend: Legend {
            bar: Some(Bar {
                keys: "⏎",
                word: "follow",
            }),
            help: "pick a source identifier and follow its definition globally",
        },
    }],
};
pub const WORKSPACE: Screen = Screen {
    layer: MODE,
    panels: &[
        Panel {
            layer: INPUT,
            kind: Some(PanelKind::Input),
        },
        Panel {
            layer: FILES,
            kind: Some(PanelKind::List),
        },
        Panel {
            layer: OUTLINE,
            kind: Some(PanelKind::List),
        },
        Panel {
            layer: SOURCE,
            kind: Some(PanelKind::Text),
        },
    ],
};

pub struct WorkspaceView<'a> {
    browse: &'a WorkspaceBrowse,
    focus: SearchPanel,
    problem: Option<&'a Problem>,
    root: &'a str,
    painter: Painter,
    split: u16,
}
impl<'a> WorkspaceView<'a> {
    pub fn new(
        browse: &'a WorkspaceBrowse,
        focus: SearchPanel,
        problem: Option<&'a Problem>,
        root: &'a str,
        painter: Painter,
        split: u16,
    ) -> Self {
        Self {
            browse,
            focus,
            problem,
            root,
            painter,
            split,
        }
    }
    pub fn screen(self) -> BoundScreen<Self, 4> {
        BoundScreen::new(
            self,
            &WORKSPACE,
            Self::layout,
            [Self::header, Self::files, Self::outline, Self::source],
        )
    }
    fn header_widget(&self) -> Header<'static> {
        let t = self.painter;
        Header::new(
            t,
            self.focus == SearchPanel::Query,
            Line::from(Span::styled(" Workspace ", t.title)),
        )
        .right(Line::from(Span::styled(self.root.to_owned(), t.dim)))
        .input(
            Line::from(Span::styled(" files: ", t.key)),
            &self.browse.filter,
            &self.browse.caret,
            "fuzzy-filter paths",
        )
    }
    fn layout(&self, area: Region) -> Vec<Region> {
        let (header, rest) = self.header_widget().areas(area);
        let (left, source) = rest.columns(self.split);
        let file_height = if left.rect().height < 12 {
            (left.rect().height / 2).max(1)
        } else {
            (left.rect().height / 3).clamp(4, 10)
        };
        let (files, outline) = left.split(file_height);
        vec![header, files, outline, source]
    }
    fn header(&self, area: Rect, buf: &mut Buffer) {
        self.header_widget().render(area, buf);
    }
    fn files(&self, area: Rect, buf: &mut Buffer) {
        let (b, t) = (self.browse, self.painter);
        let visible = b.visible();
        let mut rows = Vec::new();
        let mut cursor = None;
        for path in &visible {
            let chosen = Some(*path) == b.file.as_ref();
            for (i, text) in Fit(path.as_str(), area.width.saturating_sub(4) as usize)
                .wrapped()
                .into_iter()
                .enumerate()
            {
                let mut line = t.path_line(&text);
                line.spans.insert(
                    0,
                    Span::styled(
                        if chosen && i == 0 { "> " } else { "  " },
                        t.selection_marker(self.focus == SearchPanel::Files),
                    ),
                );
                if chosen {
                    for span in &mut line.spans {
                        span.style = span
                            .style
                            .patch(t.selection(self.focus == SearchPanel::Files));
                    }
                    cursor = Some(rows.len());
                }
                rows.push(line);
            }
        }
        Pane::new(
            t,
            Line::from(Span::styled("Files", t.title)),
            self.focus == SearchPanel::Files,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{}{}",
                visible.len(),
                Plural(b.paths.len(), "file"),
                if b.inventory_loading {
                    " · updating"
                } else {
                    ""
                }
            ),
            t.dim,
        )))
        .rows(rows)
        .cursor(cursor)
        .empty(if b.inventory_loading {
            "loading workspace…"
        } else if b.paths.is_empty() {
            "no workspace files"
        } else {
            "no matching files"
        })
        .render(area, buf);
    }
    fn outline(&self, area: Rect, buf: &mut Buffer) {
        let (b, t) = (self.browse, self.painter);
        let digits = b
            .symbols()
            .iter()
            .filter_map(|s| b.preview.as_ref()?.lines_in(s.span))
            .map(|r| r.start + 1)
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let rows = b
            .symbols()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let line = b
                    .preview
                    .as_ref()
                    .and_then(|p| p.lines_in(s.span))
                    .map_or(1, |r| r.start + 1);
                Line::from(vec![
                    Span::styled(
                        if i == b.outline.index { "> " } else { "  " },
                        t.selection_marker(self.focus == SearchPanel::Results),
                    ),
                    Span::styled(format!("{line:>digits$}  "), t.dim),
                    Span::styled(format!("{} ", s.kind), t.key),
                    Span::styled(s.name.clone(), t.title),
                ])
            })
            .collect();
        Pane::new(
            t,
            Line::from(Span::styled("Outline", t.title)),
            self.focus == SearchPanel::Results,
        )
        .right(Line::from(Span::styled(
            Plural(b.symbols().len(), "symbol").to_string(),
            t.dim,
        )))
        .rows(rows)
        .cursor((!b.symbols().is_empty()).then_some(b.outline.index))
        .empty(if b.loading {
            "waiting for source…"
        } else {
            "no declarations in this file"
        })
        .render(area, buf);
    }
    fn source(&self, area: Rect, buf: &mut Buffer) {
        let (b, t) = (self.browse, self.painter);
        if let Some(problem) = self.problem {
            problem.pane(t, self.focus == SearchPanel::Context, area, buf);
            return;
        }
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled("Source", t.title)),
            self.focus == SearchPanel::Context,
        )
        .empty("choose a workspace file");
        if let Some(preview) = &b.preview {
            pane = pane.location(preview.path.to_string());
            if b.loading {
                pane = pane.footer(Line::from(Span::styled(" updating ", t.dim)));
            }
            let height = pane.content_height(area);
            let first = b.scroll.min(preview.line_count().saturating_sub(1));
            let rows = CodeWindow {
                preview,
                painter: t,
                visible: first..first.saturating_add(height),
                width: area.width.saturating_sub(2) as usize,
                declaration: None,
                origin: b.marked(),
                marked: None,
                horizontal: 0,
                hits: &[],
                active: None,
            }
            .rows();
            pane = pane
                .right(Line::from(Span::styled(
                    format!(
                        "{}–{}/{}",
                        first + 1,
                        first + rows.len(),
                        preview.line_count()
                    ),
                    t.dim,
                )))
                .rows(rows);
        }
        pane.render(area, buf);
    }
}
