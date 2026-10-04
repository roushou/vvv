// Search behavior.

#[test]
fn local_filter_caret_motion_preserves_selection_and_never_queries_the_engine() {
    use crate::input::EditCommand;
    let mut m = searched();
    m.update(Action::FocusNth(2));
    m.update(Action::FilterFiles);
    m.on_event(Event::Paste("lang".into()));
    let generation = m.generation;
    let id = m.search.results.current().unwrap().id.clone();
    assert!(m.update(Action::InputEdit(EditCommand::Home)).is_empty());
    assert_eq!(m.search.results.current().unwrap().id, id);
    assert_eq!(m.generation, generation);
    m.update(Action::Back);
    m.on_key(ctrl('g'));
    assert!(m.on_event(Event::Paste("lang".into())).is_empty());
    let Overlay::Menu(menu) = m.overlay.as_mut().unwrap() else {
        panic!()
    };
    menu.cursor = 1;
    assert!(m.update(Action::InputEdit(EditCommand::Home)).is_empty());
    let Overlay::Menu(menu) = m.overlay.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(menu.cursor, 1);
}

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
fn esc_returns_to_search_with_the_rows_intact() {
    let mut m = renaming();
    m.update(Action::Back);
    assert!(matches!(m.mode, Mode::Search));
    assert_eq!(m.search.results.matches.len(), 4);
    assert_eq!(m.search.query.text(), "Language");
}

// ------------------------------------------------------------------ move

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
    assert!(frame.contains("Files · filter") && frame.contains("files: lib"));
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
fn clearing_hidden_selection_restores_it_until_the_user_deliberately_navigates() {
    let mut m = searched();
    let original = m.search.results.current().unwrap().id.clone();
    m.search.focus = SearchPanel::Results;
    m.update(Action::OpenMenu(MenuTarget::Category));
    typed(&mut m, "Imports");
    m.update(Action::MenuChoose);
    assert_ne!(m.search.results.current().unwrap().id, original);
    m.update(Action::OpenMenu(MenuTarget::Category));
    m.on_key(ctrl('x'));
    assert_eq!(m.search.results.current().unwrap().id, original);
    m.update(Action::OpenMenu(MenuTarget::Category));
    typed(&mut m, "Imports");
    m.update(Action::MenuChoose);
    m.update(Action::File(1));
    let deliberate = m.search.results.current().unwrap().id.clone();
    m.update(Action::OpenMenu(MenuTarget::Category));
    m.on_key(ctrl('x'));
    assert_eq!(m.search.results.current().unwrap().id, deliberate);

    let all = m.search.results.matches.as_ref().clone();
    m.update(Action::OpenMenu(MenuTarget::Location));
    typed(&mut m, "src/lang");
    let effects = m.update(Action::MenuChoose);
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: all
            .iter()
            .filter(|hit| hit.path.starts_with(std::path::Path::new("src/lang")))
            .cloned()
            .collect(),
        skipped: vec![],
    });
    assert_ne!(m.search.results.current().unwrap().id, deliberate);
    m.update(Action::OpenMenu(MenuTarget::Location));
    let effects = m.on_key(ctrl('x'));
    m.on_event(Event::Searched {
        generation: generation_of(&effects),
        matches: all,
        skipped: vec![],
    });
    assert_eq!(m.search.results.current().unwrap().id, deliberate);
}

#[test]
fn picker_counts_are_loaded_search_hits_and_never_hide_zero_count_choices() {
    let mut m = searched();
    m.search
        .results
        .set_category(crate::modes::search::Category::Uses);
    m.search.results.files.filter = "missing".into();
    m.update(Action::OpenMenu(MenuTarget::Category));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!()
    };
    assert_eq!(
        menu.items.iter().map(|i| i.count).collect::<Vec<_>>(),
        vec![Some(4), Some(1), Some(2), Some(1)]
    );
    insta::assert_snapshot!("filter_category_counts", FrameFixture::new(&m).render());
    m.update(Action::OpenMenu(MenuTarget::Symbol));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!()
    };
    assert_eq!(
        menu.items
            .iter()
            .find(|i| i.value.as_deref() == Some("struct"))
            .unwrap()
            .count,
        Some(0)
    );
    typed(&mut m, "struct");
    assert!(
        matches!(m.update(Action::MenuChoose).as_slice(), [Effect::Search { query, .. }] if query.symbol() == Some(vvv_engine::SymbolKind::Struct))
    );
    m.update(Action::OpenMenu(MenuTarget::Language));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!()
    };
    assert!(
        !menu.counted,
        "pending search must not advertise old counts as current"
    );
    assert!(menu.items.iter().all(|i| i.count.is_none()));
}

#[test]
fn empty_results_explain_restrictions_and_long_metadata_keeps_complete_paths() {
    let mut m = searched();
    m.search.focus = SearchPanel::Results;
    m.update(Action::OpenMenu(MenuTarget::Category));
    typed(&mut m, "Uses");
    m.update(Action::MenuChoose);
    m.search
        .results
        .replace(vec![fx::search().matches[0].clone()]);
    let frame = FrameFixture::new(&m).render();
    assert!(frame.contains("No uses in loaded results"));
    insta::assert_snapshot!("filtered_category_empty", frame);
    m.search
        .locations
        .select(Some("crates/very-long-package/src/语言/nested/modules"))
        .unwrap();
    m.search
        .results
        .set_location(m.search.locations.selected.clone());
    m.search.query.set_filter(Filter::Symbol, Some("trait"));
    m.search.query.set_filter(Filter::Lang, Some("rust"));
    m.search.query.set_filter(Filter::Kind, Some("trait_item"));
    m.search.results.files.filter = "lang mod".into();
    m.search.results.replace(vec![]);
    insta::assert_snapshot!(
        "filtered_search_empty_narrow",
        FrameFixture::new(&m).render_size(70, 24)
    );
    m.update(Action::OpenMenu(MenuTarget::Location));
    typed(&mut m, "zzzz");
    insta::assert_snapshot!(
        "typed_location_path",
        FrameFixture::new(&m).render_size(70, 24)
    );
    m.update(Action::OpenMenu(MenuTarget::Language));
    typed(&mut m, "zzzz");
    insta::assert_snapshot!("picker_no_choices", FrameFixture::new(&m).render());
}

#[test]
fn filter_menu_only_offers_file_browsing_in_views_with_a_file_list() {
    let mut m = searched();
    m.search.results.entered(fx::references());
    m.search.results.files.filter = "lib".into();
    m.search.results.relation = Relation::Impact;
    m.update(Action::OpenMenu(MenuTarget::Filters));
    let Some(Overlay::Menu(menu)) = &m.overlay else {
        panic!()
    };
    assert!(
        !menu
            .items
            .iter()
            .any(|item| item.value.as_deref() == Some("files"))
    );
    assert!(!FrameFixture::new(&m).render().contains("files: lib"));
    typed(&mut m, "Reset");
    m.update(Action::MenuChoose);
    assert!(
        m.search.results.files.filter.is_empty(),
        "reset also clears filters retained from another view"
    );
}
