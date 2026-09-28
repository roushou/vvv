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
    m.on_event(preview("src/lang/mod.rs", &refs));
    m
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
        None,
        "no control key is a mode accelerator"
    );
    assert_eq!(
        m.action_for(ctrl('n')),
        Some(Action::Move(1)),
        "walk the results without leaving the query"
    );
    assert_eq!(m.action_for(ctrl('p')), Some(Action::Move(-1)));
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
        m.action_for(key(KeyCode::Char('3'))),
        Some(Action::FocusNth(3))
    );
    assert_eq!(m.action_for(key(KeyCode::Char('e'))), Some(Action::Edit));
    assert_eq!(m.action_for(key(KeyCode::Char('v'))), Some(Action::View));

    m.update(Action::FocusNth(3));
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
fn the_results_cursor_stays_emphasised_while_the_query_has_the_focus() {
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
        cursor,
        "a cursor row carries the cursor style, not just dim"
    );
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
    let effects = m.update(Action::Move(1));
    assert!(
        matches!(&effects[..], [Effect::Preview { path }] if path.ends_with("registry.rs")),
        "{effects:?}"
    );
    let effects = m.update(Action::Move(1));
    assert!(
        matches!(&effects[..], [Effect::Preview { path }] if path.ends_with("lib.rs")),
        "a third file"
    );
    m.on_event(preview("src/lib.rs", &["a"]));
    assert!(
        m.update(Action::Move(1)).is_empty(),
        "same file, already shown"
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
        menu.current().value.as_deref(),
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
    m.update(Action::Move(1));
    assert!(m.update(Action::MoveSymbol).is_empty());
    assert!(matches!(&m.status.message, Some((Level::Error, _))));
    m.update(Action::Top);
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
    doc.block_body(Block::Line(Line::of(Role::Path, "src/a.rs")));
    doc.block_note(Block::Summary(Line::of(Role::Plain, "✓ #3  2 files")));
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
    m.update(Action::Back);
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
fn o_jumps_from_a_use_to_its_declaration() {
    let mut m = searched();
    m.update(Action::Enter); // into the results
    m.update(Action::Move(3)); // onto a use
    m.update(Action::Jump);
    assert_eq!(m.search.results.cursor.index, 0, "the declaration row");
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
    m.update(Action::FocusNth(4));
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
    assert!(m.search.results.body_declaration().is_some());
    m.update(Action::Move(2));
    assert!(m.search.results.body_declaration().is_none());
    assert!(m.search.results.has_body(), "space remains reserved");
}

#[test]
fn body_ambiguity_and_empty_results_do_not_leave_invisible_focus() {
    let mut m = searched();
    let mut other = m.search.results.matches[0].clone();
    other.path = "other.rs".into();
    m.search.results.matches.push(other);
    m.update(Action::Move(1));
    assert!(m.search.results.body_declaration().is_none());
    m.update(Action::FocusNth(4));
    m.search.results.replace(vec![]);
    m.search.selection_changed();
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusNth(4));
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.update(Action::FocusPrev);
    m.update(Action::FocusPrev);
    assert_eq!(m.search.focus, SearchPanel::Context);
}

#[test]
fn snapshot_body_focused() {
    let mut m = searched();
    m.update(Action::FocusNth(4));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn body_tracks_declarations_and_requests_both_files_when_needed() {
    let mut m = searched();
    m.search.preview = None;
    m.search.body.clear();
    m.update(Action::Move(1));
    let effects = m.search.preview_effect();
    assert_eq!(effects.len(), 2);
    assert!(matches!(&effects[0], Effect::Preview { path }
        if path.as_path() == std::path::Path::new("src/lang/registry.rs")));
    assert!(matches!(&effects[1], Effect::Preview { path }
        if path.as_path() == std::path::Path::new("src/lang/mod.rs")));
    m.update(Action::Move(-1));
    assert_eq!(m.search.preview_effect().len(), 1, "same file read once");

    let text = "struct Other {\n    value: usize,\n}\nfn outside() {}";
    let mut declaration = fx::decl("other.rs", 0, SymbolKind::Struct, "Other", "struct Other {");
    declaration.symbol.as_mut().unwrap().span =
        vvv_engine::Span::new(0, text.find("\nfn").unwrap());
    m.search.results.matches.push(declaration);
    m.update(Action::Move(4));
    assert!(
        m.search
            .body
            .lines(m.search.results.current().unwrap())
            .is_none()
    );
    m.on_event(Event::Previewed {
        symbols: vec![],
        path: "other.rs".into(),
        text: text.into(),
        highlights: vec![],
    });
    assert_eq!(
        m.search.body.lines(m.search.results.current().unwrap()),
        Some(0..3)
    );
    let frame = FrameFixture::new(&m).render();
    let body_rows = frame
        .lines()
        .skip_while(|line| !line.contains("definition"))
        .take(7)
        .map(|line| line.chars().take(45).collect::<String>())
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
    m.on_event(Event::Previewed {
        symbols: vec![],
        path: "large.rs".into(),
        text,
        highlights: vec![],
    });
    m.update(Action::FocusNth(4));
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
    let source = m.search.body.preview.clone().unwrap();
    m.search.body.clear();
    let event = Event::Previewed {
        symbols: vec![],
        path: source.path.clone(),
        text: source.text().to_owned(),
        highlights: source.highlights.clone(),
    };
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
    assert_eq!(loading[(2, 12)].symbol(), " ");
    m.on_event(event.clone());
    let loaded = draw(&m);
    assert_eq!(loaded[(1, 12)].symbol(), " ");
    assert_eq!(loaded[(2, 12)].symbol(), "p");
    assert_eq!(
        loaded[(6, 13)].symbol(),
        "f",
        "source indentation is preserved"
    );
    assert_eq!(loaded[(1, 11)], loading[(1, 11)], "the border stays put");
    m.on_event(event);
    assert_eq!(
        draw(&m),
        loaded,
        "a repeated file response cannot shift the text"
    );
    m.update(Action::FocusNth(4));
    m.on_key(key(KeyCode::Down));
    let scrolled = draw(&m);
    assert_eq!(scrolled[(6, 12)].symbol(), "f");
    assert_eq!(scrolled[(1, 12)].symbol(), " ");
}

#[test]
fn nested_definitions_keep_the_same_alignment_when_loaded_and_scrolled() {
    let mut m = searched();
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
                .nth(12)
                .unwrap()
                .chars()
                .take(45)
                .all(|c| c == '│' || c == ' ')
        );
        assert!(!loading.contains("Loading"));
        m.on_event(Event::Previewed {
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
                .nth(12)
                .unwrap()
                .starts_with("│ fn nested() {"),
            "indent {indent:?}: {loaded}"
        );
        assert!(
            loaded
                .lines()
                .nth(13)
                .unwrap()
                .starts_with("│     nested();")
        );
        assert!(loaded.lines().nth(14).unwrap().starts_with("│ }"));
        if indent == "        " {
            insta::assert_snapshot!("definition_nested", loaded);
        }
        m.update(Action::FocusNth(4));
        m.update(Action::Scroll(1));
        let scrolled = FrameFixture::new(&m).render();
        assert!(
            scrolled
                .lines()
                .nth(12)
                .unwrap()
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
    let loading = FrameFixture::new(&m).render();
    assert!(
        loading
            .lines()
            .nth(12)
            .unwrap()
            .chars()
            .take(45)
            .all(|c| c == '│' || c == ' ')
    );
    assert!(!loading.contains("Loading"));
    let event = Event::Previewed {
        symbols: vec![parent, variant],
        path: "error.rs".into(),
        text: text.into(),
        highlights: vec![],
    };
    m.on_event(event.clone());
    let loaded = FrameFixture::new(&m).render();
    assert!(
        loaded
            .lines()
            .nth(12)
            .unwrap()
            .starts_with("│ enum Error {")
    );
    assert!(loaded.lines().nth(15).unwrap().starts_with("│     Engine("));
    m.update(Action::FocusNth(4));
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 0 }]
        if path.as_path() == std::path::Path::new("error.rs"))
    );
    m.update(Action::FocusNth(1));
    m.on_event(event);
    assert_eq!(FrameFixture::new(&m).render(), loaded);
    let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
    terminal
        .draw(|frame| App::new(&m, Painter::colored(), 0).render(frame.area(), frame.buffer_mut()))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(6, 15)].symbol(), "E");
    let highlighted = terminal.backend().buffer()[(6, 15)].style();
    assert_eq!(highlighted.fg, Painter::colored().hit.fg);
    assert_eq!(
        highlighted.add_modifier,
        Painter::colored().hit.add_modifier
    );
    assert!(loaded.lines().nth(13).unwrap().starts_with("│     Io,"));
    assert!(loaded.lines().nth(16).unwrap().starts_with("│     Editor,"));
    assert!(loaded.lines().nth(17).unwrap().starts_with("│ }"));
    insta::assert_snapshot!("definition_enum_variant", loaded);
    let mut sibling = fx::decl("error.rs", 1, SymbolKind::Variant, "Io", "    Io,");
    let sibling_start = text.find("Io,").unwrap();
    sibling.symbol.as_mut().unwrap().span = vvv_engine::Span::new(sibling_start, sibling_start + 2);
    sibling.symbol.as_mut().unwrap().name_span = sibling.symbol.as_ref().unwrap().span;
    m.search.results.matches.push(sibling);
    assert!(
        m.update(Action::Move(1)).is_empty(),
        "the enum source is already cached"
    );
    assert_eq!(
        m.search
            .body
            .symbol(m.search.results.current().unwrap())
            .unwrap()
            .name,
        "Error"
    );
    m.update(Action::FocusNth(4));
    m.update(Action::Bottom);
    assert_eq!(m.search.body.scroll, 5, "scroll bounds cover the full enum");
}

#[test]
fn definition_keeps_its_complete_frame_until_the_selected_file_arrives() {
    let mut m = searched();
    let pending = "struct Pending {\n    value: usize,\n}";
    let final_text = "struct Final {\n    ready: bool,\n}";
    for (path, name, text) in [
        ("pending.rs", "Pending", pending),
        ("final.rs", "Final", final_text),
    ] {
        let mut declaration = fx::decl(
            path,
            0,
            SymbolKind::Struct,
            name,
            text.lines().next().unwrap(),
        );
        declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(0, text.len());
        m.search.results.matches.push(declaration);
    }
    m.update(Action::FocusNth(4));
    m.update(Action::Scroll(1));
    let body = |model: &Model| {
        FrameFixture::new(model)
            .render()
            .lines()
            .skip(11)
            .take(8)
            .map(|line| line.chars().take(45).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let before = body(&m);
    let effects = m.update(Action::Move(4));
    assert!(matches!(effects.as_slice(), [Effect::Preview { path }]
        if path.as_path() == std::path::Path::new("pending.rs")));
    assert_eq!(
        body(&m),
        before,
        "retain source, title, highlight and scroll together"
    );
    assert!(!body(&m).contains("Loading"));
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 63 }]
        if path.as_path() == std::path::Path::new("src/lang/mod.rs")),
        "editor follows the displayed source"
    );
    insta::assert_snapshot!("definition_pending", FrameFixture::new(&m).render());

    m.update(Action::Move(1));
    m.on_event(Event::Previewed {
        path: "pending.rs".into(),
        text: pending.into(),
        highlights: vec![],
        symbols: vec![],
    });
    assert_eq!(
        body(&m),
        before,
        "a superseded reply must not flash on screen"
    );
    m.on_event(Event::Previewed {
        path: "final.rs".into(),
        text: final_text.into(),
        highlights: vec![],
        symbols: vec![],
    });
    assert_eq!(m.search.body.scroll, 0);
    assert!(body(&m).contains("struct Final"));
    assert!(!body(&m).contains("Language"));
    assert!(!body(&m).contains("Loading"));
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 0 }]
        if path.as_path() == std::path::Path::new("final.rs"))
    );
}

#[test]
fn same_file_definition_switches_immediately_without_fetching() {
    let mut m = searched();
    let text = "struct First {}\nstruct Second {}";
    let second_start = text.find("struct Second").unwrap();
    let declarations = [
        ("First", 0, second_start - 1),
        ("Second", second_start, text.len()),
    ]
    .into_iter()
    .enumerate()
    .map(|(line, (name, start, end))| {
        let mut declaration = fx::decl(
            "both.rs",
            line as u32,
            SymbolKind::Struct,
            name,
            &text[start..end],
        );
        declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(start, end);
        declaration
    })
    .collect();
    m.search.results.replace(declarations);
    m.search.selection_changed();
    m.on_event(Event::Previewed {
        path: "both.rs".into(),
        text: text.into(),
        highlights: vec![],
        symbols: vec![],
    });
    assert_eq!(
        m.search
            .body
            .declaration()
            .unwrap()
            .symbol
            .as_ref()
            .unwrap()
            .name,
        "First"
    );
    assert!(m.update(Action::Move(1)).is_empty());
    assert_eq!(
        m.search
            .body
            .declaration()
            .unwrap()
            .symbol
            .as_ref()
            .unwrap()
            .name,
        "Second"
    );
    let frame = FrameFixture::new(&m).render();
    assert!(
        frame
            .lines()
            .nth(12)
            .unwrap()
            .starts_with("│ struct Second {}")
    );
    assert!(!frame.contains("Loading"));
}
