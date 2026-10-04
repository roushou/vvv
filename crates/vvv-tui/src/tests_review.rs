// Review behavior.

#[test]
fn structured_stale_failure_retains_review_and_rebuilds_without_applying() {
    use crate::problem::Problem;
    let mut m = renaming();
    typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: m.generation,
        planned: rename_plan(),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    let ticks = r.ticks.clone();
    let changes = r.changes.clone();
    let intent = match m.update(Action::Enter).pop().unwrap() {
        Effect::Commit { intent, .. } => intent,
        _ => panic!(),
    };
    let failure = vvv_engine::Failure::new(
        vvv_engine::ErrorCode::Stale,
        "src/lang/mod.rs changed after review",
    )
    .with_hint("Read the current source before applying.");
    m.on_event(Event::Failed {
        generation: None,
        problem: Box::new(Problem::new(
            failure.clone(),
            Some(Effect::Commit {
                generation: m.generation,
                intent,
            }),
        )),
    });
    assert_eq!(m.problem().unwrap().failure, failure);
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.name, "Lang");
    assert_eq!(r.ticks, ticks);
    assert_eq!(
        serde_json::to_value(&r.changes).unwrap(),
        serde_json::to_value(&changes).unwrap()
    );
    assert!(m.update(Action::Enter).is_empty());
    insta::assert_snapshot!(
        "failure_stale_review",
        FrameFixture::new(&m).render_size(110, 24)
    );
    assert!(
        FrameFixture::new(&m)
            .render_size(50, 20)
            .lines()
            .last()
            .unwrap()
            .contains("ctrl+r")
    );
    let effects = m.on_key(ctrl('r'));
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0],
        Effect::Plan {
            debounce: false,
            ..
        }
    ));
    assert!(m.on_key(ctrl('r')).is_empty());
    assert!(m.update(Action::Enter).is_empty());
    m.on_event(Event::Planned {
        generation: m.generation,
        planned: rename_plan(),
    });
    assert!(m.problem().is_none());
    assert!(matches!(
        m.update(Action::Enter).as_slice(),
        [Effect::Commit { .. }]
    ));
}

#[test]
fn workspace_preview_errors_keep_the_filter_focused_and_clear_for_another_file() {
    use crate::problem::Problem;
    let mut m = searched();
    m.on_key(ctrl('b'));
    m.on_event(Event::WorkspaceFiles {
        generation: m.generation,
        paths: vec!["a.rs".into(), "b.rs".into()],
    });
    m.on_event(Event::Failed {
        generation: None,
        problem: Box::new(Problem::new(
            vvv_engine::Failure::new(vvv_engine::ErrorCode::Io, "a.rs cannot be read"),
            Some(Effect::Preview {
                path: "a.rs".into(),
            }),
        )),
    });
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert!(m.problem().is_some());
    let effects = m.on_event(Event::Paste("b".into()));
    assert!(
        matches!(effects.as_slice(), [Effect::Preview { path }] if path.as_path() == std::path::Path::new("b.rs"))
    );
    assert!(m.problem().is_none());
    m.on_event(preview("b.rs", &["valid source"]));
    assert!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .text()
            .contains("valid source")
    );
}

#[test]
fn an_unavailable_initial_plan_offers_source_inspection_without_retrying() {
    use crate::problem::Problem;
    let mut m = searched();
    m.update(Action::Rename);
    let failure = vvv_engine::Failure::new(
        vvv_engine::ErrorCode::NoLayout,
        "This language cannot follow module paths",
    )
    .with_hint("Inspect the declaration and its uses.");
    m.on_event(Event::PlanFailed {
        generation: m.generation,
        problem: Box::new(Problem::new(failure.clone(), Some(Effect::History))),
    });
    assert_eq!(m.problem().unwrap().failure, failure);
    assert!(!m.problem().unwrap().can_retry());
    assert!(m.on_key(ctrl('r')).is_empty());
    m.update(Action::FocusNth(5));
    assert!(
        matches!(m.on_key(key(KeyCode::Char('e'))).as_slice(), [Effect::Edit { path, .. }] if path.as_path() == std::path::Path::new("src/lang/mod.rs"))
    );
    insta::assert_snapshot!(
        "failure_unavailable_capability",
        FrameFixture::new(&m).render()
    );
}

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
    m.on_event(Event::Planned {
        generation: m.generation,
        planned: rename_plan(),
    });

    let effects = m.update(Action::Enter);
    match &effects[..] {
        [
            Effect::Commit {
                intent: Intent::Rename(i),
                generation,
            },
        ] => {
            assert_eq!(*generation, m.generation);
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
        problem: Box::new(crate::problem::Problem::new(
            vvv_engine::Failure::new(
                vvv_engine::ErrorCode::Unmovable,
                "src/lang/mod.rsx: not addressable",
            ),
            None,
        )),
    });
    let Mode::Move(mv) = &m.mode else { panic!() };
    assert!(mv.plan.is_none());
    assert!(
        mv.error
            .as_ref()
            .unwrap()
            .message()
            .contains("not addressable")
    );
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
            intent: Intent::Move(_),
            ..
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
    m.on_event(Event::Planned {
        generation: m.generation,
        planned: Planned::Rewrite {
            files: rewrite_files(),
        },
    });
    let effects = m.update(Action::Enter);
    match &effects[..] {
        [
            Effect::Commit {
                intent: Intent::Rewrite(i),
                generation,
            },
        ] => {
            assert_eq!(*generation, m.generation);
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
fn unified_filters_edit_clear_and_reset_without_discarding_the_query_or_preview_choice() {
    use crate::modes::search::{Category, filters::Restriction};
    let mut m = searched();
    for panel in [
        SearchPanel::Query,
        SearchPanel::Files,
        SearchPanel::Results,
        SearchPanel::Context,
        SearchPanel::Body,
    ] {
        m.search.focus = panel;
        assert_eq!(
            m.action_for(ctrl('g')),
            Some(Action::OpenMenu(MenuTarget::Filters))
        );
    }
    m.search.focus = SearchPanel::Results;
    m.search.focus_nth(5);
    m.search.query.set_filter(Filter::Lang, Some("rust"));
    m.search.query.set_filter(Filter::Kind, Some("trait_item"));
    m.search.results.set_category(Category::Declarations);
    m.search.results.files.filter = "mod".into();
    let id = m.search.results.current().unwrap().id.clone();
    m.search.preview_scroll = Some(12);
    m.update(Action::OpenMenu(MenuTarget::Filters));
    insta::assert_snapshot!(
        "unified_filters",
        FrameFixture::new(&m).render_size(120, 24)
    );
    typed(&mut m, "Language");
    m.update(Action::MenuChoose);
    assert!(matches!(&m.overlay, Some(Overlay::Menu(menu)) if menu.target == MenuTarget::Language));
    typed(&mut m, "zzzz");
    m.on_key(ctrl('u'));
    assert!(matches!(&m.overlay, Some(Overlay::Menu(menu)) if menu.filter.is_empty()));
    assert_eq!(m.search.query.filter(Filter::Lang), Some("rust"));
    let effects = m.on_key(ctrl('x'));
    assert!(m.overlay.is_none());
    assert!(m.search.query.filter(Filter::Lang).is_none());
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(m.search.results.current().unwrap().id, id);
    assert_eq!(m.search.preview_scroll, Some(12));
    assert!(m.search.definition_tab);
    m.update(Action::OpenMenu(MenuTarget::Filters));
    typed(&mut m, "Reset");
    let effects = m.update(Action::MenuChoose);
    assert!(
        Restriction::ALL
            .iter()
            .all(|r| r.value(&m.search).is_none())
    );
    assert_eq!(
        m.search.query.parse().unwrap(),
        vvv_engine::Query::pattern("Language")
    );
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(m.search.results.current().unwrap().id, id);
    assert_eq!(m.search.preview_scroll, Some(12));
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert!(m.search.definition_tab);
}

#[test]
fn preview_find_is_local_cancellable_and_independent_in_each_pane() {
    let mut m = searched();
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    m.update(Action::FocusNth(4));
    let query = m.search.query.text().to_owned();
    let generation = m.generation;
    let selected = m.search.results.current().unwrap().id.clone();
    assert!(m.on_key(key(KeyCode::Char('/'))).is_empty());
    assert!(m.search.input_focused());
    assert_eq!(
        m.action_for(key(KeyCode::Char('5'))),
        Some(Action::Input('5'))
    );
    assert!(typed(&mut m, "fn").is_empty());
    assert_eq!(m.search.inspection.hits.len(), 2);
    assert_eq!(m.search.inspection.cursor, Some(0));
    assert_eq!(m.search.preview_scroll, Some(64));
    insta::assert_snapshot!(
        "source_find_input",
        FrameFixture::new(&m).render_size(120, 30)
    );
    m.on_key(key(KeyCode::Enter));
    m.on_key(key(KeyCode::Char('n')));
    assert_eq!(m.search.preview_scroll, Some(65));
    assert!(matches!(
        m.update(Action::Edit).as_slice(),
        [Effect::Edit { line: 65, .. }]
    ));
    m.on_key(key(KeyCode::Char('N')));
    assert_eq!(m.search.preview_scroll, Some(64));
    let scroll = m.search.preview_scroll;
    let horizontal = m.search.inspection.horizontal;
    m.on_key(key(KeyCode::Char('/')));
    typed(&mut m, "missing");
    assert!(m.search.inspection.hits.is_empty());
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.inspection.term, "fn");
    assert_eq!(m.search.preview_scroll, scroll);
    assert_eq!(m.search.inspection.horizontal, horizontal);
    m.update(Action::FocusNth(5));
    m.on_key(key(KeyCode::Char('/')));
    typed(&mut m, "Language");
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.body.inspection.hits.len(), 1);
    assert_eq!(
        m.search.body.inspection.horizontal, 0,
        "visible hits keep columns stable"
    );
    assert_eq!(m.search.inspection.term, "fn");
    assert_eq!(m.search.body.inspection.term, "Language");
    insta::assert_snapshot!(
        "definition_find",
        FrameFixture::new(&m).render_size(120, 30)
    );
    assert_eq!(m.search.query.text(), query);
    assert_eq!(m.generation, generation);
    assert_eq!(m.search.results.current().unwrap().id, selected);
}

#[test]
fn preview_line_jumps_validate_absolute_lines_and_editor_uses_the_inspected_site() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    m.on_key(key(KeyCode::Char(':')));
    typed(&mut m, "3");
    m.on_key(key(KeyCode::Enter));
    assert!(
        m.search
            .body
            .inspection
            .edit
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    assert_eq!(m.search.body.scroll, 0);
    insta::assert_snapshot!("definition_invalid_line", FrameFixture::new(&m).render());
    m.on_key(ctrl('u'));
    typed(&mut m, "66");
    m.on_key(key(KeyCode::Enter));
    assert!(m.search.body.inspection.edit.is_none());
    assert_eq!(m.search.body.scroll, 2);
    assert!(
        matches!(m.update(Action::Edit).as_slice(), [Effect::Edit { path, line: 65 }] if path.as_path() == std::path::Path::new("src/lang/mod.rs"))
    );
    m.update(Action::FocusNth(4));
    m.on_key(key(KeyCode::Char(':')));
    typed(&mut m, "0");
    m.on_key(key(KeyCode::Enter));
    assert!(m.search.inspection.edit.is_some());
    m.on_key(ctrl('u'));
    typed(&mut m, "70");
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.preview_scroll, Some(69));
    assert!(matches!(
        m.update(Action::Edit).as_slice(),
        [Effect::Edit { line: 69, .. }]
    ));
    m.on_key(key(KeyCode::Char(':')));
    typed(&mut m, "abc");
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.preview_scroll, Some(69));
}

#[test]
fn preview_expansion_restores_results_and_keeps_independent_positions_after_resize() {
    let mut m = searched();
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    let original = m.search_frame().panels;
    let selected = m.search.results.current().unwrap().id.clone();
    m.update(Action::FocusNth(4));
    m.update(Action::Scroll(3));
    let source_scroll = m.search.preview_scroll;
    m.on_key(key(KeyCode::Char('z')));
    assert_eq!(m.search.expanded, Some(SearchPanel::Context));
    let expanded = m.search_frame();
    assert!(expanded.panels[1].1.is_empty() && expanded.panels[2].1.is_empty());
    assert_eq!(expanded.panels[3].1.width, 120);
    insta::assert_snapshot!(
        "source_expanded",
        FrameFixture::new(&m).render_size(120, 30)
    );
    m.on_key(key(KeyCode::Char('p')));
    assert_eq!(m.search.expanded, Some(SearchPanel::Body));
    m.update(Action::Scroll(1));
    assert_eq!(m.search.body.scroll, 1);
    m.on_event(Event::Viewport {
        width: 70,
        height: 18,
    });
    insta::assert_snapshot!(
        "definition_expanded_narrow",
        FrameFixture::new(&m).render_size(70, 18)
    );
    m.on_key(key(KeyCode::Esc));
    assert!(m.search.expanded.is_none());
    assert_eq!(m.search.focus, SearchPanel::Body);
    m.on_event(Event::Viewport {
        width: 120,
        height: 30,
    });
    assert_eq!(m.search_frame().panels, original);
    assert_eq!(m.search.body.scroll, 1);
    assert_eq!(m.search.preview_scroll, source_scroll);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.on_key(key(KeyCode::Char('z')));
    m.on_key(key(KeyCode::Char('3')));
    assert_eq!(m.search.focus, SearchPanel::Results);
    assert!(m.search.expanded.is_none());
}

#[test]
fn horizontal_preview_navigation_preserves_focus_and_resets_on_new_displayed_files() {
    let mut m = searched();
    m.update(Action::FocusNth(4));
    let selected = m.search.results.current().unwrap().id.clone();
    m.on_key(key(KeyCode::Right));
    assert_eq!(m.search.inspection.horizontal, 8);
    assert_eq!(m.search.focus, SearchPanel::Context);
    insta::assert_snapshot!("source_horizontal", FrameFixture::new(&m).render());
    m.on_key(key(KeyCode::Left));
    assert_eq!(m.search.inspection.horizontal, 0);
    m.on_key(key(KeyCode::Char('l')));
    m.on_key(key(KeyCode::Char('0')));
    assert_eq!(m.search.inspection.horizontal, 0);
    m.update(Action::FocusNth(5));
    m.on_key(key(KeyCode::Right));
    assert_eq!(m.search.body.inspection.horizontal, 8);
    assert_eq!(m.search.inspection.horizontal, 0);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.update(Action::FocusNth(3));
    m.update(Action::File(1));
    m.update(Action::FocusNth(4));
    m.on_key(key(KeyCode::Right));
    let old = m.search.inspection.horizontal;
    m.on_event(preview("src/lib.rs", &["wrong reply"]));
    assert_eq!(
        m.search.inspection.horizontal, old,
        "superseded replies cannot change inspection state"
    );
    m.on_event(preview("src/lang/registry.rs", &["use super::Language;"]));
    assert_eq!(m.search.inspection.horizontal, 0);
}

#[test]
fn expanded_preview_preserves_manually_scrolled_file_and_match_lists() {
    use ratatui::crossterm::event::MouseEventKind;
    let mut m = searched();
    m.large_file_results();
    let frame = m.search_frame();
    for list in frame.lists {
        m.mouse(MouseEventKind::ScrollDown, list.content.x, list.content.y);
    }
    let file_offset = m.search.results.files.viewport.offset;
    let path = m.search.results.current().unwrap().path.clone();
    let match_offset = m.search.results.files.match_viewports[&path].offset;
    assert!(file_offset > 0 && match_offset > 0);
    m.update(Action::FocusNth(4));
    m.update(Action::ExpandPreview);
    m.on_event(Event::Viewport {
        width: 80,
        height: 20,
    });
    assert_eq!(m.search.results.files.viewport.offset, file_offset);
    assert_eq!(
        m.search.results.files.match_viewports[&path].offset,
        match_offset
    );
    m.update(Action::Back);
    assert_eq!(m.search.results.files.viewport.offset, file_offset);
    assert_eq!(
        m.search.results.files.match_viewports[&path].offset,
        match_offset
    );
}

#[test]
fn split_keys_resize_in_opposite_directions_from_lists_and_previews() {
    let mut m = searched();
    for panel in [2, 3, 4, 5] {
        m.update(Action::FocusNth(panel));
        let split = m.split;
        m.on_key(key(KeyCode::Char('<')));
        assert_eq!(m.split, split - 5);
        m.on_key(key(KeyCode::Char('>')));
        assert_eq!(m.split, split);
    }
}

#[test]
fn review_selection_replans_exact_ids_and_blocks_apply_until_the_latest_preview() {
    use crate::modes::review::ReviewState;
    let mut m = renaming();
    let effects = typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m.update(Action::FocusNth(3));
    let old = m.generation;
    let effects = m.update(Action::Toggle);
    let generation = generation_of(&effects);
    let Mode::Rename(r) = &m.mode else { panic!() };
    let ticks = r.ticks.clone();
    assert_eq!(ticks.len(), 2);
    assert_eq!(r.state(), ReviewState::Planning);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Plan { intent: Intent::Rename(intent), .. } if intent.selection == Selection::Ids(ticks.clone()))));
    assert!(m.update(Action::Enter).is_empty());
    m.on_event(Event::Planned {
        generation: old,
        planned: rename_plan(),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.state(), ReviewState::Planning);
    let mut planned = rename_plan();
    if let Planned::Rename {
        files, occurrences, ..
    } = &mut planned
    {
        files.retain(|file| {
            occurrences
                .iter()
                .any(|o| o.m.path == file.path && ticks.contains(&o.m.id))
        });
    }
    m.on_event(Event::Planned {
        generation,
        planned,
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.state(), ReviewState::Ready);
    assert_eq!(r.changes.len(), 2);
    assert!(matches!(
        m.update(Action::Enter).as_slice(),
        [Effect::Commit {
            intent: Intent::Rename(_),
            ..
        }]
    ));
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.state(), ReviewState::Applying);
    let name = r.name.clone();
    assert!(m.update(Action::Input('x')).is_empty());
    assert!(m.update(Action::ToggleAll).is_empty());
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.name, name);
    assert_eq!(r.ticks, ticks);
    insta::assert_snapshot!("review_applying", FrameFixture::new(&m).render());
}

#[test]
fn rewrite_preview_selection_and_clear_invalidate_pending_plans() {
    use crate::modes::review::ReviewState;
    let mut m = rewriting();
    m.update(Action::FocusNth(2));
    let effects = m.update(Action::Toggle);
    let generation = generation_of(&effects);
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    let ticks = rw.ticks.clone();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Plan { intent: Intent::Rewrite(intent), .. } if intent.selection == Selection::Ids(ticks.clone()))));
    assert!(m.update(Action::Enter).is_empty());
    m.update(Action::ToggleAll);
    let zero = m.update(Action::ToggleAll);
    assert!(zero.is_empty());
    m.on_event(Event::Planned {
        generation,
        planned: Planned::Rewrite {
            files: rewrite_files(),
        },
    });
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    assert!(rw.changes.is_empty() && rw.ticks.is_empty());
    assert_eq!(rw.state(), ReviewState::Empty("select matches"));
    insta::assert_snapshot!("review_excluded_rewrite", FrameFixture::new(&m).render());
    m.update(Action::FocusNth(1));
    m.on_key(ctrl('u'));
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    assert!(rw.template.is_empty());
    assert_eq!(rw.state(), ReviewState::Input("type a template"));
}

#[test]
fn failed_review_never_applies_the_previous_preview_and_clear_recovers_the_input() {
    use crate::modes::review::ReviewState;
    let mut m = renaming();
    let effects = typed(&mut m, "Lang");
    m.on_event(Event::Planned {
        generation: generation_of(&effects),
        planned: rename_plan(),
    });
    m.on_event(Event::PlanFailed {
        generation: m.generation,
        problem: Box::new(crate::problem::Problem::new(
            vvv_engine::Failure::new(
                vvv_engine::ErrorCode::Conflict,
                "name collides with an existing declaration",
            ),
            None,
        )),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert!(
        !r.changes.is_empty(),
        "retain the last preview for inspection"
    );
    assert!(matches!(r.state(), ReviewState::Failed(_)));
    assert!(m.update(Action::Enter).is_empty());
    insta::assert_snapshot!("review_invalid_rename", FrameFixture::new(&m).render());
    m.on_key(ctrl('u'));
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert!(r.name.is_empty() && r.error.is_none());
    assert_eq!(r.state(), ReviewState::Input("type a new name"));
    assert!(m.status.message.is_none());
    let mut m = moving();
    m.on_key(ctrl('u'));
    let Mode::Move(mv) = &m.mode else { panic!() };
    assert!(mv.to.is_empty() && mv.plan.is_none());
    assert_eq!(mv.state(), ReviewState::Input("type a destination"));
}

#[test]
fn review_details_follow_the_last_list_and_ignore_superseded_source_replies() {
    let mut m = renaming();
    m.update(Action::FocusNth(3));
    m.update(Action::Move(2));
    let Mode::Rename(r) = &m.mode else { panic!() };
    let id = r.current().unwrap().m.id.clone();
    m.update(Action::FocusNth(5));
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(r.current().unwrap().m.id, id);
    assert_eq!(r.list(), RenamePanel::Sure);
    m.on_event(preview("src/other.rs", &["first", "second"]));
    m.update(Action::Scroll(1));
    m.on_event(preview("unselected.rs", &["wrong reply"]));
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert_eq!(
        r.preview.as_ref().unwrap().path.as_path(),
        std::path::Path::new("src/other.rs")
    );
    assert_eq!(r.detail_scroll, 1);
    let mut m = moving();
    m.update(Action::FocusNth(4));
    let Mode::Move(mv) = &m.mode else { panic!() };
    let site = mv.site();
    m.update(Action::FocusNth(5));
    let Mode::Move(mv) = &m.mode else { panic!() };
    assert_eq!(mv.list(), MovePanel::Notices);
    assert_eq!(mv.site(), site);
    insta::assert_snapshot!(
        "review_manual_narrow",
        FrameFixture::new(&m).render_size(70, 14)
    );
}

#[test]
fn text_home_and_end_scroll_review_content_without_changing_its_selected_site() {
    for mut m in [renaming(), moving(), rewriting()] {
        let detail = if matches!(m.mode, Mode::Rewrite(_)) {
            3
        } else {
            5
        };
        m.update(Action::FocusNth(detail));
        let site = match &m.mode {
            Mode::Rename(r) => r.site(),
            Mode::Move(mv) => mv.site(),
            Mode::Rewrite(rw) => rw.site(),
            _ => unreachable!(),
        };
        m.on_key(key(KeyCode::End));
        let current = match &m.mode {
            Mode::Rename(r) => r.site(),
            Mode::Move(mv) => mv.site(),
            Mode::Rewrite(rw) => rw.site(),
            _ => unreachable!(),
        };
        assert_eq!(current, site);
        FrameFixture::new(&m).render(); // Rendering the saturated end offset must not overflow.
        m.on_key(key(KeyCode::Home));
        let scroll = match &m.mode {
            Mode::Rename(r) => r.detail_scroll,
            Mode::Move(mv) => mv.detail_scroll,
            Mode::Rewrite(rw) => rw.detail_scroll,
            _ => unreachable!(),
        };
        assert_eq!(scroll, 0);
    }
    let mut m = searched();
    let mut newest = history_entry(2);
    newest.files = 2;
    newest.paths = vec![
        "crates/世界/src/very_long_directory/original.rs".into(),
        "crates/世界/src/very_long_directory/other.rs".into(),
    ];
    newest.moves = vec![(
        newest.paths[0].clone(),
        "crates/世界/src/renamed_directory/renamed.rs".into(),
    )];
    m.on_event(Event::History(vec![history_entry(1), newest]));
    m.on_key(key(KeyCode::Char('2')));
    assert_eq!(
        m.action_for(key(KeyCode::Char('d'))),
        Some(Action::Scroll(20))
    );
    m.on_key(key(KeyCode::End));
    let Mode::History(h) = &m.mode else { panic!() };
    assert_eq!(h.current().unwrap().id, 2);
    assert_eq!(h.files_scroll, usize::MAX);
    m.on_key(key(KeyCode::Home));
    insta::assert_snapshot!(
        "review_history_paths",
        FrameFixture::new(&m).render_size(90, 20)
    );
    m.update(Action::FocusNth(1));
    m.update(Action::Move(-1));
    assert!(m.update(Action::Undo).is_empty());
    assert!(m.overlay.is_none());
    let rendered = FrameFixture::new(&m).render();
    assert!(rendered.contains("cannot undo") && rendered.contains("newest entry only"));
}

#[test]
fn rename_input_during_initial_judgment_still_loads_engine_defaults() {
    let mut m = searched();
    let first = m.update(Action::Rename);
    let old = generation_of(&first);
    let typing = m.update(Action::Input('L'));
    assert!(
        matches!(typing.as_slice(), [Effect::Plan { intent: Intent::Rename(intent), .. }] if intent.to == "L" && intent.selection == Selection::All)
    );
    let clearing = m.update(Action::Clear);
    let generation = generation_of(&clearing);
    assert!(
        matches!(clearing.as_slice(), [Effect::Plan { intent: Intent::Rename(intent), .. }] if intent.to == "Language" && intent.selection == Selection::All)
    );
    m.on_event(Event::Planned {
        generation: old,
        planned: rename_plan(),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert!(r.busy && !r.judged);
    m.on_event(Event::Planned {
        generation,
        planned: rename_plan(),
    });
    let Mode::Rename(r) = &m.mode else { panic!() };
    assert!(r.judged && !r.busy && r.name.is_empty());
    assert_eq!(r.ticks.len(), 3);
    let typing = m.update(Action::Input('L'));
    assert!(
        matches!(typing.as_slice(), [Effect::Plan { intent: Intent::Rename(intent), .. }] if matches!(&intent.selection, Selection::Ids(ids) if ids.len() == 3))
    );
}
