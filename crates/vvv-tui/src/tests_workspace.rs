// Workspace behavior.

#[test]
fn workspace_browsing_filters_files_and_returns_to_the_exact_search() {
    use crate::input::EditCommand;
    let mut m = searched();
    m.search.focus = SearchPanel::Query;
    m.update(Action::InputEdit(EditCommand::Home));
    let before = crate::modes::search::browse::NavigationEntry::capture(&m.search).label();
    let query = m.search.query.clone();
    let results = m.search.results.clone();
    let preview_before = m.search.preview.clone();
    let effects = m.on_key(ctrl('b'));
    assert!(matches!(
        effects.as_slice(),
        [Effect::WorkspaceFiles { .. }]
    ));
    let generation = m.generation;
    let effects = m.on_event(Event::WorkspaceFiles {
        generation,
        paths: vec!["src/a.rs".into(), "src/b.rs".into(), "README.md".into()],
    });
    assert!(
        matches!(effects.as_slice(), [Effect::Preview { path }] if path.as_path() == std::path::Path::new("README.md"))
    );
    m.on_event(preview("README.md", &["# Workspace"]));
    assert!(m.search.workspace.as_ref().unwrap().symbols().is_empty());
    let effects = m.on_event(Event::Paste("sra".into()));
    assert!(
        matches!(effects.as_slice(), [Effect::Preview { path }] if path.as_path() == std::path::Path::new("src/a.rs"))
    );
    assert_eq!(m.generation, generation);
    assert!(m.update(Action::InputEdit(EditCommand::Home)).is_empty());
    m.on_event(preview("src/b.rs", &["wrong file"]));
    assert_eq!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .path
            .as_path(),
        std::path::Path::new("README.md")
    );
    let text = "struct Alpha {\n    value: i32,\n}\nfn next() {}";
    let symbol = vvv_engine::Symbol::plain(
        SymbolKind::Struct,
        "Alpha",
        vvv_engine::Span::new(7, 12),
        vvv_engine::Span::new(0, 31),
    );
    m.on_event(Event::Previewed {
        path: "src/a.rs".into(),
        text: text.into(),
        symbols: vec![symbol],
        highlights: vec![],
        identifiers: vec![],
    });
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.focus, SearchPanel::Files);
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.focus, SearchPanel::Results);
    insta::assert_snapshot!(
        "workspace_outline",
        FrameFixture::new(&m).render_size(110, 25)
    );
    let follow = m.on_key(key(KeyCode::Enter));
    assert!(
        matches!(follow.as_slice(), [Effect::Follow { query, .. }] if matches!(&query.origin, vvv_engine::NavigationOrigin::Occurrence { anchor } if anchor.path.as_path() == std::path::Path::new("src/a.rs") && anchor.span == vvv_engine::Span::new(7, 12)))
    );
    let reply = DefinitionFixture::new("Alpha", "src/a.rs", text).reply();
    m.on_event(FollowFixture { effects: follow }.reply(Ok(reply)));
    assert!(m.search.workspace.is_none());
    m.update(Action::BrowseBack);
    assert_eq!(m.search.workspace.as_ref().unwrap().filter, "sra");
    assert_eq!(m.search.focus, SearchPanel::Results);
    m.on_key(ctrl('b'));
    assert!(m.search.workspace.is_none());
    assert_eq!(m.search.query, query);
    assert_eq!(
        m.search.results.current().unwrap().id,
        results.current().unwrap().id
    );
    assert_eq!(m.search.preview, preview_before);
    assert_eq!(
        crate::modes::search::browse::NavigationEntry::capture(&m.search).label(),
        before
    );
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert!(
        m.on_event(Event::WorkspaceFiles {
            generation,
            paths: vec![]
        })
        .is_empty()
    );
}

#[test]
fn workspace_filter_editing_and_escape_are_bound_at_every_focus() {
    use crate::input::EditCommand;
    let mut m = searched();
    m.on_key(ctrl('b'));
    m.on_event(Event::WorkspaceFiles {
        generation: m.generation,
        paths: vec!["a.rs".into(), "b.rs".into()],
    });
    m.paste("b");
    assert_eq!(
        m.action_for(key(KeyCode::Backspace)),
        Some(Action::Backspace)
    );
    assert_eq!(
        m.action_for(key(KeyCode::Home)),
        Some(Action::InputEdit(EditCommand::Home))
    );
    m.on_key(key(KeyCode::Backspace));
    assert!(m.search.workspace.as_ref().unwrap().filter.is_empty());
    m.paste("b");
    m.on_key(ctrl('u'));
    assert!(m.search.workspace.as_ref().unwrap().filter.is_empty());
    for focus in 1..=4 {
        m.update(Action::FocusNth(focus));
        assert_eq!(m.action_for(key(KeyCode::Esc)), Some(Action::Back));
        if focus > 1 {
            assert_eq!(m.action_for(key(KeyCode::Char('v'))), None);
        }
    }
    m.on_key(key(KeyCode::Esc));
    assert!(m.search.workspace.is_none());
}

#[test]
fn workspace_loading_keeps_the_shown_source_scroll_highlight_and_editor_site() {
    let mut m = searched();
    m.on_key(ctrl('b'));
    m.on_event(Event::WorkspaceFiles {
        generation: m.generation,
        paths: vec!["a.rs".into(), "b.rs".into()],
    });
    let text = numbered(50, &[(1, "struct Alpha {}")]).join("\n");
    m.on_event(Event::Previewed {
        path: "a.rs".into(),
        text,
        highlights: vec![],
        identifiers: vec![],
        symbols: vec![vvv_engine::Symbol::plain(
            SymbolKind::Struct,
            "Alpha",
            vvv_engine::Span::new(7, 12),
            vvv_engine::Span::new(0, 15),
        )],
    });
    m.update(Action::FocusNth(4));
    m.update(Action::Scroll(20));
    let before = m.search.workspace.as_ref().unwrap().scroll;
    let marked = m.search.workspace.as_ref().unwrap().marked();
    m.update(Action::FocusNth(2));
    m.update(Action::Move(1));
    let workspace = m.search.workspace.as_ref().unwrap();
    assert!(workspace.loading);
    assert_eq!(workspace.scroll, before);
    assert_eq!(workspace.marked(), marked);
    m.update(Action::FocusNth(4));
    assert!(
        matches!(m.on_key(key(KeyCode::Char('e'))).as_slice(), [Effect::Edit { path, .. }] if path.as_path() == std::path::Path::new("a.rs"))
    );
    m.on_event(preview("b.rs", &["new source"]));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, 0);
    assert!(
        matches!(m.on_key(key(KeyCode::Char('e'))).as_slice(), [Effect::Edit { path, .. }] if path.as_path() == std::path::Path::new("b.rs"))
    );
    m.update(Action::FocusNth(2));
    m.update(Action::Move(-1));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, 0);
    let lines = numbered(50, &[]);
    m.on_event(preview(
        "a.rs",
        &lines.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, before);
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
fn workspace_preferences_restore_only_layout_and_available_recipes() {
    use crate::preferences::Preferences;
    let mut m = searched();
    m.split = 65;
    m.view = ReportView::Detailed;
    m.search.definition_tab = true;
    m.search.locations.select(Some("crates/vvv")).unwrap();
    m.search.results.files.filter = "lang".into();
    let bytes = Preferences::capture(&m).encode();
    assert!(Preferences::decode(&bytes, "different-workspace").is_none());
    assert!(Preferences::decode(b"invalid json", &m.root).is_none());
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["recent"]["entries"].as_array_mut().unwrap().push(serde_json::json!({ "query": "bad symbol:nope", "location": "../outside", "category": "all", "files": "" }));
    let mut restored = model();
    Preferences::decode(&serde_json::to_vec(&json).unwrap(), &m.root)
        .unwrap()
        .restore(&mut restored);
    assert_eq!(restored.split, 65);
    assert_eq!(restored.view, ReportView::Detailed);
    assert!(restored.search.definition_tab);
    assert!(restored.search.query.is_empty());
    assert!(restored.search.locations.selected.is_none());
    assert!(restored.search.results.files.filter.is_empty());
    assert!(restored.search.recent.entries().iter().all(|r| r.valid()));
    assert!(
        restored
            .search
            .locations
            .choices()
            .contains(&"crates/vvv".into())
    );
    restored.update(Action::Places);
    assert_eq!(restored.action_for(ctrl('l')), Some(Action::ResetLayout));
    restored.on_key(ctrl('l'));
    assert_eq!(restored.split, 50);
    assert_eq!(restored.view, ReportView::Compact);
    assert!(!restored.search.definition_tab);
    assert!(!restored.search.recent.entries().is_empty());
    json["split"] = 99.into();
    assert!(Preferences::decode(&serde_json::to_vec(&json).unwrap(), &m.root).is_none());
}

#[test]
fn workspace_source_inspection_is_local_cancellable_and_retained_per_file() {
    let mut fixture = WorkspaceFixture::new();
    let m = &mut fixture.model;
    m.update(Action::FocusNth(4));
    let generation = m.generation;
    assert_eq!(
        m.action_for(key(KeyCode::Char('/'))),
        Some(Action::InspectFind)
    );
    assert_eq!(
        m.action_for(key(KeyCode::Char(':'))),
        Some(Action::InspectLine)
    );
    assert!(m.on_key(key(KeyCode::Char('/'))).is_empty());
    assert!(m.input_focused());
    assert!(m.paste("needle_31").is_empty());
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, 31);
    assert!(m.search.workspace.as_ref().unwrap().inspection.horizontal > 0);
    let term = m.search.workspace.as_ref().unwrap().inspection.term.clone();
    assert!(m.on_key(key(KeyCode::Home)).is_empty());
    assert_eq!(m.search.workspace.as_ref().unwrap().inspection.term, term);
    assert_eq!(m.generation, generation);
    m.on_key(key(KeyCode::Enter));
    assert!(!m.input_focused());
    m.on_key(key(KeyCode::Char(':')));
    m.paste("999");
    m.on_key(key(KeyCode::Enter));
    assert!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .inspection
            .edit
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    m.on_key(ctrl('u'));
    m.paste("12");
    m.on_key(key(KeyCode::Enter));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, 11);
    assert_eq!(m.search.site().unwrap().1, 11);
    m.on_key(key(KeyCode::Char('/')));
    m.on_key(ctrl('u'));
    m.paste("needle_04");
    m.on_key(key(KeyCode::Esc));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, 11);
    assert_eq!(m.search.workspace.as_ref().unwrap().inspection.term, term);
    m.on_key(key(KeyCode::Char('0')));
    assert_eq!(
        m.search.workspace.as_ref().unwrap().inspection.horizontal,
        0
    );
    m.on_key(key(KeyCode::Right));
    assert_eq!(
        m.search.workspace.as_ref().unwrap().inspection.horizontal,
        8
    );
    insta::assert_snapshot!(
        "workspace_source_find",
        FrameFixture::new(m).render_size(110, 25)
    );
    m.on_key(key(KeyCode::Char('z')));
    assert!(m.search.workspace.as_ref().unwrap().expanded);
    insta::assert_snapshot!(
        "workspace_source_expanded",
        FrameFixture::new(m).render_size(70, 18)
    );
    m.on_key(key(KeyCode::Esc));
    assert!(!m.search.workspace.as_ref().unwrap().expanded);
    assert!(m.search.workspace.is_some());
    m.update(Action::FocusNth(2));
    m.update(Action::File(1));
    fixture.preview("src/b.rs");
    assert!(fixture.browse().inspection.term.is_empty());
    fixture.model.update(Action::File(-1));
    fixture.preview("src/a.rs");
    assert_eq!(fixture.browse().scroll, 11);
    assert_eq!(fixture.browse().inspection.term, term);
    assert_eq!(fixture.browse().inspection.horizontal, 8);
    fixture.model.update(Action::FocusNth(4));
    fixture.model.on_key(key(KeyCode::Home));
    let frame = fixture.model.search_frame();
    let source = frame
        .panels
        .iter()
        .find(|(panel, _)| *panel == SearchPanel::Context)
        .unwrap()
        .1;
    let page = source.height.saturating_sub(2) as usize;
    assert!(fixture.model.on_key(key(KeyCode::PageDown)).is_empty());
    assert_eq!(fixture.browse().scroll, page);
    assert!(fixture.model.on_key(key(KeyCode::PageUp)).is_empty());
    assert_eq!(fixture.browse().scroll, 0);
}

#[test]
fn workspace_outline_filter_preserves_hidden_selection_and_can_cancel_or_clear() {
    let mut fixture = WorkspaceFixture::new();
    let m = &mut fixture.model;
    m.update(Action::FocusNth(3));
    m.update(Action::Move(10));
    let before = m.search.workspace.as_ref().unwrap().clone();
    let generation = m.generation;
    assert_eq!(
        m.action_for(key(KeyCode::Char('/'))),
        Some(Action::FilterOutline)
    );
    m.on_key(key(KeyCode::Char('/')));
    assert!(m.input_focused());
    assert!(m.paste("tgt 语言").is_empty());
    assert_eq!(
        m.search.workspace.as_ref().unwrap().symbol().unwrap().name,
        "target_语言"
    );
    assert_eq!(m.search.workspace.as_ref().unwrap().outline.index, 31);
    let scroll = m.search.workspace.as_ref().unwrap().scroll;
    m.on_key(key(KeyCode::Home));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, scroll);
    assert_eq!(m.generation, generation);
    insta::assert_snapshot!(
        "workspace_outline_filter",
        FrameFixture::new(m).render_size(110, 25)
    );
    m.on_key(key(KeyCode::End));
    m.paste("zzzz");
    assert!(m.search.workspace.as_ref().unwrap().symbol().is_none());
    m.on_key(ctrl('n'));
    m.on_key(ctrl('u'));
    assert_eq!(
        m.search.workspace.as_ref().unwrap().outline.index,
        before.outline.index
    );
    m.on_key(key(KeyCode::Esc));
    assert!(!m.input_focused());
    assert_eq!(
        m.search.workspace.as_ref().unwrap().outline.index,
        before.outline.index
    );
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, before.scroll);
    assert!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .outline_filter
            .is_empty()
    );
    m.on_key(key(KeyCode::Char('/')));
    m.paste("zzzz");
    assert!(m.search.workspace.as_ref().unwrap().query().is_none());
    m.on_key(key(KeyCode::Enter));
    assert!(!m.input_focused());
    m.on_event(Event::Viewport {
        width: 70,
        height: 18,
    });
    insta::assert_snapshot!(
        "workspace_outline_empty",
        FrameFixture::new(m).render_size(70, 18)
    );
    assert!(m.on_key(ctrl('u')).is_empty());
    assert_eq!(
        m.search.workspace.as_ref().unwrap().outline.index,
        before.outline.index
    );
    assert!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .outline_filter
            .is_empty()
    );
    m.on_key(key(KeyCode::Char('/')));
    m.paste("item");
    m.on_key(ctrl('n'));
    let chosen = m.search.workspace.as_ref().unwrap().outline.index;
    m.on_key(key(KeyCode::Tab));
    assert!(!m.input_focused());
    assert!(m.search.workspace.as_ref().unwrap().outline_edit.is_none());
    m.update(Action::FocusNth(3));
    m.on_key(ctrl('u'));
    assert_eq!(m.search.workspace.as_ref().unwrap().outline.index, chosen);
}

#[test]
fn workspace_mouse_scroll_keeps_focus_selection_and_uses_stable_outline_targets() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut fixture = WorkspaceFixture::new();
    let m = &mut fixture.model;
    m.update(Action::FocusNth(2));
    m.update(Action::File(1));
    fixture.preview("src/b.rs");
    let m = &mut fixture.model;
    m.update(Action::FocusNth(1));
    m.paste("zzzz");
    assert!(m.search.workspace.as_ref().unwrap().file.is_none());
    m.on_key(ctrl('n'));
    m.on_key(ctrl('u'));
    assert_eq!(
        m.search
            .workspace
            .as_ref()
            .unwrap()
            .file
            .as_ref()
            .unwrap()
            .as_path(),
        std::path::Path::new("src/b.rs")
    );
    fixture.preview("src/b.rs");
    let m = &mut fixture.model;
    let before = m.search.workspace.as_ref().unwrap().outline.index;
    let frame = m.search_frame();
    let list = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Results)
        .unwrap();
    let click = frame
        .pointer(ratatui::crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: list.content.x,
            row: list.content.y + 2,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    let wheel = frame
        .pointer(ratatui::crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: list.content.x,
            row: list.content.y,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    assert!(m.on_event(Event::Pointer(wheel)).is_empty());
    assert_eq!(m.search.focus, SearchPanel::Query);
    assert_eq!(m.search.workspace.as_ref().unwrap().outline.index, before);
    assert!(m.search.workspace.as_ref().unwrap().outline_viewport.offset > 0);
    m.on_event(Event::Pointer(click.clone()));
    assert_eq!(m.search.focus, SearchPanel::Results);
    assert_eq!(m.search.workspace.as_ref().unwrap().outline.index, 2);
    m.update(Action::FilterOutline);
    m.paste("target");
    m.on_event(Event::Pointer(click));
    assert_eq!(
        m.search.workspace.as_ref().unwrap().outline.index,
        31,
        "obsolete geometry cannot select hidden symbols"
    );
    m.on_key(key(KeyCode::Enter));
    m.on_key(ctrl('u'));
    m.update(Action::FocusNth(1));
    let frame = m.search_frame();
    let area = frame
        .panels
        .iter()
        .find(|(p, _)| *p == SearchPanel::Context)
        .unwrap()
        .1;
    let source_wheel = frame
        .pointer(ratatui::crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: area.x + 1,
            row: area.y + 1,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    let scroll = m.search.workspace.as_ref().unwrap().scroll;
    m.on_event(Event::Pointer(source_wheel.clone()));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, scroll + 3);
    assert_eq!(m.search.focus, SearchPanel::Query);
    m.update(Action::Help);
    m.on_event(Event::Pointer(source_wheel));
    assert_eq!(m.search.workspace.as_ref().unwrap().scroll, scroll + 3);
}

#[test]
fn workspace_wrapped_paths_and_small_layouts_keep_the_focused_list_usable() {
    use crate::modes::search::files::PointerIntent;
    let mut fixture = WorkspaceFixture::new();
    let m = &mut fixture.model;
    let paths: Vec<vvv_engine::RelPath> = (0..20)
        .map(|i| {
            format!("crates/世界/very_long_directory/another_directory/source_{i:02}.rs").into()
        })
        .collect();
    m.on_event(Event::WorkspaceFiles {
        generation: m.generation,
        paths: paths.clone(),
    });
    m.update(Action::FocusNth(2));
    m.on_event(Event::Viewport {
        width: 90,
        height: 18,
    });
    let frame = m.search_frame();
    let list = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Files)
        .unwrap();
    let first: Vec<_> = list
        .rows
        .iter()
        .take_while(|row| matches!(row, PointerIntent::File(path) if path == &paths[0]))
        .collect();
    assert!(
        first.len() >= 2,
        "full relative paths wrap without losing identity"
    );
    let old = m.search.workspace.as_ref().unwrap().file.clone();
    let page_files = list
        .rows
        .iter()
        .skip(list.offset)
        .take(list.content.height as usize)
        .filter_map(|row| {
            if let PointerIntent::File(path) = row {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    m.update(Action::Page(1));
    assert_ne!(m.search.workspace.as_ref().unwrap().file, old);
    assert_eq!(
        m.search.workspace.as_ref().unwrap().file.as_ref(),
        Some(&paths[page_files])
    );
    insta::assert_snapshot!(
        "workspace_long_paths",
        FrameFixture::new(m).render_size(90, 18)
    );
    m.on_event(Event::Viewport {
        width: 50,
        height: 12,
    });
    let frame = m.search_frame();
    let files = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Files)
        .unwrap();
    let outline = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Results)
        .unwrap();
    assert!(files.content.height > 0);
    assert_eq!(outline.area.height, 1);
    insta::assert_snapshot!(
        "workspace_narrow_files",
        FrameFixture::new(m).render_size(50, 12)
    );
    let frame = m.search_frame();
    let list = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Files)
        .unwrap();
    let selected = m.search.workspace.as_ref().unwrap().file.clone().unwrap();
    let continuation = list
        .rows
        .iter()
        .enumerate()
        .skip(list.offset)
        .take(list.content.height as usize)
        .rfind(|(_, row)| matches!(row, PointerIntent::File(path) if *path == selected))
        .unwrap()
        .0;
    let click = frame
        .pointer(ratatui::crossterm::event::MouseEvent {
            kind: ratatui::crossterm::event::MouseEventKind::Down(
                ratatui::crossterm::event::MouseButton::Left,
            ),
            column: list.content.x,
            row: list.content.y + (continuation - list.offset) as u16,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    m.on_event(Event::Pointer(click));
    assert_eq!(
        m.search.workspace.as_ref().unwrap().file.as_ref(),
        Some(&selected)
    );
    let wheel = m
        .search_frame()
        .pointer(ratatui::crossterm::event::MouseEvent {
            kind: ratatui::crossterm::event::MouseEventKind::ScrollDown,
            column: list.content.x,
            row: list.content.y,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    let offset = m.search.workspace.as_ref().unwrap().files_viewport.offset;
    m.on_event(Event::Pointer(wheel));
    assert!(m.search.workspace.as_ref().unwrap().files_viewport.offset > offset);
    assert_eq!(
        m.search.workspace.as_ref().unwrap().file.as_ref(),
        Some(&selected)
    );
    assert_eq!(m.search.focus, SearchPanel::Files);
    m.update(Action::Move(1));
    let frame = m.search_frame();
    let list = frame
        .lists
        .iter()
        .find(|list| list.panel == SearchPanel::Files)
        .unwrap();
    assert!(list.rows.iter().skip(list.offset).take(list.content.height as usize)
        .any(|row| matches!(row, PointerIntent::File(path) if Some(path) == m.search.workspace.as_ref().unwrap().file.as_ref())));
    m.update(Action::FocusNth(3));
    let frame = m.search_frame();
    assert!(
        frame
            .lists
            .iter()
            .find(|list| list.panel == SearchPanel::Results)
            .unwrap()
            .content
            .height
            > 0
    );
    assert_eq!(
        frame
            .lists
            .iter()
            .find(|list| list.panel == SearchPanel::Files)
            .unwrap()
            .area
            .height,
        1
    );
    for (width, height) in [(20, 8), (50, 8), (90, 8), (110, 12)] {
        m.on_event(Event::Viewport { width, height });
        for focus in 1..=4 {
            m.update(Action::FocusNth(focus));
            let rendered = FrameFixture::new(m).render_size(width, height);
            assert!(rendered.contains("f1"));
        }
    }
}
