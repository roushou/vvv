// Input behavior.

#[test]
fn caret_motion_never_requests_work_and_paste_is_one_edit() {
    use crate::input::EditCommand;
    let mut m = searched();
    m.search.focus = SearchPanel::Query;
    let generation = m.generation;
    let selected = m.search.results.current().unwrap().id.clone();
    let preview = m.search.preview.clone();
    for command in [
        EditCommand::Home,
        EditCommand::Right,
        EditCommand::WordRight,
        EditCommand::WordLeft,
        EditCommand::End,
        EditCommand::Left,
    ] {
        assert!(m.update(Action::InputEdit(command)).is_empty());
        assert_eq!(m.generation, generation);
        assert_eq!(m.search.results.current().unwrap().id, selected);
        assert_eq!(m.search.preview, preview);
    }
    m.update(Action::InputEdit(EditCommand::Home));
    let effects = m.on_event(Event::Paste("Wide\r\nQuery\n".into()));
    assert_eq!(m.search.query.text(), "Wide QueryLanguage");
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::Search { .. }));
    for mut m in [renaming(), moving(), rewriting()] {
        let generation = m.generation;
        for command in [EditCommand::Home, EditCommand::WordRight, EditCommand::End] {
            assert!(m.update(Action::InputEdit(command)).is_empty());
            assert_eq!(m.generation, generation);
        }
        let effects = m.on_event(Event::Paste("LongDestination".into()));
        assert_eq!(
            effects
                .iter()
                .filter(|e| matches!(e, Effect::Plan { .. }))
                .count(),
            1
        );
    }
    let mut m = rewriting();
    m.on_key(ctrl('u'));
    let effects = m.on_event(Event::Paste("$BODY\r\nnext\n".into()));
    let Mode::Rewrite(rw) = &m.mode else { panic!() };
    assert_eq!(rw.template, "$BODY\nnext\n");
    assert_eq!(effects.len(), 1);
    insta::assert_snapshot!(
        "input_multiline_paste",
        FrameFixture::new(&m).render_size(50, 16)
    );
    let mut m = searched();
    m.search.focus = SearchPanel::Query;
    m.on_key(ctrl('u'));
    m.paste("symbol:trait name:ExtremelyLongSymbolInTheWorkspace crates/vvv/src/deeply/nested/destination.rs");
    m.update(Action::InputEdit(EditCommand::WordLeft));
    insta::assert_snapshot!(
        "input_long_window",
        FrameFixture::new(&m).render_size(50, 16)
    );
}

#[test]
fn obsolete_errors_are_ignored_and_partial_recovery_cannot_be_retried() {
    use crate::problem::Problem;
    use vvv_engine::{ErrorCode, Failure};
    let mut m = searched();
    let old = m.generation;
    m.search.focus = SearchPanel::Query;
    typed(&mut m, "Next");
    m.on_event(Event::Failed {
        generation: Some(old),
        problem: Box::new(Problem::new(
            Failure::new(ErrorCode::Io, "old failure"),
            Some(Effect::History),
        )),
    });
    assert!(m.problem().is_none());
    m.on_event(Event::Failed {
        generation: None,
        problem: Box::new(Problem::new(
            Failure::new(ErrorCode::Io, "wrong file"),
            Some(Effect::Preview {
                path: "unselected.rs".into(),
            }),
        )),
    });
    assert!(m.problem().is_none());
    let mut failure = Failure::new(ErrorCode::RecoveryFailed, "Rollback could not finish");
    failure.recovery = Some(vvv_engine::Recovery {
        cause: Box::new(Failure::new(
            ErrorCode::Io,
            "destination became unavailable",
        )),
        failures: vec![],
        remaining: vec![vvv_engine::RecoveryEffect {
            path: "src/remaining.rs".into(),
            expected: vvv_engine::RecoveryState::Absent,
            observed: vvv_engine::RecoveryState::Other,
        }],
        unverified: vec![vvv_engine::RecoveryUnverified {
            path: "src/unknown.rs".into(),
            expected: vvv_engine::RecoveryState::Absent,
            code: ErrorCode::Io,
            message: "read failed".into(),
        }],
    });
    m.on_event(Event::Failed {
        generation: None,
        problem: Box::new(Problem::new(failure.clone(), Some(Effect::Undo))),
    });
    assert_eq!(m.problem().unwrap().failure, failure);
    m.update(Action::FocusNth(4));
    assert!(m.on_key(ctrl('r')).is_empty());
    assert!(
        matches!(m.on_key(key(KeyCode::Char('e'))).as_slice(), [Effect::Edit { path, line: 0 }] if path.as_path() == std::path::Path::new("src/remaining.rs"))
    );
    insta::assert_snapshot!(
        "failure_partial_recovery",
        FrameFixture::new(&m).render_size(120, 26)
    );
}

#[test]
fn problem_bottom_and_scroll_up_show_the_previous_rows_immediately() {
    let mut m = searched();
    let hint = (1..=80)
        .map(|n| format!("Issue{n:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    m.on_event(Event::Failed {
        generation: None,
        problem: Box::new(crate::problem::Problem::new(
            vvv_engine::Failure::new(vvv_engine::ErrorCode::Io, "File read failed").with_hint(hint),
            None,
        )),
    });
    m.update(Action::FocusNth(4));
    m.update(Action::Bottom);
    let bottom = FrameFixture::new(&m).render_size(50, 16);
    assert!(bottom.contains("Issue80"));
    m.update(Action::Scroll(-1));
    assert_ne!(bottom, FrameFixture::new(&m).render_size(50, 16));
    m.update(Action::Top);
    assert!(
        FrameFixture::new(&m)
            .render_size(50, 16)
            .contains("File read failed")
    );
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
