//! The TUI without a terminal: `update` with plain assertions, every mode
//! rendered into a `TestBackend` and snapshotted.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::Widget;
use vvv_engine::{Answer, Intent, Request, Selection, SymbolKind};

use super::action::{Action, Effect, Event, Planned};
use super::model::{Level, MenuTarget, Mode, Model, Overlay, ReportView};
use crate::fixtures as fx;
use crate::modes::moves::MovePanel;
use crate::modes::rename::RenamePanel;
use crate::modes::rewrite::RewritePanel;
use crate::modes::search::query::Filter;
use crate::modes::search::{Relation, SearchPanel};
use crate::render::Painter;
use crate::screen::App;

fn model() -> Model {
    Model::new("~/dev/nx".into(), vec!["rust".into(), "typescript".into()])
}

fn preview(path: &str, lines: &[&str]) -> Event {
    let text = lines.join("\n");
    // Colour every `Language` token and the word `pub` like the real thing would.
    let mut highlights = Vec::new();
    for (i, _) in text.match_indices("Language") {
        highlights.push(vvv_engine::Highlight {
            span: vvv_engine::Span::new(i, i + 8),
            kind: vvv_engine::HighlightKind::Type,
        });
    }
    for (i, _) in text.match_indices("pub") {
        highlights.push(vvv_engine::Highlight {
            span: vvv_engine::Span::new(i, i + 3),
            kind: vvv_engine::HighlightKind::Keyword,
        });
    }
    highlights.sort_by_key(|h| h.span.start);
    Event::Previewed {
        identifiers: vec![],
        symbols: vec![],
        path: path.into(),
        text,
        highlights,
    }
}

/// Lines `1..=n`, each `// line k` except the given overrides.
fn numbered(n: usize, overrides: &[(usize, &str)]) -> Vec<String> {
    (1..=n)
        .map(|k| {
            overrides
                .iter()
                .find(|(line, _)| *line == k)
                .map_or_else(|| format!("// line {k}"), |(_, text)| (*text).to_owned())
        })
        .collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typed(model: &mut Model, text: &str) -> Vec<Effect> {
    text.chars()
        .flat_map(|c| model.update(Action::Input(c)))
        .collect()
}

fn generation_of(effects: &[Effect]) -> u64 {
    match effects.last() {
        Some(Effect::Search { generation, .. } | Effect::Plan { generation, .. }) => *generation,
        other => panic!("expected a search or a plan, got {other:?}"),
    }
}

/// A model showing the search fixture's matches, as if the worker answered,
/// with the declaration's file previewed.
fn searched() -> Model {
    let mut m = model();
    let effects = typed(&mut m, "Language");
    let lines = numbered(
        70,
        &[
            (64, "pub trait Language: Send + Sync {"),
            (65, "    fn name(&self) -> &str;"),
            (66, "    fn extensions(&self) -> &[&str];"),
            (67, "}"),
        ],
    );
    let text = lines.join("\n");
    let start = text.find("pub trait").unwrap();
    let end = text.find("\n}").unwrap() + 2;
    let mut matches = fx::search().matches;
    let declaration = &mut matches[0];
    declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(start, end);
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches,
        skipped: vec![],
    });
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.definition_preview(preview("src/lang/mod.rs", &refs));
    m
}

// Source fixtures stand in for the worker's already-resolved navigation reply.
impl Model {
    fn definition_reply(&mut self, event: Event) -> Event {
        let Event::Previewed {
            identifiers,
            path,
            text,
            highlights,
            mut symbols,
        } = event
        else {
            panic!("expected source fixture")
        };
        self.search.selection_changed();
        let declaration = self
            .search
            .results
            .current()
            .filter(|m| m.symbol.is_some() && m.path == path)
            .or_else(|| self.search.results.declarations().find(|m| m.path == path))
            .unwrap()
            .clone();
        let mut declaration = declaration;
        let symbol = declaration.symbol.as_mut().unwrap();
        symbol.extent = symbol.span;
        let symbol = declaration.symbol.as_ref().unwrap();
        if !symbols
            .iter()
            .any(|s| s.name_span == symbol.name_span && s.kind == symbol.kind)
        {
            symbols.push(symbol.clone());
        }
        let container = if symbol.kind == SymbolKind::Variant {
            symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Enum)
                .unwrap_or(symbol)
        } else {
            symbol
        };
        let reference = |symbol: &vvv_engine::Symbol| vvv_engine::SymbolRef {
            language: declaration.language.clone(),
            declaration: vvv_engine::SourceAnchor {
                path: path.clone(),
                content: vvv_engine::ContentId::of(&text),
                span: symbol.extent,
            },
            name_span: symbol.name_span,
            kind: symbol.kind,
        };
        let target = reference(declaration.symbol.as_ref().unwrap());
        let container = reference(container);
        let (ticket, query) = self.search.body.pending().unwrap();
        Event::DefinitionResolved {
            ticket,
            query,
            reply: Ok(vvv_engine::NavigationReply {
                snapshot: vvv_engine::ContentId::of(&text).into(),
                outcome: vvv_engine::NavigationOutcome::Resolved {
                    target,
                    evidence: vvv_engine::ResolutionEvidence {
                        semantic: None,
                        addresses: vec![],
                    },
                    preview: Box::new(vvv_engine::DefinitionPreview {
                        selection: declaration.symbol.as_ref().unwrap().name_span,
                        container,
                        declaration,
                        source: vvv_engine::File {
                            identifiers,
                            path,
                            text,
                            highlights,
                            symbols,
                        },
                        identifiers: vec![],
                    }),
                },
            }),
        }
    }

    fn definition_preview(&mut self, event: Event) {
        let reply = self.definition_reply(event.clone());
        self.on_event(event);
        self.on_event(reply);
    }
}

/// `searched()`, then `r` on the declaration and the judge's answer.
fn renaming() -> Model {
    let mut m = searched();
    let effects = m.update(Action::Rename);
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m
}

/// The plan a `Language → Lang` rename answers with: the judged occurrences
/// and one real diff per file the default selection edits.
fn rename_plan() -> Planned {
    let rename = fx::rename(1);
    Planned::Rename {
        declarations: rename.declarations,
        occurrences: rename.occurrences,
        files: rename_files(),
    }
}

/// The rename's files: real diffs of the lines the `✓` sites sit on.
fn rename_files() -> Vec<vvv_engine::protocol::FileChange> {
    let sources: [(&str, &[ChangedLine]); 3] = [
        (
            "src/lang/mod.rs",
            &[(
                64,
                "pub trait Language: Send + Sync {",
                "pub trait Lang: Send + Sync {",
            )],
        ),
        (
            "src/lib.rs",
            &[(
                26,
                "pub use lang::{Language, LanguageId};",
                "pub use lang::{Lang, LanguageId};",
            )],
        ),
        (
            "src/other.rs",
            &[(
                13,
                "    vvv::Language::default()",
                "    vvv::Lang::default()",
            )],
        ),
    ];
    sources
        .iter()
        .map(|(path, changes)| {
            let path = *path;
            let edits = fx::rename(1)
                .occurrences
                .iter()
                .filter(|o| {
                    o.m.path == std::path::Path::new(path)
                        && o.confidence == vvv_engine::Confidence::Resolved
                })
                .map(|o| vvv_engine::Edit::replace(o.m.span, "Lang"))
                .collect();
            rewrite_file(path, changes, edits)
        })
        .collect()
}

/// `searched()`, then `m` on the row and a destination that plans.
fn moving() -> Model {
    let mut m = searched();
    let effects = m.update(Action::MoveFile);
    let generation = generation_of(&effects);
    let mv = fx::move_file();
    m.on_event(Event::Planned {
        generation,
        planned: Planned::Move {
            intent: Intent::Move(mv.intent),
            respellings: mv.respellings,
            notices: mv.notices,
            files: mv.files,
        },
    });
    m
}

/// `searched()`, then `w`, a template, and the plan's diff.
fn rewriting() -> Model {
    let mut m = searched();
    m.update(Action::Rewrite);
    let effects = typed(&mut m, "Lang");
    let generation = generation_of(&effects);
    m.on_event(Event::Planned {
        generation,
        planned: Planned::Rewrite {
            files: rewrite_files(),
        },
    });
    m
}

/// A line a rewrite changes: its 1-based number, and the text before and after.
type ChangedLine<'a> = (usize, &'a str, &'a str);

/// The plan a `Language → Lang` rewrite makes: one `FileChange` per file, with
/// its edits and a real diff of the lines the rewrite touches.
fn rewrite_files() -> Vec<vvv_engine::protocol::FileChange> {
    let sources: [(&str, &[ChangedLine]); 3] = [
        (
            "src/lang/mod.rs",
            &[(
                64,
                "pub trait Language: Send + Sync {",
                "pub trait Lang: Send + Sync {",
            )],
        ),
        (
            "src/lang/registry.rs",
            &[(
                4,
                "use super::{Language, LanguageId};",
                "use super::{Lang, LanguageId};",
            )],
        ),
        (
            "src/lib.rs",
            &[
                (
                    26,
                    "pub use lang::{Language, LanguageId};",
                    "pub use lang::{Lang, LanguageId};",
                ),
                (41, "    Language::new()", "    Lang::new()"),
            ],
        ),
    ];
    sources
        .iter()
        .map(|(path, changes)| {
            let path = *path;
            let edits = fx::search()
                .matches
                .iter()
                .filter(|m| m.path == std::path::Path::new(path))
                .map(|m| vvv_engine::Edit::replace(m.span, "Lang"))
                .collect();
            rewrite_file(path, changes, edits)
        })
        .collect()
}

/// One file's change: its edits, and a diff of `changes` (`line`, before,
/// after) in a file of `// line k` filler.
fn rewrite_file(
    path: &str,
    changes: &[ChangedLine],
    edits: Vec<vvv_engine::Edit>,
) -> vvv_engine::protocol::FileChange {
    let last = changes.iter().map(|(l, ..)| *l).max().unwrap_or(0) + 2;
    let text = |side: usize| -> String {
        let mut out = (1..=last)
            .map(|k| {
                changes.iter().find(|(l, ..)| *l == k).map_or_else(
                    || format!("// line {k}"),
                    |(_, before, after)| {
                        if side == 0 {
                            (*before).to_owned()
                        } else {
                            (*after).to_owned()
                        }
                    },
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
        out
    };
    let path = vvv_engine::RelPath::from(path);
    vvv_engine::protocol::FileChange {
        diff: vvv_engine::protocol::Diff::between(&path, &path, &text(0), &text(1)),
        path,
        moved_to: None,
        edits,
    }
}

fn history_entry(id: u64) -> vvv_engine::HistoryEntry {
    vvv_engine::HistoryEntry {
        id,
        at: vvv_engine::protocol::vocabulary::Ago::now(),
        intent: fx::rename_intent("Config", "Settings"),
        files: 0,
        paths: Vec::new(),
        moves: Vec::new(),
    }
}

struct FrameFixture<'a> {
    model: &'a Model,
}
impl<'a> FrameFixture<'a> {
    fn new(model: &'a Model) -> Self {
        Self { model }
    }
    fn render(&self) -> String {
        self.render_size(90, 20)
    }
    fn render_size(&self, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                App::new(
                    self.model,
                    Painter::plain(),
                    vvv_engine::protocol::vocabulary::Ago::now(),
                )
                .render(f.area(), f.buffer_mut())
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ------------------------------------------------------------------ search

#[test]
fn typing_requests_a_search_with_a_fresh_generation_each_time() {
    let mut m = model();
    let first = typed(&mut m, "L");
    let second = typed(&mut m, "a");
    assert_eq!(generation_of(&first) + 1, generation_of(&second));
    assert!(m.status.busy);
}

#[test]
fn stale_search_results_are_ignored() {
    let mut m = model();
    let effects = typed(&mut m, "La");
    let generation = generation_of(&effects);
    m.on_event(Event::Searched {
        generation: generation - 1,
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert!(
        m.search.results.matches.is_empty(),
        "older generation dropped"
    );
    m.on_event(Event::Searched {
        generation,
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(m.search.results.matches.len(), 4);
    assert!(!m.status.busy);
}

#[test]
fn bad_filters_show_an_error_instead_of_searching() {
    let mut m = model();
    typed(&mut m, "s:nop");
    let effects = m.update(Action::Input('e'));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(&m.status.message, Some((Level::Error, _))));
}

#[test]
fn keys_depend_on_the_focused_panel() {
    let mut m = searched();
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert_eq!(
        m.action_for(key(KeyCode::Char('j'))),
        Some(Action::Input('j'))
    );
    assert_eq!(m.action_for(key(KeyCode::Tab)), Some(Action::FocusNext));
    assert_eq!(
        m.action_for(ctrl('r')),
        Some(Action::Refresh),
        "refresh is available from every search panel"
    );
    assert_eq!(
        m.action_for(ctrl('n')),
        Some(Action::File(1)),
        "switch files without leaving the query"
    );
    assert_eq!(m.action_for(ctrl('p')), Some(Action::File(-1)));
    assert_eq!(
        m.action_for(key(KeyCode::Char('?'))),
        Some(Action::Input('?')),
        "a printable key types, even first"
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('v'))),
        Some(Action::Input('v')),
        "the view toggle is a list key, not a query one"
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('2'))),
        Some(Action::Input('2')),
        "digits are text in an input"
    );

    m.update(Action::Enter);
    assert_eq!(m.search.focus, SearchPanel::Results);
    assert_eq!(m.action_for(key(KeyCode::Char('j'))), Some(Action::Move(1)));
    assert_eq!(m.action_for(key(KeyCode::Char('r'))), Some(Action::Rename));
    assert_eq!(
        m.action_for(key(KeyCode::Char('M'))),
        Some(Action::MoveSymbol)
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('4'))),
        Some(Action::FocusNth(4))
    );
    assert_eq!(m.action_for(key(KeyCode::Char('e'))), Some(Action::Edit));
    assert_eq!(m.action_for(key(KeyCode::Char('v'))), Some(Action::View));

    m.update(Action::FocusNth(4));
    assert_eq!(m.search.focus, SearchPanel::Context);
    assert_eq!(
        m.action_for(key(KeyCode::Char('j'))),
        Some(Action::Scroll(1))
    );
    m.update(Action::FocusNext);
    assert_eq!(m.search.focus, SearchPanel::Body);
    m.update(Action::FocusNext);
    assert_eq!(m.search.focus, SearchPanel::Query, "tab wraps");
}

#[test]
fn unfocused_lists_keep_selection_markers_without_the_focused_style() {
    use ratatui::style::Modifier;

    let m = searched();
    assert_eq!(
        m.search.focus,
        SearchPanel::Query,
        "the query starts focused"
    );
    let backend = TestBackend::new(90, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            App::new(
                &m,
                Painter::plain(),
                vvv_engine::protocol::vocabulary::Ago::now(),
            )
            .render(f.area(), f.buffer_mut())
        })
        .unwrap();
    // The results pane's cursor row, wherever the report's rows put it.
    let buffer = terminal.backend().buffer();
    let cursor = (0..buffer.area.height).any(|y| {
        buffer[(1, y)]
            .style()
            .add_modifier
            .contains(Modifier::REVERSED)
    });
    assert!(
        !cursor,
        "unfocused selections use a quiet style and retain their marker"
    );
}

#[test]
fn selection_colors_cover_wrapped_paths_and_full_rows_without_dimming_match_text() {
    use ratatui::style::{Color, Modifier};
    let mut m = searched();
    let path = "crates/vvv-engine/src/a/very/long/module/directory/engine.rs";
    m.search.results.replace(vec![
        fx::m(path, 0, 4, "Engine", "use Engine;"),
        fx::m(path, 9, 0, "Engine", "Engine::new()"),
        fx::m("src/other.rs", 20, 0, "Engine", "Engine::new()"),
    ]);
    m.search.selection_changed();
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    let active = Color::Rgb(32, 59, 70);
    let retained = Color::Rgb(23, 40, 47);
    for (focus, file_bg, match_bg) in [
        (2, active, retained),
        (3, retained, active),
        (4, retained, retained),
    ] {
        m.update(Action::FocusNth(focus));
        let frame = m.search_frame();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|f| App::new(&m, Painter::colored(), 0).render(f.area(), f.buffer_mut()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let files = &frame.lists[0];
        let wrapped = crate::render::Fit(path, files.area.width.saturating_sub(10) as usize)
            .wrapped()
            .len();
        assert!(wrapped > 1);
        for row in 0..wrapped {
            let y = files.content.y + row as u16;
            for x in files.content.x..files.content.right() {
                assert_eq!(buffer[(x, y)].bg, file_bg, "wrapped path row {row}, x {x}");
            }
            assert!(
                !buffer[(files.content.x + 2, y)]
                    .modifier
                    .contains(Modifier::DIM)
            );
        }
        assert_eq!(
            buffer[(files.content.x, files.content.y)].fg,
            if focus == 2 {
                Color::Rgb(123, 220, 199)
            } else {
                Color::Rgb(84, 179, 172)
            }
        );
        let matches = &frame.lists[1];
        let y = matches.content.y;
        for x in matches.content.x..matches.content.right() {
            assert_eq!(buffer[(x, y)].bg, match_bg, "match row x {x}");
        }
        let code = matches.content.x + 6;
        assert_eq!(buffer[(code, y)].symbol(), "u");
        assert!(!buffer[(code, y)].modifier.contains(Modifier::DIM));
        assert_eq!(buffer[(code + 4, y)].fg, Painter::colored().hit.fg.unwrap());
        assert_eq!(
            buffer[(matches.content.x, y)].fg,
            if focus == 3 {
                Color::Rgb(123, 220, 199)
            } else {
                Color::Rgb(84, 179, 172)
            }
        );
        assert_eq!(
            buffer[(matches.content.x, y + 1)].bg,
            Color::Reset,
            "unselected matches retain the terminal background"
        );
    }
}

#[test]
fn globals_outrank_an_overlay_and_a_mode_outranks_its_panels() {
    // A mode's own key beats the default its panel kind would answer.
    let mut m = searched();
    m.on_event(Event::History(vec![history_entry(1)]));
    m.update(Action::FocusNext);
    assert_eq!(
        m.action_for(key(KeyCode::Char('u'))),
        Some(Action::Undo),
        "the mode's `u` beats the text panel's scroll"
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('d'))),
        Some(Action::Scroll(20)),
        "the text default still answers what the mode does not"
    );

    // A confirmation captures the rest, but the globals are checked first.
    m.update(Action::Undo);
    assert!(matches!(m.overlay, Some(Overlay::Confirm(_))));
    assert_eq!(m.action_for(key(KeyCode::Char('n'))), Some(Action::Back));
    assert_eq!(m.action_for(ctrl('c')), Some(Action::Quit));
    assert_eq!(
        m.action_for(key(KeyCode::Tab)),
        Some(Action::Back),
        "the overlay's catch-all takes the rest"
    );

    // An identifier cannot hold `?`, so it opens the key list.
    let r = renaming();
    assert_eq!(r.action_for(key(KeyCode::Char('?'))), Some(Action::Help));
}

#[test]
fn a_condition_picks_between_rows_of_one_key() {
    let mut m = searched();
    assert_eq!(m.search.focus, SearchPanel::Query);
    // The query holds "Language", so `esc` clears.
    assert_eq!(m.action_for(key(KeyCode::Esc)), Some(Action::Clear));

    // Empty, `esc` quits; `?` types — the query has no help key.
    m.update(Action::Clear);
    assert!(m.search.query.is_empty());
    assert_eq!(m.action_for(key(KeyCode::Esc)), Some(Action::Quit));
    assert_eq!(
        m.action_for(key(KeyCode::Char('?'))),
        Some(Action::Input('?'))
    );
}

#[test]
fn moving_the_cursor_asks_for_the_row_file_once() {
    let mut m = searched();
    m.update(Action::Enter);
    // Row 0 is the declaration in src/lang/mod.rs, already previewed.
    assert!(m.update(Action::Move(0)).is_empty());
    let effects = m.update(Action::File(1));
    assert!(
        matches!(&effects[..], [Effect::Definition { .. }, Effect::Preview { path }] if path.ends_with("registry.rs")),
        "{effects:?}"
    );
    let effects = m.update(Action::File(1));
    assert!(
        matches!(&effects[..], [Effect::Definition { .. }, Effect::Preview { path }] if path.ends_with("lib.rs")),
        "a third file"
    );
    m.on_event(preview("src/lib.rs", &["a"]));
    assert!(
        matches!(
            m.update(Action::Move(1)).as_slice(),
            [Effect::Definition { .. }]
        ),
        "context file is already shown; definition belongs to the occurrence"
    );
}

#[test]
fn symbol_menu_writes_a_filter_into_the_query() {
    let mut m = searched();
    m.update(Action::OpenMenu(MenuTarget::Symbol));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!("{:?}", m.overlay)
    };
    assert_eq!(menu.cursor, 0, "no filter yet");
    m.update(Action::Move(3));
    let effects = m.update(Action::MenuChoose);
    assert!(m.overlay.is_none());
    assert_eq!(m.search.query.filter(Filter::Symbol), Some("struct"));
    assert!(matches!(effects.last(), Some(Effect::Search { .. })));
    m.update(Action::OpenMenu(MenuTarget::Symbol));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!()
    };
    assert_eq!(
        menu.current().unwrap().value.as_deref(),
        Some("struct"),
        "preselected"
    );
}

#[test]
fn edit_opens_the_editor_at_the_row() {
    let mut m = searched();
    m.update(Action::Enter);
    let effects = m.update(Action::Edit);
    assert!(
        matches!(&effects[..], [Effect::Edit { path, line }] if path.ends_with("mod.rs") && *line == 63),
        "{effects:?}"
    );
}

// ------------------------------------------------------------------ rename

#[test]
fn a_mode_is_drawn_only_once_its_first_plan_answers() {
    let mut m = searched();
    let effects = m.update(Action::Rename);
    assert!(matches!(
        &effects[..],
        [Effect::Plan {
            debounce: false,
            ..
        }]
    ));
    assert!(matches!(m.mode, Mode::Rename(_)), "keys already go to it");
    assert!(
        matches!(m.shown(), Mode::Search),
        "the screen waits for something to show"
    );
    let before = FrameFixture::new(&m).render();
    assert!(before.contains("results"), "{before}");
    assert!(!before.contains("unverified"));

    let m = renaming();
    assert!(!m.arriving);
    assert!(matches!(m.shown(), Mode::Rename(_)));

    let mut m = searched();
    m.update(Action::Rename);
    m.update(Action::Back);
    assert!(!m.arriving && matches!(m.mode, Mode::Search));
}

#[test]
fn rename_judges_with_the_name_unchanged_and_starts_where_judgment_is_needed() {
    let mut m = searched();
    let effects = m.update(Action::Rename);
    match &effects[..] {
        [
            Effect::Plan {
                intent: Intent::Rename(i),
                ..
            },
        ] => {
            assert_eq!(
                (i.references.name.as_str(), i.to.as_str()),
                ("Language", "Language")
            );
            assert_eq!(
                i.references.declared_in.as_deref(),
                Some(std::path::Path::new("src/lang/mod.rs"))
            );
        }
        other => panic!("{other:?}"),
    }
    let m = renaming();
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.focus, RenamePanel::Name);
    assert_eq!(
        r.list(),
        RenamePanel::Unsure,
        "the detail follows `?` first"
    );
    assert_eq!(
        r.ticks.len(),
        3,
        "the engine's default: what its plan edits — the re-export included"
    );
    assert!(!r.busy);
}

#[test]
fn rename_ticks_feed_the_commit_and_the_name_follows_typing() {
    let mut m = renaming();
    let effects = typed(&mut m, "LangX");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.name, "LangX");
    let unsure = r.current().unwrap();
    assert!(!r.is_ticked(unsure), "unsure rows start unticked here");

    m.update(Action::FocusNth(2));
    m.update(Action::Toggle);
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.ticked(vvv_engine::Confidence::Unresolved), 1);
    assert_eq!(
        r.cursor(RenamePanel::Unsure).unwrap().index,
        0,
        "toggle stays on the last row"
    );
    m.update(Action::ToggleAll);
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(
        r.ticked(vvv_engine::Confidence::Unresolved),
        0,
        "`a` on a fully ticked panel clears it"
    );
    m.update(Action::ToggleAll);
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.ticked(vvv_engine::Confidence::Unresolved), 1);
    let ticks = r.ticks.clone();

    let effects = m.update(Action::Enter);
    match &effects[..] {
        [
            Effect::Commit {
                intent: Intent::Rename(i),
            },
        ] => {
            assert_eq!(i.to, "LangX");
            assert_eq!(i.selection, Selection::Ids(ticks));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn rename_refuses_to_commit_without_a_new_name() {
    let mut m = renaming();
    assert!(m.update(Action::Enter).is_empty());
    assert!(matches!(&m.status.message, Some((Level::Error, _))));
}

#[test]
fn esc_returns_to_search_with_the_rows_intact() {
    let mut m = renaming();
    m.update(Action::Back);
    assert!(matches!(m.mode, Mode::Search));
    assert_eq!(m.search.results.matches.len(), 4);
    assert_eq!(m.search.query.text(), "Language");
}

// ------------------------------------------------------------------ move

#[test]
fn move_plans_as_the_destination_changes_and_commits_the_plan() {
    let mut m = searched();
    let effects = m.update(Action::MoveFile);
    let first = generation_of(&effects);
    assert!(matches!(
        &effects[..],
        [Effect::Plan {
            intent: Intent::Move(_),
            ..
        }]
    ));
    let effects = typed(&mut m, "x");
    assert_eq!(
        generation_of(&effects),
        first + 1,
        "every keystroke re-plans"
    );
    m.on_event(Event::PlanFailed {
        generation: first + 1,
        message: "src/lang/mod.rsx: not addressable".into(),
    });
    let Mode::Move(mv) = &m.mode else { panic!() };
    assert!(mv.plan.is_none());
    assert!(mv.error.as_deref().unwrap().contains("not addressable"));
    assert!(m.update(Action::Enter).is_empty(), "nothing to commit");

    let mut m = moving();
    let Mode::Move(mv) = &m.mode else { panic!() };
    let plan = mv.plan.as_ref().unwrap();
    assert_eq!(plan.respellings.len(), 2);
    assert_eq!(plan.structural.len(), 2, "the mod line and the moved file");
    assert_eq!(plan.notices.len(), 2);
    assert!(mv.error.is_none());
    let effects = m.update(Action::Enter);
    assert!(matches!(
        &effects[..],
        [Effect::Commit {
            intent: Intent::Move(_)
        }]
    ));
}

#[test]
fn move_symbol_needs_a_declaration_row() {
    let mut m = searched();
    m.update(Action::Enter);
    m.update(Action::File(1));
    assert!(m.update(Action::MoveSymbol).is_empty());
    assert!(matches!(&m.status.message, Some((Level::Error, _))));
    m.update(Action::File(-1));
    let effects = m.update(Action::MoveSymbol);
    assert!(
        effects.is_empty(),
        "no destination yet: nothing to plan ({effects:?})"
    );
    let Mode::Move(mv) = &m.mode else { panic!() };
    assert_eq!(mv.symbol.as_deref(), Some("Language"));
    assert_eq!(mv.focus, MovePanel::To);
}

// ------------------------------------------------------------------ rewrite

#[test]
fn rewrite_expands_the_template_live_and_commits_the_ticks() {
    let mut m = searched();
    let effects = m.update(Action::Rewrite);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Plan { .. })),
        "no template yet: nothing to plan"
    );
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    assert_eq!(rw.focus, RewritePanel::Template);
    assert_eq!(rw.ticks.len(), 4, "every match starts ticked");

    let mut m = rewriting();
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    assert_eq!(rw.changes.len(), 3, "the plan's files, each with its diff");

    m.update(Action::FocusNth(2));
    m.update(Action::Toggle);
    let effects = m.update(Action::Enter);
    match &effects[..] {
        [
            Effect::Commit {
                intent: Intent::Rewrite(i),
            },
        ] => {
            assert_eq!(i.template.to_string(), "Lang");
            assert!(matches!(&i.selection, Selection::Ids(ids) if ids.len() == 3));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_rewrite_detail_shows_the_hunk_holding_the_current_match() {
    let mut m = rewriting();
    m.update(Action::FocusNth(2));
    // The second `src/lib.rs` match (line 41) sits in its own hunk.
    m.update(Action::Move(3));
    let lines = FrameFixture::new(&m).render();
    assert!(lines.contains("src/lib.rs:41"), "{lines}");
    assert!(lines.contains("+    Lang::new()"), "{lines}");
    assert!(
        !lines.contains("+pub use lang::{Lang"),
        "the other hunk is off screen: {lines}"
    );
}

// ------------------------------------------------------------------ history

#[test]
fn history_undoes_the_newest_after_a_confirmation() {
    let mut m = searched();
    m.update(Action::Enter);
    assert!(matches!(&m.update(Action::History)[..], [Effect::History]));
    m.on_event(Event::History(vec![history_entry(1), history_entry(2)]));
    let Mode::History(h) = &m.mode else { panic!() };
    assert_eq!(h.cursor.index, 1, "the newest is under the cursor");
    m.update(Action::Move(-1));
    assert!(m.update(Action::Undo).is_empty());
    assert!(
        matches!(&m.status.message, Some((Level::Error, _))),
        "only the newest"
    );
    m.update(Action::Move(1));
    m.update(Action::Undo);
    assert!(matches!(m.overlay, Some(Overlay::Confirm(_))));
    let effects = m.update(Action::Enter);
    assert!(matches!(&effects[..], [Effect::Undo]));
    m.on_event(Event::Undone(history_entry(2)));
    assert!(matches!(m.mode, Mode::Search));
}

#[test]
fn an_apply_returns_to_search_and_searches_again() {
    let mut m = renaming();
    let effects = m.on_event(Event::Applied {
        id: 3,
        intent: fx::rename_intent("Config", "Settings"),
        report: Default::default(),
    });
    assert!(matches!(m.mode, Mode::Search));
    assert!(matches!(effects.last(), Some(Effect::Search { .. })));
    assert!(m.status.message.as_ref().unwrap().1.starts_with("✓ #3"));
}

/// A hand-built report, for the overlay snapshot.
fn report() -> vvv_engine::report::Document {
    use vvv_engine::protocol::display::{Line, Role};
    use vvv_engine::report::{Block, Document};
    let mut doc = Document::default();
    doc.block_body(Block::Title("rename Config → Settings".into()));
    doc.block_body(Block::Line(Line::single(Role::Path, "src/a.rs")));
    doc.block_note(Block::Summary(Line::single(Role::Plain, "✓ #3  2 files")));
    doc
}

// ------------------------------------------------------------------ snapshots

#[test]
fn snapshot_report_overlay() {
    let mut m = renaming();
    m.overlay = Some(Overlay::Report {
        report: Box::new(report()),
        cursor: 0,
    });
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn shared_move_and_rename_reports_have_no_cli_flag_advice() {
    use vvv_engine::report::{Block, Document, Note};

    for answer in [Answer::Rename(fx::rename(1)), Answer::Move(fx::move_file())] {
        let report = Document::of(&answer);
        assert!(
            report
                .parts()
                .1
                .iter()
                .all(|block| !matches!(block, Block::Note(Note::Hint(_))))
        );
    }
}

#[test]
fn snapshot_search() {
    let mut m = searched();
    m.update(Action::Enter);
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_search_detailed() {
    let mut m = searched();
    m.update(Action::Enter);
    m.view = ReportView::Detailed;
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn the_report_overlay_walks_its_source_rows() {
    use vvv_engine::report::{Block, Document};
    let mut doc = Document::default();
    doc.block_body(Block::Matches(fx::search().matches));
    let mut m = model();
    m.overlay = Some(Overlay::Report {
        report: Box::new(doc),
        cursor: 0,
    });
    let first = m.report_site();
    assert!(first.is_some(), "the cursor starts on a source row");
    m.update(Action::Move(1));
    let second = m.report_site();
    assert!(
        second.is_some() && second != first,
        "and moves to the next one"
    );
    m.update(Action::Move(-1));
    assert_eq!(m.report_site(), first, "and back");
}

#[test]
fn v_toggles_the_report_view() {
    let mut m = searched();
    assert_eq!(m.view, ReportView::Compact, "compact by default");
    m.update(Action::View);
    assert_eq!(m.view, ReportView::Detailed);
    m.update(Action::View);
    assert_eq!(m.view, ReportView::Compact);
}

#[test]
fn snapshot_search_use_row_with_context() {
    let mut m = searched();
    m.update(Action::Enter);
    m.update(Action::Move(2));
    let lines = numbered(
        45,
        &[
            (26, "pub use lang::{Language, LanguageId};"),
            (41, "    Language::new()"),
        ],
    );
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lib.rs", &refs));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

/// `searched()`, then `Enter` on the declaration and the engine's answer:
/// the hub narrows to the declaration's judged references.
fn anchored() -> Model {
    let mut m = searched();
    m.update(Action::Enter); // query → results
    let effects = m.update(Action::Enter); // results → enter the scope
    let generation = match effects.last() {
        Some(Effect::Query { generation, .. }) => *generation,
        other => panic!("expected a read request, got {other:?}"),
    };
    let mut references = fx::references();
    references.declarations[0] = m.search.results.matches[0].clone();
    references.occurrences[0].m = references.declarations[0].clone();
    m.on_event(Event::Answered {
        generation,
        answer: Box::new(Answer::References(references)),
    });
    m
}

#[test]
fn enter_anchors_the_declaration_under_the_cursor() {
    let mut m = searched();
    m.update(Action::Enter);
    let effects = m.update(Action::Enter);
    let Some(Effect::Query {
        generation,
        request: Request::References(query),
    }) = effects.last()
    else {
        panic!("expected a references request: {effects:?}");
    };
    assert_eq!(query.name, "Language");
    assert_eq!(query.symbol, Some(SymbolKind::Trait));
    assert!(query.declared_in.is_some(), "the file disambiguates");
    assert!(
        !m.search.results.is_anchored(),
        "the display waits for the answer, so it never blanks"
    );
    m.on_event(Event::Answered {
        generation: *generation,
        answer: Box::new(Answer::References(fx::references())),
    });
    assert!(m.search.results.is_anchored());
}

#[test]
fn esc_leaves_the_scope_and_keeps_the_search() {
    let mut m = anchored();
    assert_eq!(m.search.results.len(), 4);
    m.update(Action::Back); // Matches → Files
    m.update(Action::Back); // Files → Query
    m.update(Action::Back); // Leave the relation
    assert!(!m.search.results.is_anchored());
    assert_eq!(m.search.results.matches.len(), 4, "the search survives");
}

#[test]
fn anchored_rename_targets_the_subject_from_any_row() {
    let mut m = anchored();
    m.update(Action::Move(2)); // onto a use, not the declaration
    let effects = m.update(Action::Rename);
    let Mode::Rename(r) = &m.mode else {
        panic!("expected rename mode");
    };
    assert_eq!(r.target.name, "Language");
    assert_eq!(r.target.symbol, Some(SymbolKind::Trait));
    assert!(matches!(effects.last(), Some(Effect::Plan { .. })));
}

#[test]
fn snapshot_search_anchored() {
    let m = anchored();
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_search_anchored_detailed() {
    let mut m = anchored();
    m.view = ReportView::Detailed;
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn the_relation_menu_narrows_references_and_switches_to_impact() {
    let mut m = anchored();
    // The verdict filter is local: the references are already in hand.
    m.update(Action::OpenMenu(MenuTarget::Relation));
    assert!(matches!(m.overlay, Some(Overlay::Menu(_))));
    m.update(Action::Move(2)); // references → ? unverified
    m.update(Action::MenuChoose);
    assert_eq!(m.search.results.relation, Relation::Unresolved);
    assert_eq!(m.search.results.len(), 1);

    // Impact asks the engine, and the view switches only when it answers.
    m.update(Action::OpenMenu(MenuTarget::Relation));
    m.update(Action::Move(2)); // ? unverified → impact
    let effects = m.update(Action::MenuChoose);
    let Some(Effect::Query {
        generation,
        request: Request::Impact(_),
    }) = effects.last()
    else {
        panic!("expected an impact request: {effects:?}");
    };
    assert_eq!(
        m.search.results.relation,
        Relation::Unresolved,
        "the references view stays until the impact answers"
    );
    m.on_event(Event::Answered {
        generation: *generation,
        answer: Box::new(Answer::Impact(fx::impact())),
    });
    assert_eq!(m.search.results.relation, Relation::Impact);
    assert_eq!(m.search.results.len(), 3);
    assert!(m.search.results.current_consumer().is_some());
}

#[test]
fn snapshot_search_anchored_unresolved() {
    let mut m = anchored();
    m.search.results.set_relation(Relation::Unresolved);
    m.search.selection_changed();
    let (ticket, query) = m.search.body.pending().unwrap();
    m.on_event(Event::DefinitionResolved {
        ticket,
        query,
        reply: Ok(vvv_engine::NavigationReply {
            snapshot: vvv_engine::ContentId::of("").into(),
            outcome: vvv_engine::NavigationOutcome::Unavailable {
                reason: vvv_engine::UnavailableReason::UnsupportedContext,
            },
        }),
    });
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn the_relation_menu_asks_for_definition_and_deps() {
    let mut m = anchored();

    // Definition is index 5; the view waits for the explain answer.
    m.update(Action::OpenMenu(MenuTarget::Relation));
    m.update(Action::Move(5));
    let effects = m.update(Action::MenuChoose);
    let Some(Effect::Query {
        generation,
        request: Request::Explain(_),
    }) = effects.last()
    else {
        panic!("expected an explain request: {effects:?}");
    };
    assert_eq!(m.search.results.relation, Relation::References);
    m.on_event(Event::Answered {
        generation: *generation,
        answer: Box::new(Answer::Explain(fx::explanation())),
    });
    assert_eq!(m.search.results.relation, Relation::Definition);

    // Deps is index 6, one below the selected definition.
    m.update(Action::OpenMenu(MenuTarget::Relation));
    m.update(Action::Move(1));
    let effects = m.update(Action::MenuChoose);
    let Some(Effect::Query {
        generation,
        request: Request::Deps(_),
    }) = effects.last()
    else {
        panic!("expected a deps request: {effects:?}");
    };
    m.on_event(Event::Answered {
        generation: *generation,
        answer: Box::new(Answer::Deps(fx::deps())),
    });
    assert_eq!(m.search.results.relation, Relation::Deps);
}

#[test]
fn o_resolves_the_selected_occurrence_without_moving_before_success() {
    let mut m = searched();
    m.update(Action::Enter); // into the results
    m.update(Action::File(2));
    m.update(Action::Move(1)); // onto a use
    let effects = m.update(Action::Follow);
    assert!(matches!(effects.as_slice(), [Effect::Follow { .. }]));
    assert_eq!(
        m.search.results.cursor.index, 3,
        "keep the origin until success"
    );
}

#[test]
fn snapshot_search_definition() {
    let mut m = anchored();
    m.search.results.show_definition(fx::explanation());
    m.search.selection_changed();
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_search_deps() {
    let mut m = anchored();
    m.search.results.show_deps(fx::deps());
    m.search.selection_changed();
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn rewrite_stays_query_scoped_under_a_relation_filter() {
    let mut m = anchored();
    m.search.results.set_relation(Relation::Unresolved);
    m.search.selection_changed(); // one row shown
    m.update(Action::Rewrite);
    let Mode::Rewrite(rw) = &m.mode else {
        panic!("expected rewrite mode");
    };
    assert_eq!(
        rw.matches.len(),
        fx::search().matches.len(),
        "the query's matches, not the filtered rows"
    );
    assert_eq!(rw.ticks.len(), rw.matches.len());
}

#[test]
fn snapshot_search_impact() {
    let mut m = anchored();
    m.search.results.set_relation(Relation::Impact);
    m.search.selection_changed();
    m.on_event(Event::Answered {
        generation: m.generation,
        answer: Box::new(Answer::Impact(fx::impact())),
    });
    let lines = numbered(5, &[(1, "use super::{Language, LanguageId};")]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lang/registry.rs", &refs));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_empty_search() {
    insta::assert_snapshot!(FrameFixture::new(&model()).render());
}

#[test]
fn snapshot_rename() {
    let mut m = renaming();
    let effects = typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    // The `✓` re-export in src/other.rs: the detail draws the plan's hunk.
    m.update(Action::FocusNth(3));
    m.update(Action::Move(2));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn the_rename_detail_shows_the_hunk_holding_the_current_site() {
    let mut m = renaming();
    let effects = typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m.update(Action::FocusNth(3));
    m.update(Action::Move(2));
    let lines = FrameFixture::new(&m).render();
    assert!(lines.contains("src/other.rs:13"), "{lines}");
    assert!(lines.contains("vvv::Lang::default()"), "{lines}");
    assert!(
        lines.contains("@@ "),
        "the file's hunks, not a before/after pair: {lines}"
    );
    assert!(!lines.contains("before"), "{lines}");
}

#[test]
fn snapshot_rename_detailed() {
    let mut m = renaming();
    m.view = ReportView::Detailed;
    let effects = typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m.update(Action::FocusNth(3));
    m.update(Action::Move(2));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_move() {
    let mut m = moving();
    m.update(Action::FocusNth(2));
    let lines = numbered(5, &[(1, "use crate::util::parse::X;")]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lib.rs", &refs));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_move_detailed() {
    let mut m = moving();
    m.view = ReportView::Detailed;
    m.update(Action::FocusNth(2));
    let lines = numbered(5, &[(1, "use crate::util::parse::X;")]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lib.rs", &refs));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_move_structural_row_shows_the_hunk() {
    let mut m = moving();
    m.update(Action::FocusNth(3));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_rewrite() {
    let mut m = rewriting();
    m.update(Action::FocusNth(2));
    m.update(Action::Move(1));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_rewrite_detailed() {
    let mut m = rewriting();
    m.view = ReportView::Detailed;
    m.update(Action::FocusNth(2));
    m.update(Action::Move(1));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn snapshot_history_and_confirm() {
    let mut m = searched();
    m.on_event(Event::History(vec![history_entry(1), history_entry(2)]));
    let history = FrameFixture::new(&m).render();
    m.update(Action::Undo);
    let confirm = FrameFixture::new(&m).render();
    insta::assert_snapshot!(format!("{history}\n\n=== confirm ===\n{confirm}"));
}

#[test]
fn snapshot_help() {
    let mut m = searched();
    m.update(Action::Help);
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn report_overlay_opens_declaration_sites_and_skips_suggested_imports() {
    use vvv_engine::report::Document;
    use vvv_engine::{Locations, Position, Site};

    let first = fx::search().matches[0].clone();
    let mut second = first.clone();
    second.path = "another.rs".into();
    second.start = Position::new(4, 0);
    let report = Document::of(&Answer::Where(Locations {
        name: "Language".into(),
        sites: vec![
            Site {
                declaration: first.clone(),
                address: None,
                import: Some("use crate::Language;".into()),
            },
            Site {
                declaration: second.clone(),
                address: None,
                import: None,
            },
        ],
    }));
    let mut model = model();
    model.overlay = Some(Overlay::Report {
        report: Box::new(report),
        cursor: 0,
    });
    assert_eq!(
        model.report_site(),
        Some((first.path.clone(), first.start.line))
    );
    assert!(FrameFixture::new(&model).render().contains("Language"));
    model.update(Action::Move(1));
    assert_eq!(
        model.report_site(),
        Some((second.path.clone(), second.start.line))
    );
    assert!(
        matches!(model.update(Action::Edit).as_slice(), [Effect::Edit { path, line }] if path == &second.path && *line == second.start.line)
    );
    model.update(Action::Move(1));
    assert_eq!(
        model.report_site(),
        Some((second.path, second.start.line)),
        "the summary is not a source row"
    );
}

#[test]
fn body_focus_scroll_and_editor_are_independent_of_context() {
    let mut m = searched();
    m.update(Action::Move(1)); // a use in another file
    m.update(Action::FocusNth(5));
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert_eq!(
        m.action_for(key(KeyCode::Char('j'))),
        Some(Action::Scroll(1))
    );
    m.on_key(key(KeyCode::Char('j')));
    assert_eq!(m.search.body.scroll, 1);
    assert_eq!(m.search.preview_scroll, None);
    assert_eq!(m.search.results.cursor.index, 1);
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 63 }]
        if path.as_path() == std::path::Path::new("src/lang/mod.rs"))
    );
    m.update(Action::Bottom);
    assert_eq!(m.search.body.scroll, 3);
    m.update(Action::Top);
    assert_eq!(m.search.body.scroll, 0);
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::Move(1));
    assert_eq!(m.search.body.scroll, 0);
}

#[test]
fn body_ignores_unrelated_preview_answers_and_unresolved_references() {
    let mut m = searched();
    let before = m.search.body.preview.clone();
    m.update(Action::Move(1));
    m.on_event(preview("src/lang/registry.rs", &["use Language;"]));
    assert_eq!(m.search.body.preview, before);
    m.on_event(preview("unrelated.rs", &["stale"]));
    assert_eq!(m.search.body.preview, before);
    assert_eq!(
        m.search.preview.as_ref().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );

    let mut m = anchored();
    m.update(Action::Move(1));
    assert!(m.search.body.pending().is_some());
    m.update(Action::Move(2));
    assert!(m.search.body.pending().is_some());
    assert!(
        m.search.results.has_body(),
        "definition focus remains available"
    );
}

#[test]
fn body_ambiguity_and_empty_results_do_not_leave_invisible_focus() {
    let mut m = searched();
    let mut other = m.search.results.matches[0].clone();
    other.path = "other.rs".into();
    other.id = vvv_engine::MatchId::derive(&other.path, other.span, &other.text);
    let mut matches = m.search.results.matches.to_vec();
    matches.push(other);
    m.search.results.replace(matches);
    m.update(Action::Move(1));
    assert!(m.search.body.pending().is_some());
    m.update(Action::FocusNth(5));
    m.search.results.replace(vec![]);
    m.search.selection_changed();
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusNth(5));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::PreviewTab);
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusPrev);
    m.update(Action::FocusPrev);
    m.update(Action::FocusPrev);
    assert_eq!(m.search.focus, SearchPanel::Context);
}

#[test]
fn preview_borders_stay_quiet_and_focus_keys_and_p_select_the_destination() {
    let mut m = searched();
    m.update(Action::Move(1));
    m.search.focus = SearchPanel::Results;
    let wide = FrameFixture::new(&m).render_size(120, 30);
    assert!(!wide.contains("4 source"));
    assert!(!wide.contains("5 definition"));
    assert!(!wide.contains("p source") && !wide.contains("p definition"));
    m.on_key(key(KeyCode::Char('4')));
    assert_eq!(m.search.focus, SearchPanel::Context);
    m.on_key(key(KeyCode::Char('p')));
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert!(FrameFixture::new(&m).render().contains("definition"));
    m.on_key(key(KeyCode::Char('p')));
    assert_eq!(m.search.focus, SearchPanel::Context);
    assert!(FrameFixture::new(&m).render().contains("source"));
    m.on_key(key(KeyCode::Char('5')));
    assert_eq!(m.search.focus, SearchPanel::Body);
}

#[test]
fn snapshot_body_focused() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn single_preview_choice_survives_file_match_filter_query_and_resize_navigation() {
    let mut m = searched();
    m.on_event(Event::Viewport {
        width: 90,
        height: 24,
    });
    m.update(Action::FocusNth(3));
    assert!(!m.search.definition_tab, "source is the default preview");
    let preview_panels = |model: &Model| {
        model
            .search_frame()
            .panels
            .into_iter()
            .filter(|(panel, _)| matches!(panel, SearchPanel::Context | SearchPanel::Body))
            .collect::<Vec<_>>()
    };
    let source = preview_panels(&m);
    assert!(!source[0].1.is_empty());
    assert!(source[1].1.is_empty());
    m.on_key(key(KeyCode::Char('5')));
    m.on_key(key(KeyCode::Esc));
    let definition = preview_panels(&m);
    assert!(definition[0].1.is_empty());
    assert_eq!(definition[1].1, source[0].1);
    for by in [1, 1, -1, -1] {
        m.update(Action::File(by));
        assert_eq!(preview_panels(&m), definition);
        assert!(m.search.definition_tab);
    }
    m.on_key(key(KeyCode::Char('F')));
    typed(&mut m, "lib");
    m.on_key(key(KeyCode::Enter));
    m.update(Action::FocusNth(3));
    m.update(Action::Move(1)); // import -> use in the same file
    assert_eq!(
        m.search.results.current().unwrap().role,
        vvv_engine::Role::Use
    );
    assert_eq!(preview_panels(&m), definition);
    m.update(Action::ClearFileFilter);
    m.update(Action::FocusNth(1));
    m.on_key(ctrl('p'));
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert_eq!(preview_panels(&m), definition);
    m.on_event(Event::Searched {
        generation: m.generation,
        matches: m.search.results.matches.to_vec(),
        skipped: vec![],
    });
    assert_eq!(preview_panels(&m), definition);
    insta::assert_snapshot!("narrow_definition_choice", FrameFixture::new(&m).render());
    m.update(Action::FocusNth(3));
    m.on_key(key(KeyCode::Char('4')));
    m.on_key(key(KeyCode::Esc));
    m.update(Action::File(-1));
    assert_eq!(preview_panels(&m), source);
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    let wide = preview_panels(&m);
    assert!(!wide[0].1.is_empty() && !wide[1].1.is_empty());
    insta::assert_snapshot!(
        "wide_declaration_previews",
        FrameFixture::new(&m).render_size(120, 30)
    );
    m.on_event(Event::Viewport {
        width: 90,
        height: 24,
    });
    assert_eq!(preview_panels(&m), source);
    insta::assert_snapshot!("narrow_source_choice", FrameFixture::new(&m).render());
    m.on_key(key(KeyCode::Char('p')));
    m.on_key(key(KeyCode::Esc));
    assert_eq!(preview_panels(&m), definition, "p also retains its choice");
    m.on_event(Event::Searched {
        generation: m.generation,
        matches: vec![],
        skipped: vec![],
    });
    assert_eq!(
        preview_panels(&m),
        definition,
        "empty results retain the choice"
    );
}

#[test]
fn source_highlight_windows_preserve_multiline_overlap_priority_and_unordered_input() {
    use vvv_engine::{Highlight, HighlightKind, Span};
    let highlights = vec![
        Highlight {
            span: Span::new(8, 12),
            kind: HighlightKind::Type,
        },
        Highlight {
            span: Span::new(0, 16),
            kind: HighlightKind::Keyword,
        },
        Highlight {
            span: Span::new(2, 6),
            kind: HighlightKind::Type,
        },
        Highlight {
            span: Span::new(14, 17),
            kind: HighlightKind::Keyword,
        },
    ];
    let preview = crate::model::FilePreview::new(vvv_engine::File {
        path: "source.rs".into(),
        text: "one\ntwo\nthree\nfour".into(),
        highlights: highlights.clone(),
        symbols: vec![],
        identifiers: vec![],
    });
    for start in 0..18 {
        for end in start..18 {
            let expected: Vec<_> = highlights
                .iter()
                .filter(|h| start < end && h.span.start < end && h.span.end > start)
                .collect();
            assert_eq!(
                preview.highlights_in(start, end),
                expected,
                "window {start}..{end}"
            );
        }
    }
}

#[test]
fn refresh_and_changed_search_contents_reload_the_selected_source_without_blanking() {
    let mut m = searched();
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    let old = m.search.preview.clone().unwrap();
    m.on_event(Event::SourcesChanged);
    insta::assert_snapshot!(
        "source_refresh_needed",
        FrameFixture::new(&m).render_size(120, 30)
    );
    let effects = m.on_key(ctrl('r'));
    let generation = generation_of(&effects);
    let new_text = "pub trait ChangedLanguage {}";
    let mut matches = fx::search().matches;
    matches[0].content = Some(vvv_engine::ContentId::of(new_text));
    let effects = m.on_event(Event::Searched {
        generation,
        matches,
        skipped: vec![],
    });
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Preview { path } if path == &old.path))
    );
    assert_eq!(m.search.preview.as_ref(), Some(&old));
    assert!(
        m.search.displayed_source().is_some(),
        "refresh retains the previous source until the reply"
    );
    insta::assert_snapshot!(
        "source_refresh_pending",
        FrameFixture::new(&m).render_size(120, 30)
    );
    m.on_event(preview(old.path.as_str(), &[new_text]));
    assert_eq!(m.search.preview.as_ref().unwrap().text(), new_text);
    assert!(!m.search.preview_dirty);
    // A subsequent ordinary search can discover an external edit too.
    let mut matches = fx::search().matches;
    matches[0].content = Some(vvv_engine::ContentId::of("a newer source version"));
    let effects = m.on_event(Event::Searched {
        generation: m.generation,
        matches,
        skipped: vec![],
    });
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Preview { path } if path == &old.path))
    );
}

#[test]
fn source_preview_keeps_its_displayed_file_until_the_latest_selection_arrives() {
    let mut m = model();
    typed(&mut m, "Engine");
    let a = numbered(60, &[(21, "Engine::from_a()")]);
    let b = numbered(60, &[(26, "Engine::from_b()")]);
    let c = numbered(60, &[(36, "Engine::from_c()")]);
    let a_text = a.join("\n");
    let matches = vec![
        fx::at(
            fx::m("a.rs", 20, 0, "Engine", "Engine::from_a()"),
            a_text.find("Engine").unwrap(),
        ),
        fx::at(
            fx::m("b.rs", 25, 0, "Engine", "Engine::from_b()"),
            b.join("\n").find("Engine").unwrap(),
        ),
        fx::at(
            fx::m("c.rs", 35, 0, "Engine", "Engine::from_c()"),
            c.join("\n").find("Engine").unwrap(),
        ),
    ];
    m.on_event(Event::Searched {
        generation: m.generation,
        matches,
        skipped: vec![],
    });
    let reply = |path: &str, lines: &[String]| {
        preview(path, &lines.iter().map(String::as_str).collect::<Vec<_>>())
    };
    m.on_event(reply("a.rs", &a));
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    m.search.preview_scroll = Some(18);
    let anchor = m.search.displayed_source().unwrap();
    let before = FrameFixture::new(&m).render_size(120, 30);
    let effects = m.on_key(ctrl('n'));
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Preview { path } if path.as_path() == std::path::Path::new("b.rs"))
    ));
    let pending = FrameFixture::new(&m).render_size(120, 30);
    let source = m
        .search_frame()
        .panels
        .into_iter()
        .find(|(panel, _)| *panel == SearchPanel::Context)
        .unwrap()
        .1;
    let source_rows = |frame: &str| {
        frame
            .lines()
            .skip(source.y as usize)
            .take(source.height.saturating_sub(1) as usize)
            .map(|line| {
                line.chars()
                    .skip(source.x as usize)
                    .take(source.width as usize)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        source_rows(&before),
        source_rows(&pending),
        "the source text and its title remain steady while another file loads"
    );
    assert_eq!(m.search.displayed_source(), Some(anchor.clone()));
    assert_eq!(m.search.preview_scroll, Some(18));
    assert!(pending.contains("updating"));
    insta::assert_snapshot!("source_preview_pending", pending);
    m.on_key(ctrl('n'));
    m.on_event(reply("b.rs", &b));
    assert_eq!(m.search.displayed_source(), Some(anchor));
    assert_eq!(
        m.search.preview.as_ref().unwrap().path.as_path(),
        std::path::Path::new("a.rs")
    );
    m.update(Action::FocusNth(4));
    assert_eq!(
        m.search.site().unwrap().0.as_path(),
        std::path::Path::new("a.rs"),
        "source actions use the displayed file during loading"
    );
    m.on_event(reply("c.rs", &c));
    assert_eq!(
        m.search.displayed_source().unwrap().path.as_path(),
        std::path::Path::new("c.rs")
    );
    assert_eq!(m.search.preview_scroll, None);
    let ready = FrameFixture::new(&m).render_size(120, 30);
    assert!(ready.contains("source · c.rs:36"));
    assert!(ready.contains("Engine::from_c()"));
    insta::assert_snapshot!("source_preview_ready", ready);
    m.on_event(reply("a.rs", &a));
    assert_eq!(
        FrameFixture::new(&m).render_size(120, 30),
        ready,
        "late replies cannot replace the latest file"
    );
}

#[test]
fn body_tracks_declarations_and_requests_both_files_when_needed() {
    let mut m = searched();
    m.search.preview = None;
    m.search.body.clear();
    m.update(Action::Move(1));
    let effects = m.search.preview_effect();
    assert_eq!(effects.len(), 2);
    assert!(matches!(&effects[0], Effect::Definition { .. }));
    assert!(matches!(&effects[1], Effect::Preview { path }
        if path.as_path() == std::path::Path::new("src/lang/registry.rs")));
    m.update(Action::Move(-1));
    assert_eq!(
        m.search.preview_effect().len(),
        2,
        "context and coherent definition requests"
    );

    let text = "struct Other {\n    value: usize,\n}\nfn outside() {}";
    let mut declaration = fx::decl("other.rs", 0, SymbolKind::Struct, "Other", "struct Other {");
    declaration.symbol.as_mut().unwrap().span =
        vvv_engine::Span::new(0, text.find("\nfn").unwrap());
    let mut matches = m.search.results.matches.to_vec();
    matches.push(declaration);
    m.search.results.replace(matches);
    m.update(Action::Top);
    assert!(
        m.search
            .body
            .lines(m.search.results.current().unwrap())
            .is_none()
    );
    m.definition_preview(Event::Previewed {
        identifiers: vec![],
        symbols: vec![],
        path: "other.rs".into(),
        text: text.into(),
        highlights: vec![],
    });
    assert_eq!(
        m.search.body.lines(m.search.results.current().unwrap()),
        Some(0..3)
    );
    m.update(Action::FocusNth(5));
    let frame = FrameFixture::new(&m).render();
    let body_rows = frame
        .lines()
        .skip_while(|line| !line.contains("definition"))
        .take(7)
        .map(|line| line.chars().skip(45).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body_rows.contains("value: usize"));
    assert!(!body_rows.contains("outside"));
}

#[test]
fn body_renders_at_small_terminal_sizes() {
    let m = searched();
    for (width, height) in [(1, 1), (20, 6), (40, 10), (90, 20)] {
        FrameFixture::new(&m).render_size(width, height);
    }
}

#[test]
fn snapshot_body_scrolled() {
    let mut m = searched();
    let text = format!(
        "struct Large {{\n{}\n}}\nfn outside() {{}}",
        (0..30)
            .map(|n| format!("    field_{n}: usize,"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut declaration = fx::decl("large.rs", 0, SymbolKind::Struct, "Large", "struct Large {");
    declaration.symbol.as_mut().unwrap().span =
        vvv_engine::Span::new(0, text.find("\nfn").unwrap());
    m.search.results.replace(vec![declaration]);
    m.search.selection_changed();
    m.definition_preview(Event::Previewed {
        identifiers: vec![],
        symbols: vec![],
        path: "large.rs".into(),
        text,
        highlights: vec![],
    });
    m.update(Action::FocusNth(5));
    m.on_key(key(KeyCode::PageDown));
    assert_eq!(m.search.body.scroll, 20);
    assert_eq!(m.search.results.cursor.index, 0);
    insta::assert_snapshot!(FrameFixture::new(&m).render());
    m.update(Action::Bottom);
    assert_eq!(m.search.body.scroll, 31);
    m.on_key(key(KeyCode::Up));
    assert_eq!(m.search.body.scroll, 30);
    m.on_key(key(KeyCode::PageUp));
    assert_eq!(m.search.body.scroll, 10);
}

#[test]
fn definition_text_keeps_its_inset_across_loading_and_redraws() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    let source = m.search.body.preview.clone().unwrap();
    m.search.body.clear();
    let event = Event::Previewed {
        identifiers: vec![],
        symbols: vec![],
        path: source.path.clone(),
        text: source.text().to_owned(),
        highlights: source.highlights.clone(),
    };
    let event = m.definition_reply(event);
    let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
    let mut draw = |model: &Model| {
        terminal
            .draw(|frame| {
                App::new(model, Painter::colored(), 0).render(frame.area(), frame.buffer_mut());
            })
            .unwrap();
        terminal.backend().buffer().clone()
    };
    let loading = draw(&m);
    assert_eq!(loading[(47, 4)].symbol(), " ");
    m.on_event(event.clone());
    let loaded = draw(&m);
    assert_eq!(loaded[(46, 4)].symbol(), " ");
    assert_eq!(loaded[(47, 4)].symbol(), "p");
    assert_eq!(
        loaded[(51, 5)].symbol(),
        "f",
        "source indentation is preserved"
    );
    assert_eq!(loaded[(46, 3)], loading[(46, 3)], "the border stays put");
    m.on_event(event);
    assert_eq!(
        draw(&m),
        loaded,
        "a repeated file response cannot shift the text"
    );
    m.update(Action::FocusNth(5));
    m.on_key(key(KeyCode::Down));
    let scrolled = draw(&m);
    assert_eq!(scrolled[(51, 4)].symbol(), "f");
    assert_eq!(scrolled[(46, 4)].symbol(), " ");
}

#[test]
fn nested_definitions_keep_the_same_alignment_when_loaded_and_scrolled() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    for indent in ["", "    ", "        ", "\t", "\t    "] {
        let text =
            format!("mod outer {{\n{indent}fn nested() {{\n{indent}    nested();\n{indent}}}");
        let start = text.find("fn nested").unwrap();
        let mut declaration = fx::decl(
            "nested.rs",
            1,
            SymbolKind::Function,
            "nested",
            "fn nested() {",
        );
        declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(start, text.len());
        m.search.results.replace(vec![declaration]);
        m.search.selection_changed();
        m.search.body.clear();
        let loading = FrameFixture::new(&m).render();
        assert!(
            loading
                .lines()
                .nth(4)
                .unwrap()
                .chars()
                .skip(45)
                .all(|c| c == '│' || c == ' ')
        );
        assert!(!loading.contains("Loading"));
        m.definition_preview(Event::Previewed {
            identifiers: vec![],
            symbols: vec![],
            path: "nested.rs".into(),
            text: format!("{text}\n}}"),
            highlights: vec![vvv_engine::Highlight {
                span: vvv_engine::Span::new(start, start + 2),
                kind: vvv_engine::HighlightKind::Keyword,
            }],
        });
        let loaded = FrameFixture::new(&m).render();
        assert!(
            loaded
                .lines()
                .nth(4)
                .unwrap()
                .chars()
                .skip(45)
                .collect::<String>()
                .starts_with("│ fn nested() {"),
            "indent {indent:?}: {loaded}"
        );
        assert!(
            loaded
                .lines()
                .nth(5)
                .unwrap()
                .chars()
                .skip(45)
                .collect::<String>()
                .starts_with("│     nested();")
        );
        assert!(
            loaded
                .lines()
                .nth(6)
                .unwrap()
                .chars()
                .skip(45)
                .collect::<String>()
                .starts_with("│ }")
        );
        if indent == "        " {
            insta::assert_snapshot!("definition_nested", loaded);
        }
        m.update(Action::FocusNth(5));
        m.update(Action::Scroll(1));
        let scrolled = FrameFixture::new(&m).render();
        assert!(
            scrolled
                .lines()
                .nth(4)
                .unwrap()
                .chars()
                .skip(45)
                .collect::<String>()
                .starts_with("│     nested();")
        );
    }
}

#[test]
fn enum_variant_shows_its_whole_enum_when_its_file_arrives() {
    let mut m = searched();
    let text = "enum Error {\n    Io,\n    #[error(transparent)]\n    Engine(#[from] vvv_engine::EngineError),\n    Editor,\n}";
    let start = text.find("Engine(").unwrap();
    let end = start + text[start..].find(",\n").unwrap();
    let mut declaration = fx::decl(
        "error.rs",
        3,
        SymbolKind::Variant,
        "Engine",
        "    Engine(#[from] vvv_engine::EngineError),",
    );
    declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(start, end);
    declaration.symbol.as_mut().unwrap().name_span = vvv_engine::Span::new(start, start + 6);
    let parent = vvv_engine::Symbol::plain(
        SymbolKind::Enum,
        "Error",
        vvv_engine::Span::new(5, 10),
        vvv_engine::Span::new(0, text.len()),
    );
    let variant = declaration.symbol.clone().unwrap();
    m.search.results.replace(vec![declaration]);
    m.search.selection_changed();
    m.search.body.clear();
    m.update(Action::FocusNth(5));
    m.update(Action::FocusNth(1));
    let loading = FrameFixture::new(&m).render();
    assert!(
        loading
            .lines()
            .nth(4)
            .unwrap()
            .chars()
            .skip(45)
            .all(|c| c == '│' || c == ' ')
    );
    assert!(!loading.contains("Loading"));
    let event = Event::Previewed {
        identifiers: vec![],
        symbols: vec![parent, variant],
        path: "error.rs".into(),
        text: text.into(),
        highlights: vec![],
    };
    m.on_event(event.clone());
    let event = m.definition_reply(event);
    m.on_event(event.clone());
    let loaded = FrameFixture::new(&m).render();
    assert!(
        loaded
            .lines()
            .nth(4)
            .unwrap()
            .chars()
            .skip(45)
            .collect::<String>()
            .starts_with("│ enum Error {")
    );
    assert!(
        loaded
            .lines()
            .nth(7)
            .unwrap()
            .chars()
            .skip(45)
            .collect::<String>()
            .starts_with("│     Engine(")
    );
    m.update(Action::FocusNth(5));
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 3 }]
        if path.as_path() == std::path::Path::new("error.rs"))
    );
    m.update(Action::FocusNth(1));
    m.on_event(event);
    assert_eq!(FrameFixture::new(&m).render(), loaded);
    let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
    terminal
        .draw(|frame| App::new(&m, Painter::colored(), 0).render(frame.area(), frame.buffer_mut()))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(51, 7)].symbol(), "E");
    let highlighted = terminal.backend().buffer()[(51, 7)].style();
    assert_eq!(highlighted.fg, Painter::colored().hit.fg);
    assert_eq!(
        highlighted.add_modifier,
        Painter::colored().hit.add_modifier
    );
    assert!(
        loaded
            .lines()
            .nth(5)
            .unwrap()
            .chars()
            .skip(45)
            .collect::<String>()
            .starts_with("│     Io,")
    );
    assert!(
        loaded
            .lines()
            .nth(8)
            .unwrap()
            .chars()
            .skip(45)
            .collect::<String>()
            .starts_with("│     Editor,")
    );
    assert!(
        loaded
            .lines()
            .nth(9)
            .unwrap()
            .chars()
            .skip(45)
            .collect::<String>()
            .starts_with("│ }")
    );
    insta::assert_snapshot!("definition_enum_variant", loaded);
    let mut sibling = fx::decl("error.rs", 1, SymbolKind::Variant, "Io", "    Io,");
    let sibling_start = text.find("Io,").unwrap();
    sibling.symbol.as_mut().unwrap().span = vvv_engine::Span::new(sibling_start, sibling_start + 2);
    sibling.symbol.as_mut().unwrap().name_span = sibling.symbol.as_ref().unwrap().span;
    let mut matches = m.search.results.matches.to_vec();
    matches.push(sibling);
    m.search.results.replace(matches);
    assert!(
        m.update(Action::Move(-1))
            .iter()
            .any(|e| matches!(e, Effect::Definition { .. })),
        "a new occurrence requests coherent metadata"
    );
    assert_eq!(
        m.search
            .body
            .symbol(m.search.results.current().unwrap())
            .unwrap()
            .name,
        "Error"
    );
    m.update(Action::FocusNth(5));
    m.update(Action::Bottom);
    assert_eq!(m.search.body.scroll, 5, "scroll bounds cover the full enum");
}

#[test]
fn definition_replies_are_ticketed_and_keep_the_previous_frame_while_pending() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    m.update(Action::Scroll(1));
    let before = m.search.body.preview.clone();
    m.update(Action::Move(1));
    let (old_ticket, old_query) = m.search.body.pending().unwrap();
    m.update(Action::Move(1));
    let (ticket, query) = m.search.body.pending().unwrap();
    assert_ne!(ticket, old_ticket);
    let failed = |ticket, query| Event::DefinitionResolved {
        ticket,
        query,
        reply: Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Stale,
            "Source changed; refresh the search",
        )),
    };
    m.on_event(failed(old_ticket, old_query));
    assert_eq!(m.search.body.preview, before);
    assert_eq!(m.search.body.scroll, 1);
    assert!(m.search.body.pending().is_some());
    assert!(!FrameFixture::new(&m).render().contains("Loading"));
    insta::assert_snapshot!("definition_pending", FrameFixture::new(&m).render());
    m.on_event(failed(ticket, query));
    assert!(m.search.body.pending().is_none());
    assert!(m.search.body.declaration().is_none());
    assert!(
        m.search
            .body
            .message
            .as_deref()
            .unwrap()
            .contains("Source changed")
    );
}

#[test]
fn old_success_cannot_replace_a_new_selection_or_result_set() {
    let mut m = searched();
    let file = m.search.body.preview.clone().unwrap();
    m.update(Action::Move(1));
    let reply = m.definition_reply(Event::Previewed {
        identifiers: vec![],
        path: file.path.clone(),
        text: file.text.clone(),
        highlights: file.highlights.clone(),
        symbols: file.symbols.clone(),
    });
    m.update(Action::Move(1));
    let pending = m.search.body.pending();
    m.on_event(reply.clone());
    assert_eq!(m.search.body.pending(), pending);
    m.search.results.replace(m.search.results.matches.to_vec());
    m.search.selection_changed();
    let pending = m.search.body.pending();
    m.on_event(reply);
    assert_eq!(m.search.body.pending(), pending);
}

#[test]
fn same_definition_keeps_scroll_and_duplicate_success_has_no_effect() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    m.update(Action::Scroll(2));
    let file = m.search.body.preview.clone().unwrap();
    m.update(Action::Move(1));
    let reply = m.definition_reply(Event::Previewed {
        identifiers: vec![],
        path: file.path.clone(),
        text: file.text.clone(),
        highlights: file.highlights.clone(),
        symbols: file.symbols.clone(),
    });
    m.on_event(reply.clone());
    assert_eq!(m.search.body.scroll, 2);
    m.update(Action::Scroll(1));
    m.on_event(reply);
    assert_eq!(m.search.body.scroll, 3);
}

#[test]
fn ambiguous_and_unavailable_answers_settle_without_guessing() {
    for outcome in [
        vvv_engine::NavigationOutcome::Ambiguous { candidates: vec![] },
        vvv_engine::NavigationOutcome::Unavailable {
            reason: vvv_engine::UnavailableReason::UnsupportedContext,
        },
    ] {
        let mut m = searched();
        m.update(Action::Move(1));
        let (ticket, query) = m.search.body.pending().unwrap();
        m.on_event(Event::DefinitionResolved {
            ticket,
            query,
            reply: Ok(vvv_engine::NavigationReply {
                snapshot: vvv_engine::ContentId::of("").into(),
                outcome,
            }),
        });
        assert!(m.search.body.pending().is_none());
        assert!(m.search.body.declaration().is_none());
        assert!(m.search.results.has_body());
        assert!(m.search.body.message.is_some());
    }
}

#[test]
fn variant_navigation_reveals_the_selection_using_the_actual_pane_height() {
    let text = format!(
        "enum State {{\n{}\n}}",
        (0..20)
            .map(|i| format!("    V{i},"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let start = text.find("V19,").unwrap();
    let mut declaration = fx::decl("state.rs", 20, SymbolKind::Variant, "V19", "    V19,");
    let variant = vvv_engine::Symbol::plain(
        SymbolKind::Variant,
        "V19",
        vvv_engine::Span::new(start, start + 3),
        vvv_engine::Span::new(start, start + 3),
    );
    declaration.symbol = Some(variant.clone());
    let parent = vvv_engine::Symbol::plain(
        SymbolKind::Enum,
        "State",
        vvv_engine::Span::new(5, 10),
        vvv_engine::Span::new(0, text.len()),
    );
    let mut m = model();
    m.search.results.replace(vec![declaration]);
    m.search.selection_changed();
    m.on_event(Event::Viewport {
        width: 90,
        height: 20,
    });
    let event = m.definition_reply(Event::Previewed {
        identifiers: vec![],
        path: "state.rs".into(),
        text,
        highlights: vec![],
        symbols: vec![parent, variant],
    });
    m.on_event(event.clone());
    m.search.focus = SearchPanel::Results;
    let effects = m.update(Action::Follow);
    let Event::DefinitionResolved { reply, .. } = event else {
        panic!()
    };
    m.on_event(FollowFixture { effects }.reply(reply));
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 20 }] if path.as_path() == std::path::Path::new("state.rs"))
    );
    let height = m.search.body.viewport.unwrap();
    assert_eq!(m.search.body.scroll, 20 - (height - 1));
    let frame = FrameFixture::new(&m).render();
    assert!(
        frame
            .lines()
            .skip(4)
            .any(|line| line.chars().skip(45).collect::<String>().contains("V19"))
    );
}

struct DefinitionFixture {
    reply: vvv_engine::NavigationReply,
}
impl DefinitionFixture {
    fn new(name: &str, path: &str, text: &str) -> Self {
        use vvv_engine::{
            ContentId, DefinitionPreview, NavigationOutcome, NavigationReply, ResolutionEvidence,
            SourceAnchor, Span, SymbolRef,
        };
        let mut declaration = fx::decl(path, 0, SymbolKind::Struct, name, text);
        let start = text.find(name).unwrap();
        let symbol = declaration.symbol.as_mut().unwrap();
        symbol.span = Span::new(0, text.len());
        symbol.extent = symbol.span;
        symbol.name_span = Span::new(start, start + name.len());
        declaration.content = Some(ContentId::of(text));
        let symbol = symbol.clone();
        let target = SymbolRef {
            language: declaration.language.clone(),
            kind: symbol.kind,
            name_span: symbol.name_span,
            declaration: SourceAnchor {
                path: path.into(),
                content: ContentId::of(text),
                span: symbol.extent,
            },
        };
        let identifiers = [name, "Beta"]
            .into_iter()
            .flat_map(|name| {
                text.match_indices(name)
                    .map(move |(start, _)| SourceAnchor {
                        path: path.into(),
                        content: ContentId::of(text),
                        span: Span::new(start, start + name.len()),
                    })
            })
            .collect::<Vec<_>>();
        Self {
            reply: NavigationReply {
                snapshot: ContentId::of(text).into(),
                outcome: NavigationOutcome::Resolved {
                    target: target.clone(),
                    evidence: ResolutionEvidence {
                        semantic: None,
                        addresses: vec![],
                    },
                    preview: Box::new(DefinitionPreview {
                        container: target,
                        declaration,
                        selection: symbol.name_span,
                        identifiers: identifiers.clone(),
                        source: vvv_engine::File {
                            path: path.into(),
                            text: text.into(),
                            symbols: vec![symbol],
                            highlights: vec![],
                            identifiers,
                        },
                    }),
                },
            },
        }
    }
    fn reply(&self) -> vvv_engine::NavigationReply {
        self.reply.clone()
    }
    fn browsing(&self) -> (Model, vvv_engine::NavigationReply) {
        let reply = self.reply();
        let vvv_engine::NavigationOutcome::Resolved { preview, .. } = &reply.outcome else {
            unreachable!()
        };
        let mut m = model();
        let effects = typed(&mut m, "Alpha");
        m.on_event(Event::Searched {
            generation: generation_of(&effects),
            matches: vec![preview.declaration.clone()],
            skipped: vec![],
        });
        let (ticket, query) = m.search.body.pending().unwrap();
        m.on_event(Event::DefinitionResolved {
            ticket,
            query,
            reply: Ok(reply.clone()),
        });
        m.search.preview = m.search.body.preview.clone();
        m.search.focus = SearchPanel::Body;
        (m, reply)
    }
}
impl Default for DefinitionFixture {
    fn default() -> Self {
        Self::new(
            "Alpha",
            "a.rs",
            "struct Alpha {\n    first: Beta,\n    second: Beta,\n}",
        )
    }
}
struct FollowFixture {
    effects: Vec<Effect>,
}
impl FollowFixture {
    fn reply(self, reply: Result<vvv_engine::NavigationReply, vvv_engine::Failure>) -> Event {
        let [Effect::Follow { ticket, query }] = self.effects.as_slice() else {
            panic!("expected follow: {:?}", self.effects)
        };
        Event::Followed {
            ticket: *ticket,
            query: query.clone(),
            reply,
        }
    }
}
impl Model {
    fn pick_identifier(&mut self, name: &str) -> Vec<Effect> {
        self.update(Action::Follow);
        for c in name.chars() {
            self.update(Action::Input(c));
        }
        self.update(Action::MenuChoose)
    }
}

#[test]
fn identifier_picker_filters_repeated_names_and_follows_exact_anchors() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    m.update(Action::Follow);
    for c in "Beta".chars() {
        m.update(Action::Input(c));
    }
    let Some(Overlay::Navigation(picker)) = &m.overlay else {
        panic!()
    };
    assert_eq!(picker.visible().len(), 2);
    assert!(picker.visible()[0].label.contains("first:"));
    insta::assert_snapshot!("navigation_identifiers", FrameFixture::new(&m).render());
    m.update(Action::Move(1));
    let expected = match &m.overlay {
        Some(Overlay::Navigation(p)) => p.chosen().unwrap(),
        _ => panic!(),
    };
    let effects = m.update(Action::MenuChoose);
    assert!(matches!(&effects[0], Effect::Follow { query, .. } if query == &expected));
    assert_eq!(m.search.query.text(), "Alpha");
}

#[test]
fn following_restores_both_scrolls_focus_query_and_shared_results() {
    let (mut m, original) = DefinitionFixture::default().browsing();
    m.search.body.scroll = 1;
    m.search.preview_scroll = Some(2);
    let matches = m.search.results.matches.clone();
    let effects = m.pick_identifier("Beta");
    let destination = DefinitionFixture::new("Beta", "b.rs", "struct Beta {}").reply();
    let event = FollowFixture { effects }.reply(Ok(destination.clone()));
    m.on_event(event.clone());
    assert_eq!(
        m.search.body.declaration().unwrap().path.as_path(),
        std::path::Path::new("b.rs")
    );
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 0 }] if path.as_path() == std::path::Path::new("b.rs"))
    );
    insta::assert_snapshot!("navigation_destination", FrameFixture::new(&m).render());
    let effects = m.update(Action::BrowseBack);
    assert_eq!(m.search.query.text(), "Alpha");
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert_eq!(m.search.body.scroll, 1);
    assert_eq!(m.search.preview_scroll, Some(2));
    assert!(std::sync::Arc::ptr_eq(&matches, &m.search.results.matches));
    assert!(m.search.stale);
    m.on_event(event); // late duplicate from the page we just left
    assert!(m.search.stale);
    m.on_event(FollowFixture { effects }.reply(Ok(original)));
    assert!(!m.search.stale);
    assert_eq!(m.search.body.scroll, 1);
    let effects = m.update(Action::BrowseForward);
    m.on_event(FollowFixture { effects }.reply(Ok(destination)));
    assert_eq!(
        m.search.body.declaration().unwrap().path.as_path(),
        std::path::Path::new("b.rs")
    );
}

#[test]
fn cancelled_failed_and_ambiguous_follows_do_not_add_history() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    m.update(Action::Follow);
    m.update(Action::Back);
    assert!(m.update(Action::BrowseBack).is_empty());
    let effects = m.pick_identifier("Beta");
    m.on_event(
        FollowFixture { effects }.reply(Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Io,
            "unreadable",
        ))),
    );
    assert!(m.update(Action::BrowseBack).is_empty());
    let reply = DefinitionFixture::new("Beta", "one.rs", "struct Beta {}").reply();
    let vvv_engine::NavigationOutcome::Resolved {
        target,
        preview,
        evidence,
    } = reply.outcome
    else {
        unreachable!()
    };
    let candidate = vvv_engine::DefinitionCandidate {
        target,
        declaration: preview.declaration,
        evidence,
    };
    let effects = m.pick_identifier("Beta");
    let query = match &effects[0] {
        Effect::Follow { query, .. } => query.clone(),
        _ => panic!(),
    };
    m.on_event(
        FollowFixture { effects }.reply(Ok(vvv_engine::NavigationReply {
            snapshot: reply.snapshot,
            outcome: vvv_engine::NavigationOutcome::Ambiguous {
                candidates: vec![candidate.clone()],
            },
        })),
    );
    insta::assert_snapshot!("navigation_candidates", FrameFixture::new(&m).render());
    let effects = m.update(Action::MenuChoose);
    assert!(
        matches!(&effects[0], Effect::Follow { query: selected, .. } if selected.origin == query.origin && selected.selection == Selection::ids([candidate.declaration.id]))
    );
    // Cancelling the in-flight follow also makes its failure harmless.
    m.update(Action::Back);
    let before = m.status.clone();
    m.on_event(
        FollowFixture { effects }.reply(Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Io,
            "late",
        ))),
    );
    assert_eq!(m.status, before);
    assert!(m.update(Action::BrowseBack).is_empty());
}

#[test]
fn stale_history_keeps_old_bytes_and_requires_refresh() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    let effects = m.pick_identifier("Beta");
    m.on_event(FollowFixture { effects }.reply(Ok(
        DefinitionFixture::new("Beta", "b.rs", "struct Beta {}").reply(),
    )));
    let effects = m.update(Action::BrowseBack);
    m.on_event(
        FollowFixture { effects }.reply(Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Stale,
            "a.rs changed",
        ))),
    );
    assert!(m.search.stale);
    let old = m.search.body.preview.as_ref().unwrap().text().to_owned();
    assert!(m.update(Action::Follow).is_empty());
    assert!(m.overlay.is_none());
    assert_eq!(m.search.body.preview.as_ref().unwrap().text(), old);
    insta::assert_snapshot!("navigation_stale", FrameFixture::new(&m).render());
    assert!(matches!(
        m.update(Action::Refresh).as_slice(),
        [Effect::Search { .. }]
    ));
}

#[test]
fn new_success_after_back_replaces_forward_but_failure_preserves_it() {
    let (mut m, original) = DefinitionFixture::default().browsing();
    let beta = DefinitionFixture::new("Beta", "b.rs", "struct Beta {}").reply();
    let effects = m.pick_identifier("Beta");
    m.on_event(FollowFixture { effects }.reply(Ok(beta.clone())));
    let back = m.update(Action::BrowseBack);
    m.on_event(FollowFixture { effects: back }.reply(Ok(original.clone())));
    let effects = m.pick_identifier("Beta");
    m.on_event(
        FollowFixture { effects }.reply(Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Io,
            "failed",
        ))),
    );
    let forward = m.update(Action::BrowseForward);
    m.on_event(FollowFixture { effects: forward }.reply(Ok(beta)));
    let back = m.update(Action::BrowseBack);
    m.on_event(FollowFixture { effects: back }.reply(Ok(original)));
    let effects = m.pick_identifier("Beta");
    m.on_event(FollowFixture { effects }.reply(Ok(
        DefinitionFixture::new("Beta", "new.rs", "struct Beta {}").reply(),
    )));
    assert!(m.update(Action::BrowseForward).is_empty());
}

#[test]
fn context_picker_and_navigation_keys_use_the_displayed_source() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    m.search.focus = SearchPanel::Context;
    m.search.preview_scroll = Some(2);
    assert_eq!(m.action_for(key(KeyCode::Enter)), Some(Action::Follow));
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
        Some(Action::BrowseBack)
    );
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
        Some(Action::BrowseForward)
    );
    m.on_key(key(KeyCode::Enter));
    let Some(Overlay::Navigation(picker)) = &m.overlay else {
        panic!()
    };
    let query = picker.chosen().unwrap();
    assert!(
        matches!(query.origin, vvv_engine::NavigationOrigin::Occurrence { anchor } if anchor.span.start == m.search.preview.as_ref().unwrap().text().rfind("Beta").unwrap())
    );
    m.update(Action::Top);
    m.update(Action::Bottom);
    assert!(
        matches!(&m.overlay, Some(Overlay::Navigation(p)) if p.cursor.index == p.items.len() - 1)
    );
    m.on_event(Event::SourcesChanged);
    assert!(m.overlay.is_none());
    assert!(m.search.stale);
    assert!(m.update(Action::Follow).is_empty());
}

#[test]
fn a_reply_cannot_enter_a_page_after_selection_or_mode_changes() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    let effects = m.pick_identifier("Beta");
    let reply = FollowFixture { effects }.reply(Ok(DefinitionFixture::new(
        "Beta",
        "b.rs",
        "struct Beta {}",
    )
    .reply()));
    m.update(Action::Help);
    let status = m.status.clone();
    m.on_event(reply);
    assert!(matches!(m.overlay, Some(Overlay::Help { .. })));
    assert_eq!(m.status, status);
    assert_eq!(
        m.search.body.declaration().unwrap().path.as_path(),
        std::path::Path::new("a.rs")
    );
}

#[test]
fn restored_references_keep_their_filter_and_selected_occurrence() {
    let (mut m, original) = DefinitionFixture::default().browsing();
    m.search.results.entered(fx::references());
    m.search.results.set_relation(Relation::Resolved);
    m.search.results.cursor.index = 1;
    m.search.page = crate::modes::search::browse::BrowsePage::References;
    let references = m.search.results.references.clone().unwrap();
    let selected = m.search.results.current().unwrap().id.clone();
    let effects = m.pick_identifier("Beta");
    m.on_event(FollowFixture { effects }.reply(Ok(
        DefinitionFixture::new("Beta", "b.rs", "struct Beta {}").reply(),
    )));
    let effects = m.update(Action::BrowseBack);
    assert!(matches!(
        m.search.page,
        crate::modes::search::browse::BrowsePage::References
    ));
    assert_eq!(m.search.results.relation, Relation::Resolved);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    assert!(std::sync::Arc::ptr_eq(
        m.search.results.references.as_ref().unwrap(),
        &references
    ));
    m.on_event(FollowFixture { effects }.reply(Ok(original)));
}

#[test]
fn back_to_an_ambiguous_origin_validates_without_opening_a_picker() {
    let (mut m, original) = DefinitionFixture::default().browsing();
    let vvv_engine::NavigationOutcome::Resolved { preview, .. } = &original.outcome else {
        panic!()
    };
    let ambiguous = vvv_engine::NavigationReply {
        snapshot: original.snapshot.clone(),
        outcome: vvv_engine::NavigationOutcome::Ambiguous { candidates: vec![] },
    };
    m.search.results.replace(vec![preview.declaration.clone()]);
    m.search.selection_changed();
    let (ticket, query) = m.search.body.pending().unwrap();
    m.on_event(Event::DefinitionResolved {
        ticket,
        query,
        reply: Ok(ambiguous.clone()),
    });
    m.search.focus = SearchPanel::Results;
    let effects = m.update(Action::Follow);
    m.on_event(FollowFixture { effects }.reply(Ok(
        DefinitionFixture::new("Beta", "b.rs", "struct Beta {}").reply(),
    )));
    let back = m.update(Action::BrowseBack);
    m.on_event(FollowFixture { effects: back }.reply(Ok(ambiguous)));
    assert!(!m.search.stale);
    assert!(m.overlay.is_none());
    assert_eq!(
        m.search.body.message.as_deref(),
        Some("Several definitions match")
    );
}

#[test]
fn categories_keep_exact_occurrences_and_switching_from_kinds_restores_uses() {
    use crate::modes::search::Category;
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.update(Action::File(2));
    m.update(Action::Move(1));
    let use_id = m.search.results.current().unwrap().id.clone();
    m.update(Action::OpenMenu(MenuTarget::Category));
    m.update(Action::Move(3));
    m.update(Action::MenuChoose);
    assert_eq!(m.search.results.category, Category::Uses);
    assert_eq!(m.search.results.len(), 1);
    assert_eq!(m.search.results.current().unwrap().id, use_id);
    assert_eq!(
        m.search.results.matches.len(),
        4,
        "hidden roles stay available"
    );
    let rw = crate::modes::rewrite::RewriteMode::from_results(&m.search.results).unwrap();
    assert_eq!(
        rw.matches.len(),
        1,
        "rewrite only offers the visible search matches"
    );
    m.update(Action::OpenMenu(MenuTarget::Symbol));
    typed(&mut m, "trait");
    m.update(Action::MenuChoose);
    assert_eq!(m.search.results.category, Category::Declarations);
    assert_eq!(m.search.query.filter(Filter::Symbol), Some("trait"));
    m.update(Action::OpenMenu(MenuTarget::Category));
    typed(&mut m, "uses");
    let effects = m.update(Action::MenuChoose);
    assert!(m.search.query.filter(Filter::Symbol).is_none());
    assert!(matches!(effects.as_slice(), [Effect::Search { .. }]));
    assert_eq!(m.search.results.category, Category::Uses);
}

#[test]
fn locations_are_engine_scopes_but_navigation_and_reference_subjects_are_global() {
    use crate::modes::search::Category;
    use vvv_engine::{NavigationOrigin, NavigationOutcome};
    let definition = DefinitionFixture::new(
        "Language",
        "crates/vvv-lang/src/language.rs",
        "trait Language {}",
    )
    .reply();
    let NavigationOutcome::Resolved { preview, .. } = &definition.outcome else {
        panic!()
    };
    let declaration_id = preview.declaration.id.clone();
    let mut m = model();
    typed(&mut m, "Language");
    m.update(Action::OpenMenu(MenuTarget::Location));
    typed(&mut m, "crates/vvv");
    let effects = m.update(Action::MenuChoose);
    assert!(
        matches!(effects.as_slice(), [Effect::Search { scope, .. }] if scope.paths == [vvv_engine::RelPath::from("crates/vvv")])
    );
    let occurrence = fx::m("crates/vvv/src/main.rs", 5, 0, "Language", "Language");
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: vec![occurrence.clone()],
        skipped: vec![],
    });
    m.search.focus = SearchPanel::Results;
    m.update(Action::FilterFiles);
    typed(&mut m, "main");
    m.update(Action::Enter);
    let (ticket, query) = m.search.body.pending().unwrap();
    assert!(
        matches!(&query.origin, NavigationOrigin::Position { path, .. } if path.as_path() == std::path::Path::new("crates/vvv/src/main.rs"))
    );
    m.on_event(Event::DefinitionResolved {
        ticket,
        query,
        reply: Ok(definition.clone()),
    });
    assert!(
        m.search
            .body
            .declaration()
            .unwrap()
            .path
            .starts_with("crates/vvv-lang")
    );
    let references = m.update(Action::Enter);
    assert!(
        matches!(references.as_slice(), [Effect::Query { request: Request::References(query), .. }] if query.declared_in.as_ref().unwrap().as_path() == std::path::Path::new("crates/vvv-lang/src/language.rs"))
    );
    m.status.busy = false;
    let query_text = m.search.query.text().to_owned();
    m.search.results.set_category(Category::Uses);
    let effects = m.update(Action::Follow);
    m.on_event(FollowFixture { effects }.reply(Ok(definition)));
    assert_eq!(
        m.search.results.len(),
        1,
        "a followed declaration remains visible outside the result location"
    );
    assert_eq!(m.search.results.current().unwrap().id, declaration_id);
    assert!(m.search.results.files.filter.is_empty());
    m.update(Action::BrowseBack);
    assert_eq!(m.search.query.text(), query_text);
    assert_eq!(m.search.results.category, Category::Uses);
    assert_eq!(
        m.search.locations.selected.unwrap().as_path(),
        std::path::Path::new("crates/vvv")
    );
    assert_eq!(m.search.results.current().unwrap().id, occurrence.id);
    assert_eq!(m.search.results.files.filter, "main");
}

#[test]
fn file_jumps_skip_headings_and_filters_preserve_the_selected_match_id() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.on_key(key(KeyCode::Char(']')));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    m.on_key(key(KeyCode::Char(']')));
    assert_eq!(m.search.results.current().unwrap().start.line, 25);
    m.update(Action::Move(1));
    m.on_key(key(KeyCode::Char('[')));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    let id = m.search.results.current().unwrap().id.clone();
    let mut matches = m.search.results.matches.to_vec();
    matches.push(fx::decl(
        "aaa.rs",
        0,
        SymbolKind::Struct,
        "Language",
        "struct Language {}",
    ));
    m.search.results.replace(matches);
    assert_eq!(m.search.results.current().unwrap().id, id);
}

#[test]
fn searchable_menus_handle_no_matches_and_reject_paths_outside_the_workspace() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.on_key(key(KeyCode::Char('s')));
    typed(&mut m, "DOES_NOT_EXIST");
    assert!(m.update(Action::MenuChoose).is_empty());
    assert!(m.overlay.is_some(), "an empty menu has no implicit choice");
    m.update(Action::Back);
    m.on_key(key(KeyCode::Char('f')));
    typed(&mut m, "../outside");
    m.update(Action::MenuChoose);
    assert!(m.search.locations.selected.is_none());
    assert!(matches!(m.status.message, Some((Level::Error, _))));
    m.update(Action::OpenMenu(MenuTarget::Location));
    m.update(Action::MenuChoose);
    assert!(m.search.locations.selected.is_none());
    m.search.focus = SearchPanel::Query;
    assert_eq!(
        m.action_for(ctrl('f')),
        Some(Action::OpenMenu(MenuTarget::Location))
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('f'))),
        Some(Action::Input('f'))
    );
}

#[test]
fn file_list_keeps_its_full_identity_while_matches_scroll_or_paths_wrap() {
    let mut m = model();
    typed(&mut m, "Engine");
    let directory = "crates/vvv-engine/src/very/long/module/directory";
    let matches = (0..35)
        .map(|line| {
            fx::m(
                &format!("{directory}/engine.rs"),
                line,
                0,
                "Engine",
                "Engine",
            )
        })
        .collect();
    m.on_event(Event::Searched {
        generation: m.generation,
        matches,
        skipped: vec![],
    });
    m.search.focus = SearchPanel::Results;
    let before = FrameFixture::new(&m).render_size(120, 24);
    m.update(Action::Bottom);
    let wide = FrameFixture::new(&m).render_size(120, 24);
    let files = |frame: &str| {
        frame
            .lines()
            .take_while(|row| !row.starts_with('├'))
            .map(|row| row.chars().take(60).collect::<String>())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        files(&before),
        files(&wide),
        "match scrolling cannot move the file viewport"
    );
    assert!(wide.contains("engine.rs"));
    assert!(wide.contains(directory));
    assert!(wide.contains("> 35  Engine"));
    let narrow = FrameFixture::new(&m).render_size(70, 20);
    let left: String = narrow
        .lines()
        .skip(3)
        .take(16)
        .map(|row| {
            row.chars()
                .skip(3)
                .take(25)
                .filter(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '.'))
                .collect::<String>()
        })
        .collect();
    assert!(
        left.contains(&format!("{directory}/engine.rs")),
        "wrapped headings preserve every path component: {narrow}"
    );
    insta::assert_snapshot!("grouped_scrolled", wide);
}

#[test]
fn fuzzy_file_filter_is_local_cancellable_and_keeps_each_files_selected_match() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    let query = m.search.query.text().to_owned();
    let generation = m.generation;
    m.on_key(key(KeyCode::Char('F')));
    assert_eq!(
        m.action_for(key(KeyCode::Char('r'))),
        Some(Action::Input('r'))
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char('F'))),
        Some(Action::Input('F'))
    );
    let effects = typed(&mut m, "lib");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Search { .. } | Effect::Query { .. }))
    );
    assert_eq!(m.generation, generation);
    assert_eq!(m.search.query.text(), query);
    assert_eq!(m.search.results.file_groups().len(), 1);
    assert_eq!(m.search.results.len(), 2);
    let frame = FrameFixture::new(&m).render_size(120, 24);
    assert!(frame.contains("Files · filter") && frame.contains("ctrl+f in:"));
    assert!(
        !frame.contains("F files")
            && !frame.contains("4 source")
            && !frame.contains("5 definition")
    );
    insta::assert_snapshot!("fuzzy_file_filter", frame);
    m.on_key(key(KeyCode::Enter));
    assert!(m.search.results.files.edit.is_none());
    m.update(Action::Move(1));
    let selected = m.search.results.current().unwrap().id.clone();
    assert_eq!(m.search.results.current().unwrap().start.line, 40);
    m.on_key(key(KeyCode::Char('F')));
    m.on_key(ctrl('u'));
    m.on_key(key(KeyCode::Up));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.results.files.filter, "lib");
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.on_key(key(KeyCode::Char('F')));
    m.on_key(ctrl('u'));
    m.on_key(key(KeyCode::Enter));
    m.on_key(key(KeyCode::Char('[')));
    m.on_key(key(KeyCode::Char(']')));
    assert_eq!(m.search.results.current().unwrap().id, selected);
    assert_eq!(m.search.results.matches.len(), 4);
}

#[test]
fn empty_file_filter_results_clear_selection_reject_old_previews_and_cancel_restores_it() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.update(Action::File(1));
    let selected = m.search.results.current().unwrap().id.clone();
    let (ticket, query) = m.search.body.pending().unwrap();
    m.update(Action::FilterFiles);
    typed(&mut m, "zzzzzz");
    assert_eq!(m.search.results.len(), 0);
    assert!(m.search.results.current_site().is_none());
    assert!(m.search.body.pending().is_none());
    m.on_event(Event::DefinitionResolved {
        ticket,
        query,
        reply: Err(vvv_engine::Failure::new(
            vvv_engine::ErrorCode::Stale,
            "old reply",
        )),
    });
    assert!(
        !m.search
            .body
            .message
            .as_deref()
            .unwrap()
            .contains("old reply")
    );
    insta::assert_snapshot!("file_filter_empty", FrameFixture::new(&m).render());
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.results.current().unwrap().id, selected);
    assert!(m.search.results.files.filter.is_empty());
    assert!(m.search.body.pending().is_some());
}

#[test]
fn file_filter_cancel_restores_preview_focus_and_never_changes_panel_shortcuts() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    assert_eq!(
        m.action_for(key(KeyCode::Char('F'))),
        Some(Action::FilterFiles)
    );
    m.on_key(key(KeyCode::Char('F')));
    typed(&mut m, "lib");
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert!(m.search.results.files.filter.is_empty());
    m.on_key(key(KeyCode::Char('4')));
    assert_eq!(m.search.focus, SearchPanel::Context);
    m.on_key(key(KeyCode::Char('5')));
    assert_eq!(m.search.focus, SearchPanel::Body);
}

#[test]
fn fuzzy_file_filter_never_narrows_rewrite_and_references_keep_one_group_per_file() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.update(Action::FilterFiles);
    typed(&mut m, "lib");
    m.update(Action::Enter);
    let rewrite = crate::modes::rewrite::RewriteMode::from_results(&m.search.results).unwrap();
    assert_eq!(rewrite.matches.len(), 4);
    assert_eq!(rewrite.ticks.len(), 4);
    m.on_event(Event::Answered {
        generation: m.generation,
        answer: Box::new(Answer::References(fx::references())),
    });
    assert_eq!(m.search.results.file_groups().len(), 1);
    assert_eq!(m.search.results.file_groups()[0].matches.len(), 2);
    assert_eq!(
        m.search
            .results
            .references
            .as_ref()
            .unwrap()
            .occurrences
            .len(),
        4
    );
    let frame = FrameFixture::new(&m).render_size(120, 24);
    assert!(frame.contains("✓ 3  ? 1  ✗ 0"));
    assert_eq!(
        crate::modes::rewrite::RewriteMode::from_results(&m.search.results)
            .unwrap()
            .matches
            .len(),
        4
    );
}

#[test]
fn changing_categories_and_entering_references_keep_the_active_file_when_possible() {
    use crate::modes::search::Category;
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.update(Action::File(2));
    m.update(Action::Move(1));
    m.search.results.set_category(Category::Imports);
    m.search.selection_changed();
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lib.rs")
    );
    assert_eq!(m.search.results.current().unwrap().start.line, 25);
    m.search.results.set_category(Category::All);
    m.search.selection_changed();
    m.update(Action::Move(1));
    let selected = m.search.results.current().unwrap().id.clone();
    m.on_event(Event::Answered {
        generation: m.generation,
        answer: Box::new(Answer::References(fx::references())),
    });
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.search.results.set_relation(Relation::Unresolved);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.update(Action::Back); // Matches → Files
    m.update(Action::Back); // Files → Query
    m.update(Action::Back); // Leave the relation
    assert!(!m.search.results.is_anchored());
    assert_eq!(m.search.results.current().unwrap().id, selected);
}

#[test]
fn source_excerpts_explain_results_without_opening_a_preview() {
    let mut m = model();
    typed(&mut m, "Engine");
    let import = "use vvv_engine::{Answer, Call, ErrorCode, Failure, Engine, Reply};";
    let matches = vec![
        fx::m(
            "crates/vvv/src/cli/commands/serve.rs",
            5,
            import.find("Engine").unwrap() as u32,
            "Engine",
            import,
        ),
        fx::m(
            "crates/vvv/src/cli/commands/serve.rs",
            37,
            8,
            "Engine",
            "engine: Engine,",
        ),
        fx::m(
            "crates/vvv/src/cli/commands/serve.rs",
            62,
            15,
            "Engine",
            "fn engine() -> Engine {",
        ),
        fx::m(
            "crates/vvv/src/context.rs",
            11,
            8,
            "Engine",
            "engine: Engine,",
        ),
    ];
    m.on_event(Event::Searched {
        generation: m.generation,
        matches,
        skipped: vec![],
    });
    m.search.focus = SearchPanel::Results;
    let frame = FrameFixture::new(&m).render_size(120, 24);
    assert!(frame.contains("crates/vvv/src/cli/commands/serve.rs"));
    assert!(frame.contains("engine: Engine,"));
    assert!(frame.contains("fn engine() -> Engine {"));
    let import_row = frame
        .lines()
        .find(|line| line.contains("use vvv_engine::{"))
        .unwrap();
    assert!(import_row.contains("Engine"), "{import_row}");
    assert!(import_row.contains('…'), "{import_row}");
    m.update(Action::Move(1));
    assert_eq!(m.search.results.current().unwrap().start.line, 37);
    m.update(Action::File(1));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("crates/vvv/src/context.rs")
    );
    insta::assert_snapshot!("readable_results", frame);
}

#[test]
fn snapshot_location_category_and_wide_cross_crate_preview() {
    let definition = DefinitionFixture::new(
        "Engine",
        "crates/vvv-engine/src/engine.rs",
        "pub struct Engine {\n    workspace: Workspace,\n    languages: LanguageRegistry,\n}",
    )
    .reply();
    let mut m = model();
    typed(&mut m, "Engine");
    m.search.locations.select(Some("crates/vvv")).unwrap();
    m.search
        .results
        .set_location(m.search.locations.selected.clone());
    let matches = vec![
        fx::m(
            "crates/vvv/src/cli/commands/serve.rs",
            37,
            8,
            "Engine",
            "engine: Engine,",
        ),
        fx::m(
            "crates/vvv/src/cli/context.rs",
            11,
            8,
            "Engine",
            "engine: Engine,",
        ),
    ];
    m.on_event(Event::Searched {
        generation: m.generation,
        matches,
        skipped: vec![],
    });
    m.search.focus = SearchPanel::Results;
    let (ticket, query) = m.search.body.pending().unwrap();
    m.on_event(Event::DefinitionResolved {
        ticket,
        query,
        reply: Ok(definition),
    });
    m.on_event(preview(
        "crates/vvv/src/cli/commands/serve.rs",
        &numbered(42, &[(38, "    engine: Engine,")])
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    ));
    insta::assert_snapshot!("scoped_wide", FrameFixture::new(&m).render_size(120, 30));
    m.update(Action::OpenMenu(MenuTarget::Location));
    typed(&mut m, "crates");
    insta::assert_snapshot!(
        "location_picker",
        FrameFixture::new(&m).render_size(120, 30)
    );
    m.update(Action::Back);
    m.update(Action::OpenMenu(MenuTarget::Category));
    insta::assert_snapshot!("category_picker", FrameFixture::new(&m).render());
}

#[test]
fn reference_locations_keep_global_verdicts_and_leaving_refreshes_changed_search_scope() {
    let mut m = anchored();
    m.search.focus = SearchPanel::Results;
    let query = m.search.query.text().to_owned();
    m.update(Action::OpenMenu(MenuTarget::Location));
    typed(&mut m, "src/lib.rs");
    let effects = m.update(Action::MenuChoose);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Search { .. })),
        "reference scopes filter an already judged answer"
    );
    assert_eq!(m.search.results.len(), 2);
    assert!(
        m.search
            .results
            .listed()
            .iter()
            .all(|m| m.path.as_path() == std::path::Path::new("src/lib.rs"))
    );
    assert_eq!(
        m.search
            .results
            .references
            .as_ref()
            .unwrap()
            .occurrences
            .len(),
        4
    );
    m.update(Action::Move(1));
    let frame = FrameFixture::new(&m).render();
    assert!(frame.contains("? Language::new()"));
    m.update(Action::Back);
    m.update(Action::Back);
    let effects = m.update(Action::Back);
    assert!(
        matches!(effects.as_slice(), [Effect::Search { scope, .. }] if scope.paths == [vvv_engine::RelPath::from("src/lib.rs")])
    );
    assert_eq!(m.search.query.text(), query);
}

impl Model {
    fn large_file_results(&mut self) {
        let matches = (0..20)
            .flat_map(|file| {
                (0..30).map(move |line| {
                    let path = format!("src/file{file:02}.rs");
                    let mut m = fx::m(&path, file * 100 + line, 0, "Thing", "Thing()");
                    m.id = vvv_engine::MatchId::derive(&m.path, m.span, &format!("{file}-{line}"));
                    m
                })
            })
            .collect();
        self.search.results.replace(matches);
        self.search.selection_changed();
        self.on_event(Event::Viewport {
            width: 120,
            height: 24,
        });
    }

    fn mouse(
        &mut self,
        kind: ratatui::crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
    ) -> Vec<Effect> {
        let event = ratatui::crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        };
        self.search_frame()
            .pointer(event)
            .map_or_else(Vec::new, |pointer| self.on_event(Event::Pointer(pointer)))
    }
}

#[test]
fn file_list_keys_share_search_actions_and_keep_filter_text_and_focus_local() {
    let mut m = searched();
    m.on_key(key(KeyCode::Tab));
    assert_eq!(m.search.focus, SearchPanel::Files);
    for (c, expected) in [
        ('f', Action::OpenMenu(MenuTarget::Location)),
        ('s', Action::OpenMenu(MenuTarget::Symbol)),
        ('t', Action::OpenMenu(MenuTarget::Category)),
        ('L', Action::OpenMenu(MenuTarget::Language)),
        ('r', Action::Rename),
        ('m', Action::MoveFile),
        ('M', Action::MoveSymbol),
        ('w', Action::Rewrite),
        ('o', Action::Follow),
        ('p', Action::PreviewTab),
        ('h', Action::History),
        ('u', Action::Undo),
        ('/', Action::FocusNth(1)),
        ('i', Action::FocusNth(1)),
        ('[', Action::File(-1)),
        (']', Action::File(1)),
        ('q', Action::Quit),
    ] {
        assert_eq!(m.action_for(key(KeyCode::Char(c))), Some(expected), "{c}");
    }
    for (c, target) in [
        ('f', MenuTarget::Location),
        ('s', MenuTarget::Symbol),
        ('t', MenuTarget::Category),
        ('L', MenuTarget::Language),
    ] {
        m.on_key(key(KeyCode::Char(c)));
        assert!(
            matches!(&m.overlay, Some(Overlay::Menu(menu)) if menu.target == target),
            "{c}"
        );
        m.on_key(key(KeyCode::Esc));
        assert_eq!(m.search.focus, SearchPanel::Files);
    }
    m.on_key(ctrl('n'));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    m.on_key(ctrl('p'));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/mod.rs")
    );
    m.on_key(key(KeyCode::Right));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.on_key(key(KeyCode::Esc));
    m.on_key(key(KeyCode::Char('/')));
    assert_eq!(m.search.focus, SearchPanel::Query);

    m.on_key(key(KeyCode::Tab));
    m.search.results.entered(fx::references());
    assert_eq!(
        m.action_for(key(KeyCode::Char('R'))),
        Some(Action::OpenMenu(MenuTarget::Relation))
    );
    m.on_key(key(KeyCode::Char('F')));
    for c in ['f', 's', 't', 'L', 'R', 'r', 'o', '/', 'q'] {
        assert_eq!(m.action_for(key(KeyCode::Char(c))), Some(Action::Input(c)));
    }
    m.on_key(key(KeyCode::Esc));
    let frame = FrameFixture::new(&m).render_size(120, 24);
    assert!(frame.contains("f location") && frame.contains("t category"));
    insta::assert_snapshot!("file_list_keys", frame);
}

#[test]
fn files_and_matches_have_independent_focus_and_clamped_keyboard_navigation() {
    let mut m = searched();
    m.update(Action::FocusNext);
    assert_eq!(m.search.focus, SearchPanel::Files);
    m.on_key(key(KeyCode::Down));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    m.on_key(key(KeyCode::End));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lib.rs")
    );
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.on_key(key(KeyCode::End));
    let last = m.search.results.current().unwrap().id.clone();
    m.on_key(key(KeyCode::Down));
    assert_eq!(m.search.results.current().unwrap().id, last);
    m.on_key(key(KeyCode::Home));
    m.on_key(key(KeyCode::Up));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lib.rs")
    );
    m.on_key(key(KeyCode::End));
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.focus, SearchPanel::Files);
    m.on_key(key(KeyCode::Up));
    m.on_key(key(KeyCode::Down));
    assert_eq!(m.search.results.current().unwrap().id, last);
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.focus, SearchPanel::Query);
    let query = m.search.query.text().to_owned();
    let generation = m.generation;
    m.on_key(ctrl('p'));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/lang/registry.rs")
    );
    m.on_key(ctrl('n'));
    assert_eq!(m.search.results.current().unwrap().id, last);
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert_eq!(m.search.query.text(), query);
    assert_eq!(m.generation, generation);
}

#[test]
fn file_shortcuts_and_tab_order_skip_unavailable_panes_without_renumbering() {
    let mut m = searched();
    for (n, panel) in [
        (2, SearchPanel::Files),
        (3, SearchPanel::Results),
        (4, SearchPanel::Context),
        (5, SearchPanel::Body),
    ] {
        m.update(Action::FocusNth(n));
        assert_eq!(m.search.focus, panel);
    }
    m.search.results.entered(fx::references());
    m.search.results.show_impact(fx::impact());
    m.search.focus = SearchPanel::Query;
    m.update(Action::FocusNext);
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusNth(2));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusNth(4));
    assert_eq!(m.search.focus, SearchPanel::Context);
}

#[test]
fn mouse_wheel_scrolls_the_hovered_list_without_selection_or_focus_changes() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut m = searched();
    m.large_file_results();
    let selected = m.search.results.current().unwrap().id.clone();
    let frame = m.search_frame();
    let files = &frame.lists[0];
    let (x, y) = (files.content.x, files.content.y);
    assert!(m.mouse(MouseEventKind::ScrollDown, x, y).is_empty());
    assert_eq!(m.search.results.files.viewport.offset, 3);
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.mouse(MouseEventKind::Down(MouseButton::Left), x, y + 1);
    assert_eq!(m.search.focus, SearchPanel::Files);
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/file04.rs")
    );
    let current = m.search.results.current().unwrap().id.clone();
    let frame = m.search_frame();
    let matches = &frame.lists[1];
    let (x, y) = (matches.content.x, matches.content.y);
    m.mouse(
        MouseEventKind::Down(MouseButton::Left),
        matches.area.x + 1,
        matches.area.y,
    );
    assert_eq!(
        m.search.focus,
        SearchPanel::Results,
        "shared border belongs to the Matches title"
    );
    m.update(Action::FocusNth(2));
    m.mouse(MouseEventKind::ScrollDown, x, y);
    assert_eq!(m.search.results.current().unwrap().id, current);
    assert_eq!(m.search.focus, SearchPanel::Files);
    m.mouse(MouseEventKind::Down(MouseButton::Left), x, y + 2);
    assert_eq!(m.search.focus, SearchPanel::Results);
    assert_eq!(m.search.results.current().unwrap().start.line, 405);
    insta::assert_snapshot!(
        "interactive_file_browser",
        FrameFixture::new(&m).render_size(120, 24)
    );
}

#[test]
fn paging_uses_the_list_viewport_and_restores_each_files_match_scroll() {
    let mut m = searched();
    m.large_file_results();
    m.update(Action::FocusNth(2));
    let frame = m.search_frame();
    let height = frame.lists[0].content.height as usize;
    m.on_key(key(KeyCode::PageDown));
    assert_eq!(
        m.search.results.current().unwrap().path,
        vvv_engine::RelPath::from(format!("src/file{height:02}.rs"))
    );
    m.on_key(key(KeyCode::PageUp));
    assert_eq!(
        m.search.results.current().unwrap().path.as_path(),
        std::path::Path::new("src/file00.rs")
    );
    m.update(Action::Enter);
    m.update(Action::Bottom);
    let id = m.search.results.current().unwrap().id.clone();
    let offset = m
        .search
        .results
        .files
        .match_viewports
        .get(std::path::Path::new("src/file00.rs"))
        .unwrap()
        .offset;
    assert!(offset > 0);
    m.update(Action::File(1));
    m.update(Action::File(-1));
    assert_eq!(m.search.results.current().unwrap().id, id);
    assert_eq!(
        m.search
            .results
            .files
            .match_viewports
            .get(std::path::Path::new("src/file00.rs"))
            .unwrap()
            .offset,
        offset
    );
    m.update(Action::Top);
    assert_eq!(m.search.results.current().unwrap().start.line, 0);
    m.update(Action::Move(-1));
    assert_eq!(m.search.results.current().unwrap().start.line, 0);
}

#[test]
fn wrapped_unicode_paths_map_every_visible_row_to_the_same_file() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut m = searched();
    let path = "src/语言/long-directory/another-directory/engine.rs";
    let matched = fx::m(path, 0, 0, "Thing", "Thing()");
    let id = matched.id.clone();
    m.search.results.replace(vec![matched]);
    m.search.selection_changed();
    m.on_event(Event::Viewport {
        width: 70,
        height: 24,
    });
    let frame = m.search_frame();
    let list = &frame.lists[0];
    assert!(list.rows.len() > 1);
    let (x, y) = (list.content.x, list.content.y + 1);
    m.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(m.search.focus, SearchPanel::Files);
    assert_eq!(m.search.results.current().unwrap().id, id);
    let frame = m.search_frame();
    let list = &frame.lists[0];
    m.mouse(
        MouseEventKind::Down(MouseButton::Left),
        list.content.x,
        list.content.y + list.content.height - 1,
    );
    assert_eq!(
        m.search.results.current().unwrap().id,
        id,
        "blank rows only change focus"
    );
}

#[test]
fn overlays_and_changed_results_reject_pointer_targets_from_an_old_frame() {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut m = searched();
    m.large_file_results();
    let frame = m.search_frame();
    let list = &frame.lists[0];
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: list.content.x,
        row: list.content.y + 1,
        modifiers: KeyModifiers::NONE,
    };
    let pointer = frame.pointer(event).unwrap();
    let id = m.search.results.current().unwrap().id.clone();
    m.update(Action::FocusNth(3));
    m.update(Action::Help);
    assert!(m.on_event(Event::Pointer(pointer.clone())).is_empty());
    assert_eq!(m.search.results.current().unwrap().id, id);
    m.update(Action::Back);
    m.search.results.replace(vec![]);
    assert!(m.on_event(Event::Pointer(pointer)).is_empty());
    assert!(m.search.results.current().is_none());
}

#[test]
fn filtering_keeps_layout_stable_and_cancellation_restores_manual_viewports() {
    use ratatui::crossterm::event::MouseEventKind;
    let mut m = searched();
    m.large_file_results();
    m.update(Action::FocusNth(2));
    let frame = m.search_frame();
    let list = &frame.lists[0];
    m.mouse(MouseEventKind::ScrollDown, list.content.x, list.content.y);
    let old = m.search.results.files.viewport.offset;
    let rect = m.search_frame().lists[0].area;
    m.update(Action::FilterFiles);
    typed(&mut m, "file19");
    assert_eq!(m.search_frame().lists[0].area, rect);
    m.update(Action::Back);
    assert_eq!(m.search.focus, SearchPanel::Files);
    assert_eq!(m.search.results.files.viewport.offset, old);
    m.update(Action::FilterFiles);
    typed(&mut m, "zzzz");
    m.update(Action::FocusNext);
    assert_eq!(m.search.focus, SearchPanel::Results);
    assert!(m.search.results.files.edit.is_none());
    m.update(Action::FocusNth(2));
    m.on_key(ctrl('u'));
    assert!(m.search.results.files.filter.is_empty());
    assert!(m.search.results.current().is_some());
}

#[test]
fn short_terminal_keeps_the_focused_list_browsable_and_geometry_in_bounds() {
    let mut m = searched();
    m.large_file_results();
    for (width, height) in [(1, 1), (20, 6), (40, 10), (90, 20)] {
        m.on_event(Event::Viewport { width, height });
        for focus in [2, 3, 4, 5] {
            m.update(Action::FocusNth(focus));
            FrameFixture::new(&m).render_size(width, height);
            for (_, area) in m.search_frame().panels {
                assert!(area.is_empty() || (area.right() <= width && area.bottom() < height));
            }
        }
    }
}
