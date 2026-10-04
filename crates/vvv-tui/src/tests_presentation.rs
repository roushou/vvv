// Presentation behavior.

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
fn snapshot_report_overlay() {
    let mut m = renaming();
    m.overlay = Some(Overlay::Report {
        report: Box::new(report()),
        cursor: 0,
    });
    insta::assert_snapshot!(FrameFixture::new(&m).render());
}

#[test]
fn dialogs_leave_the_bottom_bar_visible_and_show_their_own_controls() {
    let mut m = searched();
    m.update(Action::OpenMenu(MenuTarget::Filters));
    let frame = FrameFixture::new(&m).render();
    let (body, footer) = frame.rsplit_once('\n').unwrap();
    assert!(body.contains("search filters"));
    assert!(!body.contains("Ctrl+") && !body.contains("ctrl+"));
    assert!(footer.contains("⏎ edit") && footer.contains("ctrl+x clear"));
    m.update(Action::Back);
    m.update(Action::OpenMenu(MenuTarget::Language));
    let frame = FrameFixture::new(&m).render();
    assert!(frame.lines().last().unwrap().contains("⏎ choose"));
    m.update(Action::Back);
    m.update(Action::Places);
    for width in [50, 90, 120] {
        let frame = FrameFixture::new(&m).render_size(width, 12);
        let (body, footer) = frame.rsplit_once('\n').unwrap();
        assert!(body.contains("Places"));
        assert!(!body.contains("ctrl+") && !body.contains("tab switch"));
        assert!(
            footer.contains("⏎ open")
                && footer.contains("esc cancel")
                && footer.contains("f1 help")
        );
        assert!(!footer.contains("forget"));
    }
    m.update(Action::PlacesTab);
    let frame = FrameFixture::new(&m).render_size(120, 12);
    assert!(frame.lines().last().unwrap().contains("ctrl+d forget"));
    m.on_event(Event::Viewport {
        width: 50,
        height: 12,
    });
    m.update(Action::Help);
    m.on_key(key(KeyCode::End));
    let frame = FrameFixture::new(&m).render_size(50, 12);
    let (body, footer) = frame.rsplit_once('\n').unwrap();
    assert!(
        body.contains("ctrl+c"),
        "the final help row must remain reachable"
    );
    assert!(!body.contains("f1 / esc return"));
    assert!(footer.contains("f1 return"));
    m.update(Action::Help);
    m.update(Action::Back);
    m.on_event(Event::History(vec![history_entry(1), history_entry(2)]));
    m.update(Action::Undo);
    let frame = FrameFixture::new(&m).render();
    let (body, footer) = frame.rsplit_once('\n').unwrap();
    assert!(!body.contains("y/n"));
    assert!(footer.contains("y yes") && footer.contains("n no"));
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
#[test]
fn snapshot_search_deps() {
    let mut m = anchored();
    m.search.results.show_deps(fx::deps());
    m.search.selection_changed();
    insta::assert_snapshot!(FrameFixture::new(&m).render());
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

#[test]
fn help_excludes_shadowed_and_unavailable_keys_and_works_from_query_focus() {
    let mut m = searched();
    let rows = m
        .screen()
        .sections(m.focus(), |w| m.holds(w))
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .collect::<Vec<_>>();
    assert!(
        !rows
            .iter()
            .any(|r| r.binding.dispatch == crate::keymap::Dispatch::Run(Action::BrowseBack))
    );
    assert!(
        !rows
            .iter()
            .any(|r| r.binding.dispatch == crate::keymap::Dispatch::Run(Action::FilterFiles))
    );
    m.on_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert!(matches!(m.overlay, Some(Overlay::Help { .. })));
    m.on_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert!(m.overlay.is_none());
    assert_eq!(m.search.focus, SearchPanel::Query);
    m.on_event(Event::History(vec![history_entry(1)]));
    m.update(Action::FocusNth(2));
    let rows = m
        .screen()
        .sections(m.focus(), |w| m.holds(w))
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .collect::<Vec<_>>();
    let page = rows
        .iter()
        .find(|r| r.binding.dispatch == crate::keymap::Dispatch::Run(Action::Scroll(20)))
        .unwrap();
    assert!(!page.labels.split_whitespace().any(|k| k == "u"));
    assert!(page.labels.contains("pgup"));
}
