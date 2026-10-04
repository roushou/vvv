//! Bound workspace file, outline, and source panes.
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::PanelKind;
use crate::modes::search::SearchPanel;
use crate::modes::search::files::{ListGeometry, PointerIntent, SearchFrame};
use crate::modes::workspace::WorkspaceBrowse;
use crate::problem::Problem;
use crate::render::{CodeWindow, Fit, Header, Painter, Pane, Region};
use crate::screen::{BoundScreen, Panel, Screen};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Widget,
};
use vvv_engine::protocol::vocabulary::Plural;

const MODE: Layer<Action> = Layer {
    name: "Workspace",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::char('i'))],
            dispatch: Dispatch::Run(Action::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "focus the workspace file filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('<'))],
            dispatch: Dispatch::Run(Action::Resize(-5)),
            when: When::WorkspaceSplit,
            legend: Legend {
                bar: Some(Bar {
                    keys: "<",
                    word: "resize",
                }),
                help: "shrink the list column",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('>'))],
            dispatch: Dispatch::Run(Action::Resize(5)),
            when: When::WorkspaceSplit,
            legend: Legend {
                bar: None,
                help: "grow the list column",
            },
        },
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
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
            dispatch: Dispatch::Run(Action::FocusNth(3)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "focus the outline",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Dispatch::Run(Action::ClearFileFilter),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "clear the file filter",
            },
        },
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
            triggers: &[Trigger::Key(Key::char('/'))],
            dispatch: Dispatch::Run(Action::FilterOutline),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "filter",
                }),
                help: "fuzzy-filter declaration names locally",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Dispatch::Run(Action::Clear),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "clear the outline filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::left()), Trigger::Key(Key::char('h'))],
            dispatch: Dispatch::Run(Action::FocusNth(2)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "focus workspace files",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Dispatch::Run(Action::Follow),
            when: When::WorkspaceSymbol,
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
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::page_down()), Trigger::Key(Key::char('d'))],
            dispatch: Dispatch::Run(Action::Page(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "d/u",
                    word: "page",
                }),
                help: "scroll by the visible source height",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::page_up()), Trigger::Key(Key::char('u'))],
            dispatch: Dispatch::Run(Action::Page(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "d/u",
                    word: "page",
                }),
                help: "scroll by the visible source height",
            },
        },
        Keybinding {
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
        },
    ],
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

const OUTLINE_INPUT: Layer<Action> = Layer {
    name: "Outline filter",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Dispatch::Run(Action::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "keep filter",
                }),
                help: "keep the filter and return to the outline",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Dispatch::Run(Action::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "cancel",
                }),
                help: "restore the previous outline filter and position",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::ctrl('n'))],
            dispatch: Dispatch::Run(Action::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n/ctrl+p",
                    word: "symbols",
                }),
                help: "select the next matching declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::ctrl('p'))],
            dispatch: Dispatch::Run(Action::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "select the previous matching declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "fuzzy-filter declaration names",
            },
        },
    ],
};
pub const FILTER_OUTLINE: Screen = Screen {
    layer: MODE,
    panels: &[
        WORKSPACE.panels[0],
        WORKSPACE.panels[1],
        Panel {
            layer: OUTLINE_INPUT,
            kind: Some(PanelKind::Input),
        },
        WORKSPACE.panels[3],
    ],
};
pub const INSPECT_SOURCE: Screen = Screen {
    layer: MODE,
    panels: &[
        WORKSPACE.panels[0],
        WORKSPACE.panels[1],
        WORKSPACE.panels[2],
        Panel {
            layer: crate::modes::search::screen::INSPECTION_INPUT,
            kind: Some(PanelKind::Input),
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
        let zero = Region::new(Rect::new(area.rect().x, area.rect().y, 0, 0));
        if self.browse.expanded && self.focus == SearchPanel::Context {
            return vec![zero, zero, zero, area];
        }
        let (header, rest) = self.header_widget().areas(area);
        let active = match self.focus {
            SearchPanel::Results => 1,
            SearchPanel::Context => 2,
            _ => 0,
        };
        if area.rect().width < 72 {
            let constraints: Vec<_> = (0..3)
                .map(|i| {
                    if i == active {
                        Constraint::Fill(1)
                    } else {
                        Constraint::Length(1)
                    }
                })
                .collect();
            let panels = Layout::vertical(constraints).split(rest.rect());
            return vec![
                header,
                Region::new(panels[0]),
                Region::new(panels[1]),
                Region::new(panels[2]),
            ];
        }
        let (left, source) = rest.columns(self.split);
        let (files, outline) = if left.rect().height < 10 {
            let constraints = if active == 1 {
                [Constraint::Length(1), Constraint::Fill(1)]
            } else {
                [Constraint::Fill(1), Constraint::Length(1)]
            };
            let panels = Layout::vertical(constraints).split(left.rect());
            (Region::new(panels[0]), Region::new(panels[1]))
        } else {
            let width = left.rect().width.saturating_sub(4) as usize;
            let rows: usize = self
                .browse
                .visible()
                .iter()
                .map(|path| Fit(path.as_str(), width.max(1)).wrapped().len())
                .sum();
            let height = (rows + 2)
                .clamp(3, 10)
                .min(left.rect().height.saturating_sub(4) as usize);
            left.split(height as u16)
        };
        vec![header, files, outline, source]
    }
    pub(crate) fn frame(&self, area: Rect) -> SearchFrame {
        let regions = self.layout(Region::new(area));
        let panels = [
            SearchPanel::Query,
            SearchPanel::Files,
            SearchPanel::Results,
            SearchPanel::Context,
        ]
        .into_iter()
        .zip(regions.iter().map(|region| region.rect()))
        .collect();
        let lists = vec![
            self.list_geometry(SearchPanel::Files, regions[1].rect()),
            self.list_geometry(SearchPanel::Results, regions[2].rect()),
        ];
        SearchFrame {
            revision: self.browse.revision,
            panels,
            lists,
        }
    }
    fn filter_visible(&self) -> bool {
        self.browse.outline_edit.is_some() || !self.browse.outline_filter.is_empty()
    }
    fn outline_prefix(&self, area: Rect) -> Vec<Line<'static>> {
        if !self.filter_visible() {
            return Vec::new();
        }
        let t = self.painter;
        let mut line = Line::from(Span::styled(" /", t.key));
        if self.browse.outline_filter.is_empty() && self.browse.outline_edit.is_none() {
            return Vec::new();
        }
        line.spans.extend(
            self.browse
                .outline_caret
                .line(
                    &self.browse.outline_filter,
                    t,
                    self.focus == SearchPanel::Results && self.browse.outline_edit.is_some(),
                    area.width.saturating_sub(5) as usize,
                )
                .spans,
        );
        vec![line]
    }
    fn list_geometry(&self, panel: SearchPanel, area: Rect) -> ListGeometry {
        let prefix = usize::from(panel == SearchPanel::Results && self.filter_visible())
            .min(area.height.saturating_sub(3) as usize);
        let content = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1 + prefix as u16),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2 + prefix as u16),
        );
        let mut rows = Vec::new();
        let mut selection = None;
        if panel == SearchPanel::Files {
            let width = area.width.saturating_sub(4).max(1) as usize;
            for path in self.browse.visible() {
                let start = rows.len();
                rows.extend(
                    Fit(path.as_str(), width)
                        .wrapped()
                        .iter()
                        .map(|_| PointerIntent::File(path.clone())),
                );
                if Some(path) == self.browse.file.as_ref() {
                    selection = Some(start..rows.len());
                }
            }
        } else if let Some(preview) = &self.browse.preview {
            let width = area.width.saturating_sub(2) as usize;
            let digits = self.outline_digits();
            for (index, symbol) in self.browse.visible_symbols() {
                let start = rows.len();
                let prefix = self.symbol_prefix(symbol, width, digits);
                let name_width = width.saturating_sub(2 + Span::raw(&prefix).width()).max(1);
                rows.extend(Fit(&symbol.name, name_width).wrapped().iter().map(|_| {
                    PointerIntent::Outline {
                        path: preview.path.clone(),
                        content: preview.content_id().clone(),
                        span: symbol.name_span,
                    }
                }));
                if index == self.browse.outline.index {
                    selection = Some(start..rows.len());
                }
            }
        }
        let viewport = if panel == SearchPanel::Files {
            self.browse.files_viewport
        } else {
            self.browse.outline_viewport
        };
        let offset = viewport.offset(rows.len(), content.height as usize, selection);
        ListGeometry {
            panel,
            area,
            content,
            rows,
            offset,
        }
    }
    fn outline_digits(&self) -> usize {
        let b = self.browse;
        b.symbols()
            .iter()
            .filter_map(|s| b.preview.as_ref()?.lines_in(s.name_span))
            .map(|r| r.start + 1)
            .max()
            .unwrap_or(1)
            .to_string()
            .len()
    }
    fn symbol_prefix(&self, symbol: &vvv_engine::Symbol, width: usize, digits: usize) -> String {
        let b = self.browse;
        let line = b
            .preview
            .as_ref()
            .and_then(|preview| preview.lines_in(symbol.name_span))
            .map_or(1, |r| r.start + 1);
        format!(
            "{line:>digits$}  {}",
            if width >= 24 {
                format!("{} ", symbol.kind)
            } else {
                String::new()
            }
        )
    }
    fn file_rows(&self, width: usize, visible: std::ops::Range<usize>) -> Vec<Line<'static>> {
        let (b, t) = (self.browse, self.painter);
        let mut rows = Vec::new();
        let mut row_index = 0;
        for path in b.visible() {
            let chosen = Some(path) == b.file.as_ref();
            let matched = crate::modes::search::files::PathMatch::find(path.as_str(), &b.filter)
                .unwrap_or_default();
            let filename = path
                .as_str()
                .chars()
                .collect::<Vec<_>>()
                .iter()
                .rposition(|c| *c == '/')
                .map_or(0, |i| i + 1);
            let mut position = 0;
            for (i, text) in Fit(path.as_str(), width.max(1))
                .wrapped()
                .into_iter()
                .enumerate()
            {
                let shown = visible.contains(&row_index);
                row_index += 1;
                if !shown {
                    position += text.chars().count();
                    continue;
                }
                let mut spans = vec![Span::styled(
                    if chosen && i == 0 { "> " } else { "  " },
                    t.selection_marker(self.focus == SearchPanel::Files),
                )];
                for c in text.chars() {
                    let style = if matched.positions.binary_search(&position).is_ok() {
                        t.hit
                    } else if position >= filename {
                        t.title
                    } else {
                        ratatui::style::Style::default()
                    };
                    spans.push(Span::styled(c.to_string(), style));
                    position += 1;
                }
                let line = Line::from(spans);
                rows.push(if chosen {
                    t.selected_line(line, self.focus == SearchPanel::Files, width + 2)
                } else {
                    line
                });
            }
        }
        rows
    }
    fn outline_rows(&self, width: usize, visible: std::ops::Range<usize>) -> Vec<Line<'static>> {
        let (b, t) = (self.browse, self.painter);
        let mut rows = Vec::new();
        let mut row_index = 0;
        let digits = self.outline_digits();
        for (index, symbol) in b.visible_symbols() {
            let prefix = self.symbol_prefix(symbol, width, digits);
            let name_width = width.saturating_sub(2 + Span::raw(&prefix).width()).max(1);
            let chosen = index == b.outline.index;
            let matched =
                crate::modes::search::files::PathMatch::find(&symbol.name, &b.outline_filter)
                    .unwrap_or_default();
            let mut position = 0;
            for (i, name) in Fit(&symbol.name, name_width)
                .wrapped()
                .into_iter()
                .enumerate()
            {
                let shown = visible.contains(&row_index);
                row_index += 1;
                if !shown {
                    position += name.chars().count();
                    continue;
                }
                let mut spans = vec![
                    Span::styled(
                        if chosen && i == 0 { "> " } else { "  " },
                        t.selection_marker(self.focus == SearchPanel::Results),
                    ),
                    Span::styled(
                        if i == 0 {
                            prefix.clone()
                        } else {
                            " ".repeat(Span::raw(&prefix).width())
                        },
                        t.dim,
                    ),
                ];
                for c in name.chars() {
                    spans.push(Span::styled(
                        c.to_string(),
                        if matched.positions.binary_search(&position).is_ok() {
                            t.hit
                        } else {
                            t.title
                        },
                    ));
                    position += 1;
                }
                let row = Line::from(spans);
                rows.push(if chosen {
                    t.selected_line(row, self.focus == SearchPanel::Results, width)
                } else {
                    row
                });
            }
        }
        rows
    }
    fn header(&self, area: Rect, buf: &mut Buffer) {
        self.header_widget().render(area, buf);
    }
    fn files(&self, area: Rect, buf: &mut Buffer) {
        let (b, t) = (self.browse, self.painter);
        let geometry = self.list_geometry(SearchPanel::Files, area);
        let rows = self.file_rows(
            area.width.saturating_sub(4) as usize,
            geometry.offset..geometry.offset + geometry.content.height as usize,
        );
        Pane::new(
            t,
            Line::from(Span::styled("Files", t.title)),
            self.focus == SearchPanel::Files,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{}{}{}",
                b.visible().len(),
                Plural(b.paths.len(), "file"),
                if b.inventory_loading {
                    " · updating"
                } else {
                    ""
                },
                geometry.indicator()
            ),
            t.dim,
        )))
        .rows(rows)
        .list_offset(0)
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
        let geometry = self.list_geometry(SearchPanel::Results, area);
        let rows = self.outline_rows(
            area.width.saturating_sub(2) as usize,
            geometry.offset..geometry.offset + geometry.content.height as usize,
        );
        Pane::new(
            t,
            Line::from(Span::styled("Outline", t.title)),
            self.focus == SearchPanel::Results,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}{}",
                if b.loading {
                    "updating".into()
                } else if b.outline_filter.is_empty() {
                    Plural(b.symbols().len(), "symbol").to_string()
                } else {
                    format!(
                        "{}/{}",
                        b.visible_symbols().len(),
                        Plural(b.symbols().len(), "symbol")
                    )
                },
                geometry.indicator()
            ),
            t.dim,
        )))
        .prefix(self.outline_prefix(area))
        .rows(rows)
        .list_offset(0)
        .empty(if b.loading {
            "waiting for source…"
        } else if !b.outline_filter.is_empty() {
            "no declarations match the filter"
        } else {
            "no declarations in this file"
        })
        .render(area, buf);
    }
    fn source_pane(&self) -> Pane<'static> {
        let mut pane = Pane::new(
            self.painter,
            Line::from(Span::styled("Source", self.painter.title)),
            self.focus == SearchPanel::Context,
        )
        .empty("choose a workspace file");
        if let Some(preview) = &self.browse.preview {
            pane = pane.location(preview.path.to_string());
        }
        pane
    }
    pub(crate) fn source_columns(&self, area: Rect) -> usize {
        let regions = self.layout(Region::new(area));
        // CodeWindow's file gutter occupies eight columns inside the border.
        regions[3].rect().width.saturating_sub(10) as usize
    }
    pub(crate) fn source_rows(&self, area: Rect) -> usize {
        let regions = self.layout(Region::new(area));
        self.source_pane().content_height(regions[3].rect())
    }
    fn source(&self, area: Rect, buf: &mut Buffer) {
        let (b, t) = (self.browse, self.painter);
        if let Some(problem) = self.problem {
            problem.pane(t, self.focus == SearchPanel::Context, area, buf);
            return;
        }
        let mut pane = self.source_pane();
        if let Some(preview) = &b.preview {
            pane = pane.footer(b.inspection.footer(
                t,
                b.expanded,
                if b.loading { "updating" } else { "" },
                area.width.saturating_sub(2) as usize,
            ));
            let first = b.scroll.min(preview.line_count().saturating_sub(1));
            let rows = CodeWindow {
                preview,
                painter: t,
                visible: first..first.saturating_add(pane.content_height(area)),
                width: area.width.saturating_sub(2) as usize,
                declaration: None,
                origin: b.marked(),
                marked: b.inspection.line.map(|line| (line, line)),
                horizontal: b.inspection.horizontal,
                hits: &b.inspection.hits,
                active: b.inspection.active(),
            }
            .rows();
            pane = pane
                .right(Line::from(Span::styled(
                    if rows.is_empty() {
                        format!(
                            "{}{}",
                            Plural(preview.line_count(), "line"),
                            if b.loading { " · updating" } else { "" }
                        )
                    } else {
                        format!(
                            "{}–{}/{}",
                            first + 1,
                            first + rows.len(),
                            preview.line_count()
                        )
                    },
                    t.dim,
                )))
                .rows(rows);
        }
        pane.render(area, buf);
    }
}
