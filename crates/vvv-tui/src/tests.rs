//! The TUI without a terminal: `update` with plain assertions, every mode
//! rendered into a `TestBackend` and snapshotted.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::Widget;
use vvv_engine::{Intent, Selection};

use super::action::{Action, Effect, Event, Planned};
use super::model::{
    Level, MenuTarget, Mode, Model, MovePanel, Overlay, RenamePanel, ReportView, RewritePanel,
    SearchPanel,
};
use super::query::Filter;
use crate::fixtures as fx;
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
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: fx::search().matches,
        skipped: vec![],
    });
    let lines = numbered(70, &[(64, "pub trait Language: Send + Sync {")]);
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
            addresses: mv.from_address.zip(mv.to_address),
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

fn render(model: &Model) -> String {
    let backend = TestBackend::new(90, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| App::new(model, Painter::plain()).render(f.area(), f.buffer_mut()))
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
        .draw(|f| App::new(&m, Painter::plain()).render(f.area(), f.buffer_mut()))
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
    let before = render(&m);
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
            assert_eq!((i.name.as_str(), i.to.as_str()), ("Language", "Language"));
            assert_eq!(
                i.declared_in.as_deref(),
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
    let lines = render(&m);
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
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_search() {
    let mut m = searched();
    m.update(Action::Enter);
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_search_detailed() {
    let mut m = searched();
    m.update(Action::Enter);
    m.view = ReportView::Detailed;
    insta::assert_snapshot!(render(&m));
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
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_empty_search() {
    insta::assert_snapshot!(render(&model()));
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
    insta::assert_snapshot!(render(&m));
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
    let lines = render(&m);
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
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_move() {
    let mut m = moving();
    m.update(Action::FocusNth(2));
    let lines = numbered(5, &[(1, "use crate::util::parse::X;")]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lib.rs", &refs));
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_move_detailed() {
    let mut m = moving();
    m.view = ReportView::Detailed;
    m.update(Action::FocusNth(2));
    let lines = numbered(5, &[(1, "use crate::util::parse::X;")]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    m.on_event(preview("src/lib.rs", &refs));
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_move_structural_row_shows_the_hunk() {
    let mut m = moving();
    m.update(Action::FocusNth(3));
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_rewrite() {
    let mut m = rewriting();
    m.update(Action::FocusNth(2));
    m.update(Action::Move(1));
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_rewrite_detailed() {
    let mut m = rewriting();
    m.view = ReportView::Detailed;
    m.update(Action::FocusNth(2));
    m.update(Action::Move(1));
    insta::assert_snapshot!(render(&m));
}

#[test]
fn snapshot_history_and_confirm() {
    let mut m = searched();
    m.on_event(Event::History(vec![history_entry(1), history_entry(2)]));
    let history = render(&m);
    m.update(Action::Undo);
    let confirm = render(&m);
    insta::assert_snapshot!(format!("{history}\n\n=== confirm ===\n{confirm}"));
}

#[test]
fn snapshot_help() {
    let mut m = searched();
    m.update(Action::Help);
    insta::assert_snapshot!(render(&m));
}
