//! The hub: separate files and matches, filters, and source/definition previews.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use vvv_engine::{Answer, Confidence, Impact};

use super::files::{ListGeometry, PointerIntent, SearchFrame};
use super::filters::Restriction;
use super::{Category, Relation, Search, SearchPanel};
use crate::action::Action;
use crate::keymap::{Bar, Dispatch, Key, Keybinding, Layer, Legend, Trigger, When};
use crate::model::{MenuTarget, PanelKind, ReportView};
use crate::render::{CodeWindow, Pane};
use crate::render::{Fit, Header, Painter, Region};
use crate::screen::{BoundScreen, Panel, Screen};
use vvv_engine::protocol::vocabulary::{Files, Mark, Plural};
use vvv_engine::report::{Document, Options, View};

use Action as A;
use Dispatch::Run;

/// Shared search keys; list actions apply in Files and Matches outside inputs.
const MODE: Layer<Action> = Layer {
    name: "Search",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('g'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Filters)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+g",
                    word: "filters",
                }),
                help: "edit or clear filters; reset restrictions while keeping the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('o'))],
            dispatch: Run(A::Places),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+o",
                    word: "places",
                }),
                help: "open browsing trail and recent searches",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('F'))],
            dispatch: Run(A::FilterFiles),
            when: When::FileList,
            legend: Legend {
                bar: Some(Bar {
                    keys: "F",
                    word: "files",
                }),
                help: "fuzzy-filter result files (navigation only)",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('f'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Location)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "choose result location",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('s'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Symbol)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "choose declaration kind",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('t'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Category)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "choose result category",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('l'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Language)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "choose language",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::alt_left())],
            dispatch: Run(A::BrowseBack),
            when: When::BrowseBack,
            legend: Legend {
                bar: Some(Bar {
                    keys: "alt+←",
                    word: "back",
                }),
                help: "previous browsing location",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::alt_right())],
            dispatch: Run(A::BrowseForward),
            when: When::BrowseForward,
            legend: Legend {
                bar: Some(Bar {
                    keys: "alt+→",
                    word: "forward",
                }),
                help: "next browsing location",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('r'))],
            dispatch: Run(A::Refresh),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+r",
                    word: "refresh",
                }),
                help: "refresh search after source changes",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('f'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Location)),
            when: When::SearchList,
            legend: Legend {
                bar: Some(Bar {
                    keys: "f",
                    word: "location",
                }),
                help: "choose result location",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('t'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Category)),
            when: When::SearchList,
            legend: Legend {
                bar: Some(Bar {
                    keys: "t",
                    word: "category",
                }),
                help: "all, declarations, imports, or uses",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('['))],
            dispatch: Run(A::File(-1)),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "previous file; restore its selected match",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char(']'))],
            dispatch: Run(A::File(1)),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "next file; restore its selected match",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('p'))],
            dispatch: Run(A::PreviewTab),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "toggle source / definition focus; reveal the hidden preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('r'))],
            dispatch: Run(A::Rename),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "rename",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('m'))],
            dispatch: Run(A::MoveFile),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "move the file / the declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('M'))],
            dispatch: Run(A::MoveSymbol),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "move the file / the declaration",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('w'))],
            dispatch: Run(A::Rewrite),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "rewrite",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('o'))],
            dispatch: Run(A::Follow),
            when: When::SearchList,
            legend: Legend {
                bar: Some(Bar {
                    keys: "o",
                    word: "follow",
                }),
                help: "follow the exact reference to its definition",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('R'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Relation)),
            when: When::SearchListAnchored,
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
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "pick a symbol kind",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('L'))],
            dispatch: Run(A::OpenMenu(MenuTarget::Language)),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "pick a language",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('h'))],
            dispatch: Run(A::History),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "history",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('u'))],
            dispatch: Run(A::Undo),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "undo the newest apply",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/')), Trigger::Key(Key::char('i'))],
            dispatch: Run(A::FocusNth(1)),
            when: When::SearchList,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "query",
                }),
                help: "the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('<'))],
            dispatch: Run(A::Resize(-5)),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "resize",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('>'))],
            dispatch: Run(A::Resize(5)),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "grow the result column",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('q'))],
            dispatch: Run(A::Quit),
            when: When::SearchList,
            legend: Legend {
                bar: None,
                help: "quit",
            },
        },
    ],
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
                    keys: "⏎",
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
            dispatch: Run(A::File(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n ctrl+p",
                    word: "files",
                }),
                help: "switch files without leaving the query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('p'))],
            dispatch: Run(A::File(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "ctrl+n ctrl+p",
                    word: "files",
                }),
                help: "switch files without leaving the query",
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
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "move",
                }),
                help: "move between matches in this file",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "move",
                }),
                help: "move between matches in this file",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
            dispatch: Run(A::FocusNth(4)),
            when: When::Always,
            legend: Legend {
                bar: None,
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
                    word: "references",
                }),
                help: "enter the declaration's scope: its judged references",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "focus Files, or leave the entered relation",
            },
        },
    ],
};

/// The context for the cursor row.
const CONTEXT: Layer<Action> = Layer {
    name: "Search",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter()), Trigger::Key(Key::char('o'))],
            dispatch: Run(A::Follow),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "follow",
                }),
                help: "pick an identifier to follow",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('/'))],
            dispatch: Run(A::InspectFind),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "/",
                    word: "find",
                }),
                help: "find literal text in this preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char(':'))],
            dispatch: Run(A::InspectLine),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: ":",
                    word: "line",
                }),
                help: "go to an absolute file line in this preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('n'))],
            dispatch: Run(A::InspectNext(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "n",
                    word: "hit",
                }),
                help: "next / previous preview find hit; wraps",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('N'))],
            dispatch: Run(A::InspectNext(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "previous preview find hit; wraps",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
            dispatch: Run(A::InspectHorizontal(8)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "→",
                    word: "columns",
                }),
                help: "scroll right by eight terminal columns",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::left()), Trigger::Key(Key::char('h'))],
            dispatch: Run(A::InspectHorizontal(-8)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "scroll left by eight terminal columns",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('0'))],
            dispatch: Run(A::InspectStart),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "restore the first code column",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('z'))],
            dispatch: Run(A::ExpandPreview),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "z",
                    word: "",
                }),
                help: "expand / restore this preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('p'))],
            dispatch: Run(A::PreviewTab),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "p",
                    word: "preview",
                }),
                help: "toggle source / definition focus; reveal the hidden preview",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "esc",
                    word: "",
                }),
                help: "restore an expanded preview, otherwise return to results",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('i'))],
            dispatch: Run(A::FocusNth(1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('<'))],
            dispatch: Run(A::Resize(-5)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "<",
                    word: "resize",
                }),
                help: "resize the split",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::char('>'))],
            dispatch: Run(A::Resize(5)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "grow the result column",
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

const FILES: Layer<Action> = Layer {
    name: "Files",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::char('j'))],
            dispatch: Run(A::Move(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "move",
                }),
                help: "move between files",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::char('k'))],
            dispatch: Run(A::Move(-1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "j/k",
                    word: "move",
                }),
                help: "move between files",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "matches",
                }),
                help: "browse this file's matches",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::esc())],
            dispatch: Run(A::Back),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "return to query",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::ctrl('u'))],
            dispatch: Run(A::ClearFileFilter),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "clear file filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::right()), Trigger::Key(Key::char('l'))],
            dispatch: Run(A::FocusNth(3)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "browse this file's matches",
            },
        },
    ],
};

static FILES_PANEL: Panel = Panel {
    layer: FILES,
    kind: Some(PanelKind::List),
};

static QUERY_PANEL: Panel = Panel {
    layer: QUERY,
    kind: Some(PanelKind::Input),
};

static RESULTS_PANEL: Panel = Panel {
    layer: RESULTS,
    kind: Some(PanelKind::List),
};

static CONTEXT_PANEL: Panel = Panel {
    layer: CONTEXT,
    kind: Some(PanelKind::Text),
};

const FILE_FILTER: Layer<Action> = Layer {
    name: "File filter",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::down()), Trigger::Key(Key::ctrl('n'))],
            dispatch: Run(A::File(1)),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "↑/↓",
                    word: "files",
                }),
                help: "next matching file",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::up()), Trigger::Key(Key::ctrl('p'))],
            dispatch: Run(A::File(-1)),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "previous matching file",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "keep filter",
                }),
                help: "keep filter and return to matches",
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
                help: "restore the previous filter and selection",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "erase a character",
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
                help: "clear the file filter",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "filter file paths",
            },
        },
    ],
};

const INSPECTION_INPUT: Layer<Action> = Layer {
    name: "Preview inspection",
    bindings: &[
        Keybinding {
            triggers: &[Trigger::Key(Key::enter())],
            dispatch: Run(A::Enter),
            when: When::Always,
            legend: Legend {
                bar: Some(Bar {
                    keys: "⏎",
                    word: "accept",
                }),
                help: "keep find text or go to line",
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
                help: "restore previous find and position",
            },
        },
        Keybinding {
            triggers: &[Trigger::Key(Key::backspace())],
            dispatch: Run(A::Backspace),
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "erase a character",
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
                help: "clear inspection text",
            },
        },
        Keybinding {
            triggers: &[Trigger::Text],
            dispatch: Dispatch::Type,
            when: When::Always,
            legend: Legend {
                bar: None,
                help: "literal find text or an absolute file line",
            },
        },
    ],
};
const INSPECTION_PANEL: Panel = Panel {
    layer: INSPECTION_INPUT,
    kind: Some(PanelKind::Input),
};
pub(crate) static INSPECT_SOURCE: Screen = Screen {
    layer: MODE,
    panels: &[
        QUERY_PANEL,
        FILES_PANEL,
        RESULTS_PANEL,
        INSPECTION_PANEL,
        CONTEXT_PANEL,
    ],
};
pub(crate) static INSPECT_BODY: Screen = Screen {
    layer: MODE,
    panels: &[
        QUERY_PANEL,
        FILES_PANEL,
        RESULTS_PANEL,
        CONTEXT_PANEL,
        INSPECTION_PANEL,
    ],
};

pub(crate) static FILTER_SEARCH: Screen = Screen {
    layer: MODE,
    panels: &[
        QUERY_PANEL,
        Panel {
            layer: FILE_FILTER,
            kind: Some(PanelKind::Input),
        },
        RESULTS_PANEL,
        CONTEXT_PANEL,
        CONTEXT_PANEL,
    ],
};

/// The search screen.
pub(crate) static SEARCH: Screen = Screen {
    layer: MODE,
    panels: &[
        QUERY_PANEL,
        FILES_PANEL,
        RESULTS_PANEL,
        CONTEXT_PANEL,
        CONTEXT_PANEL,
    ],
};

pub struct SearchView<'a> {
    search: &'a Search,
    root: &'a str,
    busy: bool,
    split: u16,
    view: ReportView,
    painter: Painter,
}

impl<'a> SearchView<'a> {
    pub fn new(
        search: &'a Search,
        root: &'a str,
        busy: bool,
        painter: Painter,
        split: u16,
        view: ReportView,
    ) -> Self {
        Self {
            search,
            root,
            busy,
            painter,
            split,
            view,
        }
    }

    pub fn screen(self) -> BoundScreen<Self, 5> {
        let metadata = self.search.screen();
        BoundScreen::new(
            self,
            metadata,
            Self::layout,
            [
                Self::draw_query,
                Self::draw_files,
                Self::draw_results,
                Self::draw_context,
                Self::draw_body,
            ],
        )
    }
    pub(crate) fn frame(&self, area: Rect) -> SearchFrame {
        let regions = self.layout(Region::new(area));
        let panels = [
            SearchPanel::Query,
            SearchPanel::Files,
            SearchPanel::Results,
            SearchPanel::Context,
            SearchPanel::Body,
        ]
        .into_iter()
        .zip(regions.iter().map(|region| region.rect()))
        .collect();
        let mut lists = Vec::new();
        if self.search.results.has_file_list() {
            lists.push(self.list_geometry(SearchPanel::Files, regions[1].rect()));
            lists.push(self.list_geometry(SearchPanel::Results, regions[2].rect()));
        }
        SearchFrame {
            revision: self.search.results.revision,
            panels,
            lists,
        }
    }

    fn list_geometry(&self, panel: SearchPanel, area: Rect) -> ListGeometry {
        let s = self.search;
        let groups = s.results.file_groups();
        let selected = s.results.current();
        let active = groups
            .iter()
            .position(|g| selected.is_some_and(|m| m.path == *g.path));
        let filtering = s.results.files.edit.is_some() || !s.results.files.filter.is_empty();
        let prefix = usize::from(panel == SearchPanel::Files && filtering)
            .min(area.height.saturating_sub(3) as usize);
        let content = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1 + prefix as u16),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2 + prefix as u16),
        );
        let mut rows = Vec::new();
        let mut selection = None;
        let viewport = if panel == SearchPanel::Files {
            let width = area.width.saturating_sub(10).max(1) as usize;
            for (index, group) in groups.iter().enumerate() {
                let start = rows.len();
                let count = Fit(group.path.as_str(), width).wrapped().len();
                rows.extend((0..count).map(|_| PointerIntent::File(group.path.clone())));
                if active == Some(index) {
                    selection = Some(start..rows.len());
                }
            }
            s.results.files.viewport
        } else {
            if let Some(index) = active {
                for m in &groups[index].matches {
                    let index = rows.len();
                    if selected.is_some_and(|selected| selected.id == m.id) {
                        selection = Some(index..index + 1);
                    }
                    rows.push(PointerIntent::Match(m.id.clone()));
                }
            }
            selected
                .and_then(|m| s.results.files.match_viewports.get(&m.path))
                .copied()
                .unwrap_or_default()
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

    pub(crate) fn definition_rows(&self, area: Rect) -> usize {
        self.definition_rows_for(area, self.search.body.declaration())
    }

    pub(crate) fn definition_rows_for(
        &self,
        area: Rect,
        declaration: Option<&vvv_engine::Match>,
    ) -> usize {
        let regions = self.layout(Region::new(area));
        let body = regions[4].rect();
        let region = if body.is_empty() {
            regions[3].rect()
        } else {
            body
        };
        let mut pane = Pane::new(self.painter, Line::from("definition"), false);
        if let Some(d) = declaration {
            pane = pane.location(format!("{}:{}", d.path, d.start.line + 1));
        }
        pane.content_height(region)
    }

    pub(crate) fn full_definition_rows(
        &self,
        area: Rect,
        declaration: &vvv_engine::Match,
    ) -> usize {
        let (_, body) = self.header(area.width).areas(Region::new(area));
        let right = if self.search.expanded == Some(SearchPanel::Body) {
            body
        } else {
            body.columns(self.split).1
        };
        Pane::new(self.painter, Line::from("definition"), false)
            .location(format!(
                "{}:{}",
                declaration.path,
                declaration.start.line + 1
            ))
            .content_height(right.rect())
    }

    fn definition_pane(&self) -> Pane<'a> {
        let t = self.painter;
        let mut pane = Pane::new(
            t,
            Line::from(Span::styled("definition", t.title)),
            self.search.focus == SearchPanel::Body,
        );
        if let Some(d) = self.search.body.declaration() {
            pane = pane.location(format!("{}:{}", d.path, d.start.line + 1));
        }
        pane
    }

    fn layout(&self, area: Region) -> Vec<Region> {
        let mut regions = self.base_layout(area);
        let left = regions[1].rect();
        let (files, matches) = self.navigator_areas(left);
        regions[1] = Region::new(matches);
        regions.insert(1, Region::new(files));
        regions
    }

    fn navigator_areas(&self, area: Rect) -> (Rect, Rect) {
        if area.is_empty() {
            return (Rect::default(), Rect::default());
        }
        if !self.search.results.has_file_list() {
            return (Rect::default(), area);
        }
        if area.height < 6 {
            let height = area.height.saturating_sub(1);
            if self.search.focus == SearchPanel::Files {
                return (
                    Rect::new(area.x, area.y, area.width, height),
                    Rect::new(area.x, area.y + height, area.width, area.height - height),
                );
            }
            return (
                Rect::new(area.x, area.y, area.width, area.height.min(1)),
                Rect::new(area.x, area.y + area.height.min(1), area.width, height),
            );
        }
        let width = area.width.saturating_sub(10).max(1) as usize;
        let needed = self
            .search
            .results
            .unfiltered_file_groups()
            .iter()
            .map(|group| Fit(group.path.as_str(), width).wrapped().len())
            .sum::<usize>()
            + 3;
        let height = (area.height / 2)
            .max(3)
            .min(needed.min(u16::MAX as usize) as u16);
        (
            Rect::new(area.x, area.y, area.width, height),
            Rect::new(
                area.x,
                area.y + height - 1,
                area.width,
                area.height - height + 1,
            ),
        )
    }

    fn base_layout(&self, area: Region) -> Vec<Region> {
        let (top, body) = self.header(area.rect().width).areas(area);
        if let Some(panel) = self.search.expanded {
            return if panel == SearchPanel::Body {
                vec![
                    top,
                    Region::new(Rect::default()),
                    Region::new(Rect::default()),
                    body,
                ]
            } else {
                vec![
                    top,
                    Region::new(Rect::default()),
                    body,
                    Region::new(Rect::default()),
                ]
            };
        }
        let (left, right) = body.columns(self.split);
        if area.rect().width < 110 || right.rect().height < 14 {
            if self.search.focus == SearchPanel::Body
                || (self.search.definition_tab && self.search.focus != SearchPanel::Context)
            {
                vec![top, left, Region::new(Rect::default()), right]
            } else {
                vec![top, left, right, Region::new(Rect::default())]
            }
        } else {
            let (source, definition) = right.split(right.rect().height * 3 / 5);
            vec![top, left, source, definition]
        }
    }
    fn draw_query(&self, area: Rect, buf: &mut Buffer) {
        self.header(area.width).render(area, buf);
    }
    fn draw_files(&self, area: Rect, buf: &mut Buffer) {
        if !area.is_empty() {
            self.files(area, buf);
        }
    }
    fn draw_results(&self, area: Rect, buf: &mut Buffer) {
        if self.search.results.has_file_list() {
            self.matches(area, buf);
        } else {
            self.results(area, buf);
        }
    }
    fn draw_context(&self, area: Rect, buf: &mut Buffer) {
        self.context(area, buf);
    }
    fn draw_body(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (s, t) = (self.search, self.painter);
        let declaration = s.body.declaration();
        let pane = self.definition_pane();
        let mut rows = Vec::new();
        let mut empty = s.body.message.as_deref().unwrap_or("");
        if let Some(d) = declaration
            && let Some(preview) = &s.body.preview
            && preview.path == d.path
        {
            empty = "Declaration source unavailable";
            if let Some(lines) = s.body.lines(d)
                && let Some(symbol) = s.body.symbol(d)
            {
                let height = pane.content_height(area);
                let offset = s.body.scroll.min(lines.len().saturating_sub(1));
                let first = lines.start + offset;
                let hit = d
                    .symbol
                    .as_ref()
                    .filter(|selected| selected.span != symbol.span)
                    .map(|selected| selected.name_span);
                rows = CodeWindow {
                    preview,
                    painter: t,
                    visible: first..lines.end.min(first.saturating_add(height)),
                    width: area.width.saturating_sub(2) as usize,
                    declaration: Some(symbol.span),
                    origin: hit,
                    marked: None,
                    horizontal: s.body.inspection.horizontal,
                    hits: &s.body.inspection.hits,
                    active: s.body.inspection.active(),
                }
                .rows();
            }
        }
        let status = if s.body.pending().is_some() {
            "updating"
        } else if declaration.is_some_and(|d| !s.locations.includes(&d.path)) {
            "outside location"
        } else {
            ""
        };
        let position = declaration
            .and_then(|d| s.body.lines(d))
            .map(|lines| {
                let first = lines.start + s.body.scroll.min(lines.len().saturating_sub(1));
                format!("{}–{}", first + 1, (first + rows.len()).min(lines.end))
            })
            .unwrap_or_default();
        let pane = pane
            .right(Line::from(Span::styled(position, t.dim)))
            .footer(self.inspection_footer(
                &s.body.inspection,
                status,
                area.width.saturating_sub(4) as usize,
            ));
        pane.rows(rows).empty(empty).render(area, buf);
    }
    fn header(&self, width: u16) -> Header<'a> {
        let (s, t) = (self.search, self.painter);
        let focused = s.focus == SearchPanel::Query;
        let right = if self.busy {
            "searching"
        } else if s.stale {
            "stale"
        } else if matches!(s.page, super::browse::BrowsePage::Definition(_)) {
            "definition"
        } else {
            "search"
        };
        let placeholder = if s.query.is_empty() && !focused {
            Span::styled("type to search", t.dim)
        } else {
            Span::raw("")
        };
        let right = format!("Query · {right}");
        let available =
            width.saturating_sub(Line::from(right.as_str()).width() as u16 + 12) as usize;
        let root = if Line::from(self.root).width() <= available {
            self.root
        } else {
            self.root.rsplit(['/', '\\']).next().unwrap_or(self.root)
        };
        let mut title = vec![Span::styled(" vvv ", t.title)];
        if Line::from(root).width() <= available {
            title.push(Span::styled(format!("{root} "), t.dim));
        }
        Header::new(t, focused, Line::from(title))
            .right(Line::from(Span::styled(right, t.dim)))
            .bottom(self.restrictions())
            .line(Line::from(vec![
                Span::styled("> ", t.key),
                Span::raw(s.query.text().to_owned()),
                t.caret(focused),
                placeholder,
            ]))
    }

    fn restrictions(&self) -> Line<'static> {
        let mut spans = vec![Span::raw(" ")];
        let (position, total) = self.search.trail.position();
        if total > 1 {
            spans.push(Span::styled(
                format!("{position}/{total}"),
                self.painter.key,
            ));
        }
        let context_spans = spans.len();
        for r in Restriction::ALL {
            if *r == Restriction::Files && !self.search.results.has_file_list() {
                continue;
            }
            if self.search.results.is_anchored()
                && !matches!(r, Restriction::Location | Restriction::Files)
            {
                continue;
            }
            if let Some(value) = r.value(self.search) {
                if spans.len() > 1 {
                    spans.push(Span::styled(" · ", self.painter.dim));
                }
                spans.push(Span::styled(format!("{}: ", r.key()), self.painter.dim));
                spans.push(Span::styled(value, self.painter.key));
            }
        }
        if spans.len() == context_spans {
            if spans.len() > 1 {
                spans.push(Span::styled(" · ", self.painter.dim));
            }
            spans.push(Span::styled(
                if self.search.results.is_anchored() {
                    format!("workspace · {}", self.search.results.relation.label())
                } else {
                    "workspace · all results".into()
                },
                self.painter.dim,
            ));
        }
        spans.push(Span::raw(" "));
        Line::from(spans)
    }

    fn empty_reason(&self) -> String {
        let s = self.search;
        if self.busy {
            return "searching…".into();
        }
        if s.results.eligible_count() > 0 && !s.results.files.filter.trim().is_empty() {
            return "No files pass the file filter.".into();
        }
        if s.results.is_anchored() {
            return "No occurrences in this view.".into();
        }
        if s.results.category != Category::All
            && s.results
                .matches
                .iter()
                .any(|m| s.locations.includes(&m.path))
        {
            return format!(
                "No {} in loaded results.",
                s.results.category.label().to_lowercase()
            );
        }
        if s.query.is_empty() {
            return "Type a name or pattern to search.".into();
        }
        let restrictions: Vec<_> = Restriction::ALL
            .iter()
            .filter(|r| r.value(s).is_some())
            .collect();
        if restrictions.is_empty() {
            "No matches for this query.".into()
        } else {
            "No matches with active filters.".into()
        }
    }

    fn empty_rows(&self, width: usize, height: usize) -> Vec<Line<'static>> {
        self.empty_reason()
            .lines()
            .flat_map(|line| Fit(line, width.saturating_sub(2)).wrapped())
            .take(height)
            .map(|row| Line::from(vec![Span::raw(" "), Span::styled(row, self.painter.dim)]))
            .collect()
    }

    fn results(&self, area: Rect, buf: &mut Buffer) {
        let (s, t) = (self.search, self.painter);
        let width = area.width.saturating_sub(2) as usize;
        let view = self.view.view();

        if s.results.has_file_list() {
            self.matches(area, buf);
            return;
        }
        let (rows, selectable) = match s.results.relation {
            Relation::Impact => s
                .results
                .impact
                .as_ref()
                .map_or_else(|| (Vec::new(), Vec::new()), |i| Self::impact_rows(t, i)),
            Relation::Definition => s.results.definition.as_ref().map_or_else(
                || (Vec::new(), Vec::new()),
                |e| Self::present(t, &*view, &Answer::Explain((**e).clone()), width),
            ),
            Relation::Deps => s.results.deps.as_ref().map_or_else(
                || (Vec::new(), Vec::new()),
                |d| Self::present(t, &*view, &Answer::Deps((**d).clone()), width),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        let empty = if s.query.is_empty() {
            ""
        } else if self.busy {
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
            None => Line::from(Span::styled(
                format!(
                    "results · {}",
                    Files::among(s.results.listed().iter().map(|m| m.path.as_path()))
                ),
                t.title,
            )),
        };
        Pane::new(t, title, s.focus == SearchPanel::Results)
            .right(Line::from(Span::styled(
                format!(
                    "{}/{}",
                    if selectable.is_empty() {
                        0
                    } else {
                        s.results.cursor.index + 1
                    },
                    selectable.len()
                ),
                t.dim,
            )))
            .footer(self.categories(width))
            .rows(rows)
            .cursor(selectable.get(s.results.cursor.index).copied())
            .emphasized(s.focus == SearchPanel::Results)
            .empty(empty)
            .render(area, buf);
    }

    fn categories(&self, width: usize) -> Line<'static> {
        let (s, t) = (self.search, self.painter);
        if s.results.is_anchored() {
            let mut line = Line::from(Span::styled(
                format!(" {} ", s.results.relation.label()),
                t.dim,
            ));
            if s.results.relation.is_references()
                && let Some(references) = &s.results.references
            {
                let counts = [
                    Confidence::Resolved,
                    Confidence::Unresolved,
                    Confidence::Other,
                ]
                .map(|c| {
                    (
                        Mark::from(c),
                        references
                            .occurrences
                            .iter()
                            .filter(|o| o.confidence == c)
                            .count(),
                    )
                });
                let summary = t.line(&vvv_engine::protocol::display::Line::counts(&counts));
                line.spans.extend(summary.spans);
                line.spans.push(Span::raw(" "));
            }
            return line;
        }
        let short = ["All", "Decl", "Imp", "Uses"];
        let full_width: usize = Category::ALL.iter().map(|c| c.label().len() + 6).sum();
        let spans = Category::ALL
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let count = s
                    .results
                    .matches
                    .iter()
                    .filter(|m| c.includes(m.role))
                    .count();
                let label = if full_width <= width {
                    c.count_label(count)
                } else {
                    match (c, count) {
                        (Category::Uses, 1) => "Use",
                        _ => short[i],
                    }
                };
                if *c == s.results.category {
                    Span::styled(format!(" [{label} {count}] "), t.key)
                } else {
                    Span::styled(format!("{label} {count} "), t.dim)
                }
            })
            .collect::<Vec<_>>();
        let line = Line::from(spans);
        if line.width() <= width {
            line
        } else {
            Line::from(Span::styled(
                format!(
                    " {} {} ",
                    s.results.category.count_label(s.results.len()),
                    s.results.len()
                ),
                t.key,
            ))
        }
    }

    fn files(&self, area: Rect, buf: &mut Buffer) {
        let (s, t) = (self.search, self.painter);
        let width = area.width.saturating_sub(2) as usize;
        let groups = s.results.file_groups();
        let selected = s.results.current();
        let active = groups
            .iter()
            .position(|g| selected.is_some_and(|m| m.path == *g.path));
        let editing = s.results.files.edit.is_some();
        let filtering = editing || !s.results.files.filter.is_empty();
        let path_width = width.saturating_sub(8).max(1);
        let geometry = self.list_geometry(SearchPanel::Files, area);
        let visible_rows = geometry.offset..geometry.offset + geometry.content.height as usize;
        let mut rows = Vec::new();
        let mut cursor = None;
        let mut display_row = 0;
        for (i, group) in groups.iter().enumerate() {
            let chosen = active == Some(i);
            let mut offset = 0;
            let wrapped = Fit(group.path.as_str(), path_width).wrapped();
            let last = wrapped.len().saturating_sub(1);
            for (row, text) in wrapped.into_iter().enumerate() {
                let shown = visible_rows.contains(&display_row);
                display_row += 1;
                if !shown {
                    offset += text.chars().count();
                    continue;
                }
                let mut spans = vec![Span::styled(
                    if chosen && row == 0 { "> " } else { "  " },
                    if chosen {
                        t.selection_marker(s.focus == SearchPanel::Files)
                    } else {
                        t.key
                    },
                )];
                let filename = group
                    .path
                    .as_str()
                    .rfind('/')
                    .map_or(0, |j| group.path.as_str()[..j + 1].chars().count());
                for (j, c) in text.chars().enumerate() {
                    let position = offset + j;
                    let style = if group.rank.positions.binary_search(&position).is_ok() {
                        t.key
                    } else if position >= filename {
                        t.title
                    } else {
                        ratatui::style::Style::new()
                    };
                    if let Some(span) = spans.last_mut().filter(|span| span.style == style) {
                        span.content.to_mut().push(c);
                    } else {
                        spans.push(Span::styled(c.to_string(), style));
                    }
                }
                offset += text.chars().count();
                if row == last {
                    let used = Line::from(spans.clone()).width();
                    let count = group.matches.len().to_string();
                    spans.push(Span::raw(
                        " ".repeat(width.saturating_sub(used + count.len() + 1)),
                    ));
                    spans.push(Span::styled(format!("{count} "), t.dim));
                }
                let line = Line::from(spans);
                rows.push(if chosen {
                    t.selected_line(line, s.focus == SearchPanel::Files, width)
                } else {
                    line
                });
                if chosen && row == last {
                    cursor = Some(rows.len() - 1);
                }
            }
        }
        let eligible = s.results.eligible_count();
        let total_files = s.results.eligible_file_count();
        let visible: usize = groups.iter().map(|g| g.matches.len()).sum();
        let count = if s.results.files.filter.trim().is_empty() {
            format!(
                "{}/{} · {}{}",
                active.map_or(0, |i| i + 1),
                total_files,
                Plural(visible, "hit"),
                geometry.indicator()
            )
        } else {
            format!(
                "{}/{} of {} · {visible}/{}{}",
                active.map_or(0, |i| i + 1),
                groups.len(),
                total_files,
                Plural(eligible, "hit"),
                geometry.indicator()
            )
        };
        let prefix = if filtering {
            vec![Line::from(vec![
                Span::styled(" files: ", t.dim),
                Span::raw(Fit(&s.results.files.filter, width.saturating_sub(9)).to_string()),
                t.caret(editing),
            ])]
        } else {
            Vec::new()
        };
        Pane::new(
            t,
            Line::from(Span::styled(
                if editing { "Files · filter" } else { "Files" },
                t.title,
            )),
            s.focus == SearchPanel::Files,
        )
        .right(Line::from(Span::styled(count, t.dim)))
        .prefix(prefix)
        .rows(rows)
        .cursor(cursor)
        .emphasized(s.focus == SearchPanel::Files)
        .list_offset(0)
        .empty(if eligible == 0 {
            "no result files"
        } else {
            "no matching files"
        })
        .render(area, buf);
    }

    fn matches(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (s, t) = (self.search, self.painter);
        let width = area.width.saturating_sub(2) as usize;
        let groups = s.results.file_groups();
        let selected = s.results.current();
        let active = groups
            .iter()
            .position(|g| selected.is_some_and(|m| m.path == *g.path));
        let eligible = s.results.eligible_count();
        let visible: usize = groups.iter().map(|g| g.matches.len()).sum();
        let file_matches = active.map_or(&[][..], |i| groups[i].matches.as_slice());
        let line_width = file_matches
            .iter()
            .map(|m| m.start.line + 1)
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let position = file_matches
            .iter()
            .position(|m| selected.is_some_and(|current| current.id == m.id));
        let geometry = self.list_geometry(SearchPanel::Results, area);
        let file_start = active.map_or(0, |i| groups[..i].iter().map(|g| g.matches.len()).sum());
        let mut rows: Vec<Line<'static>> = file_matches
            .iter()
            .enumerate()
            .skip(geometry.offset)
            .take(geometry.content.height as usize)
            .map(|(i, m)| {
                let mut spans = vec![
                    Span::styled(
                        if position == Some(i) { "> " } else { "  " },
                        if position == Some(i) {
                            t.selection_marker(s.focus == SearchPanel::Results)
                        } else {
                            t.key
                        },
                    ),
                    Span::styled(format!("{:>line_width$}  ", m.start.line + 1), t.dim),
                ];
                let occurrence = s.results.occurrence_at(file_start + i);
                if let Some(o) = occurrence {
                    spans.push(t.glyph(Mark::from(o.confidence)));
                }
                if self.view == ReportView::Detailed
                    && let Some(o) = occurrence
                {
                    return t.line(&self.view.view().relation(o, i + 1, width).line);
                }
                let used = Line::from(spans.clone()).width();
                spans.extend(t.excerpt(
                    &vvv_engine::protocol::display::Line::hit(
                        m,
                        vvv_engine::protocol::display::Role::Plain,
                    ),
                    width.saturating_sub(used),
                ));
                Line::from(spans)
            })
            .collect();
        let title = if let Some(subject) = &s.results.subject {
            format!("Matches · {}", subject.name)
        } else {
            "Matches".into()
        };
        if rows.is_empty() {
            rows = self.empty_rows(width, geometry.content.height as usize);
        }
        Pane::new(
            t,
            Line::from(Span::styled(title, t.title)),
            s.focus == SearchPanel::Results,
        )
        .right(Line::from(Span::styled(
            format!(
                "{}/{}{}",
                position.map_or(0, |i| i + 1),
                file_matches.len(),
                geometry.indicator()
            ),
            t.dim,
        )))
        .footer(self.categories(width))
        .rows(rows)
        .cursor(position.and_then(|i| i.checked_sub(geometry.offset)))
        .emphasized(s.focus == SearchPanel::Results)
        .list_offset(0)
        .empty(if eligible == 0 && self.busy {
            "searching…"
        } else if visible == 0 && !s.results.files.filter.is_empty() {
            "no matching files"
        } else {
            "no matches"
        })
        .render(area, buf);
        if area.height > 1 {
            buf[(area.x, area.y)].set_symbol("├");
            buf[(area.right().saturating_sub(1), area.y)].set_symbol("┤");
        }
    }

    fn inspection_footer(
        &self,
        inspection: &super::inspection::Inspection,
        status: &str,
        width: usize,
    ) -> Line<'static> {
        use super::inspection::InspectionKind;
        let t = self.painter;
        if let Some(edit) = &inspection.edit {
            let prefix = if edit.kind == InspectionKind::Find {
                "/"
            } else {
                "line: "
            };
            let suffix = if let Some(error) = &edit.error {
                format!(" · {error}")
            } else if edit.kind == InspectionKind::Find {
                format!(
                    " · {}/{}",
                    inspection.cursor.map_or(0, |i| i + 1),
                    inspection.hits.len()
                )
            } else {
                String::new()
            };
            let budget =
                width.saturating_sub(Span::raw(prefix).width() + Span::raw(&suffix).width() + 3);
            let mut used = 0;
            let text: String = edit
                .text
                .chars()
                .rev()
                .take_while(|c| {
                    used += Span::raw(c.to_string()).width();
                    used <= budget
                })
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            return Line::from(vec![
                Span::raw(" "),
                Span::styled(prefix, t.key),
                Span::raw(text),
                t.caret(true),
                Span::styled(suffix, if edit.error.is_some() { t.error } else { t.dim }),
                Span::raw(" "),
            ]);
        }
        let mut parts = Vec::new();
        if !inspection.term.is_empty() {
            parts.push(format!(
                "/{} · {}/{}",
                inspection.term,
                inspection.cursor.map_or(0, |i| i + 1),
                inspection.hits.len()
            ));
        }
        if inspection.horizontal > 0 {
            parts.push(format!("col {}", inspection.horizontal + 1));
        }
        if self.search.expanded.is_some() {
            parts.push("expanded".into());
        }
        if !status.is_empty() {
            parts.push(status.into());
        }
        let text = parts.join(" · ");
        if text.is_empty() {
            Line::default()
        } else {
            Line::from(Span::styled(
                format!(" {} ", Fit(&text, width.saturating_sub(2))),
                if status.is_empty() { t.key } else { t.warning },
            ))
        }
    }

    /// A report answer as rows: what `Document`/`View` compose, with the
    /// rows that name a source selectable.
    fn present(
        t: Painter,
        view: &dyn View,
        answer: &Answer,
        width: usize,
    ) -> (Vec<Line<'static>>, Vec<usize>) {
        let document = Document::of(answer);
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
        let (s, t) = (self.search, self.painter);
        if area.is_empty() {
            return;
        }
        let focused = s.focus == SearchPanel::Context;
        let site = s.results.current_site();
        let displayed = s.displayed_source();
        let title = Line::from(Span::styled("source", t.title));
        let mut rows: Vec<Line> = Vec::new();
        let head = 0;
        let mut panel = Pane::new(t, title, focused).empty("");
        if let Some(anchor) = &displayed {
            panel = panel.location(format!("{}:{}", anchor.path, anchor.line + 1));
        } else if let Some((path, line)) = &site {
            panel = panel.location(format!("{}:{}", path, line + 1));
        }
        let status = if displayed.as_ref().is_some_and(|anchor| {
            s.preview_dirty || site.as_ref().is_some_and(|(path, _)| *path != anchor.path)
        }) {
            if s.stale && s.preview_dirty && !self.busy {
                "stale"
            } else {
                "updating"
            }
        } else {
            ""
        };
        panel = panel.footer(self.inspection_footer(
            &s.inspection,
            status,
            area.width.saturating_sub(4) as usize,
        ));
        // Source rows fill the space below pane metadata.
        let inner_height = panel.content_height(area);
        let inner_width = area.width.saturating_sub(2) as usize;
        if let (Some(anchor), Some(preview)) = (displayed.as_ref(), &s.preview) {
            let height = inner_height.saturating_sub(head);
            let first = s
                .preview_scroll
                .unwrap_or_else(|| s.preview_anchor())
                .min(preview.line_count().saturating_sub(1));
            let highlight = anchor.hit.map(|span| (span.start, span.end));
            rows.extend(
                CodeWindow {
                    preview,
                    painter: t,
                    visible: first..first.saturating_add(height),
                    width: inner_width,
                    declaration: None,
                    origin: highlight.map(|(start, end)| vvv_engine::Span::new(start, end)),
                    marked: anchor.lines,
                    horizontal: s.inspection.horizontal,
                    hits: &s.inspection.hits,
                    active: s.inspection.active(),
                }
                .rows(),
            );
            panel = panel.right(Line::from(Span::styled(
                format!(
                    "{}–{}/{}",
                    first + 1,
                    first + rows.len(),
                    preview.line_count()
                ),
                t.dim,
            )));
        }
        panel.rows(rows).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_regions_stay_fixed_across_selection_loading_and_empty_results() {
        let mut search = Search::default();
        search.results.replace(crate::fixtures::search().matches);
        search.selection_changed();
        let layout = |s: &Search, width, height| {
            SearchView::new(
                s,
                "repo",
                false,
                Painter::plain(),
                40,
                ReportView::default(),
            )
            .layout(Region::new(Rect::new(0, 0, width, height)))
        };
        let declaration = layout(&search, 120, 30);
        assert!(!declaration[3].rect().is_empty());
        assert!(!declaration[4].rect().is_empty());
        assert_eq!(
            declaration[3].rect().height + declaration[4].rect().height,
            declaration[1].rect().height + declaration[2].rect().height - 1
        );
        search.moved(1);
        let uses = layout(&search, 120, 30);
        assert_eq!(declaration, uses, "selection does not resize previews");
        assert_eq!(uses[3].rect().x, uses[4].rect().x);
        assert_eq!(uses[3].rect().width, uses[4].rect().width);
        assert_eq!(
            uses[3].rect().height + uses[4].rect().height,
            uses[1].rect().height + uses[2].rect().height - 1
        );
        search.body.clear();
        assert_eq!(
            uses,
            layout(&search, 120, 30),
            "loading does not resize previews"
        );
        search.results.replace(vec![]);
        search.selection_changed();
        let empty = layout(&search, 120, 30);
        assert_eq!(empty[3..], uses[3..], "empty results keep both regions");
        let narrow = layout(&search, 90, 24);
        assert!(narrow[4].rect().is_empty());
        search.results.replace(crate::fixtures::search().matches);
        search.selection_changed();
        assert_eq!(narrow[3..], layout(&search, 90, 24)[3..]);
        search.focus_nth(5);
        let definition = layout(&search, 90, 24);
        assert!(definition[3].rect().is_empty());
        assert_eq!(definition[4].rect(), narrow[3].rect());
        let short = layout(&search, 120, 16);
        assert!(short[3].rect().is_empty());
        assert!(!short[4].rect().is_empty());
        search.focus_nth(4);
        let short_source = layout(&search, 120, 16);
        assert!(short_source[4].rect().is_empty());
        assert_eq!(short_source[3].rect(), short[4].rect());
    }
}
