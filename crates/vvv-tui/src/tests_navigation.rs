// Navigation behavior.

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
fn passive_search_key_hints_only_appear_in_the_bottom_bar_and_follow_focus() {
    let mut m = searched();
    for (focus, hint) in [
        (SearchPanel::Query, "⏎ results"),
        (SearchPanel::Files, "⏎ matches"),
        (SearchPanel::Results, "⏎ references"),
        (SearchPanel::Context, "j/k scroll"),
        (SearchPanel::Body, "j/k scroll"),
    ] {
        m.search.focus = focus;
        for width in [50, 90, 120] {
            let frame = FrameFixture::new(&m).render_size(width, 20);
            let (body, footer) = frame.rsplit_once('\n').unwrap();
            assert!(footer.contains(hint), "{focus:?}, {width}: {footer}");
            assert!(footer.contains("f1 help"));
            for key in [
                "ctrl+",
                "Ctrl+",
                "j/k",
                "alt+←",
                "alt+→",
                "R relation",
                "t category",
                "z restore",
            ] {
                assert!(!body.contains(key), "hint {key:?} outside footer: {body}");
            }
            if focus == SearchPanel::Query && width >= 90 {
                assert!(footer.contains("ctrl+g filters") && footer.contains("ctrl+o places"));
                assert!(
                    !footer.contains("j/k"),
                    "typing must not advertise list keys"
                );
            }
        }
    }
    m.search.focus = SearchPanel::Query;
    // The long file-cycling hint cannot fit here; a shorter filter hint still can.
    let frame = FrameFixture::new(&m).render_size(45, 20);
    assert!(frame.lines().last().unwrap().contains("ctrl+g filters"));
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
fn snapshot_search_definition() {
    let mut m = anchored();
    m.search.results.show_definition(fx::explanation());
    m.search.selection_changed();
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
fn snapshot_body_focused() {
    let mut m = searched();
    m.update(Action::FocusNth(5));
    insta::assert_snapshot!(FrameFixture::new(&m).render());
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
    assert!(
        m.search.body.declaration().is_some(),
        "keep the last definition while showing the failure"
    );
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
    let frame = FrameFixture::new(&m).render_size(50, 12);
    let (body, footer) = frame.rsplit_once('\n').unwrap();
    assert!(!body.contains("enter follow") && !body.contains("esc cancel"));
    assert!(footer.contains("⏎ follow") && footer.contains("esc cancel"));
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
fn context_picker_and_navigation_keys_use_the_displayed_source() {
    let (mut m, _) = DefinitionFixture::default().browsing();
    m.search.focus = SearchPanel::Context;
    m.search.preview_scroll = Some(2);
    assert_eq!(m.action_for(key(KeyCode::Enter)), Some(Action::Follow));
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
        None
    );
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
        None
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
fn navigation_history_restores_find_hits_horizontal_scroll_and_expansion() {
    let mut m = searched();
    m.update(Action::FocusNth(4));
    m.on_key(key(KeyCode::Char('/')));
    typed(&mut m, "fn");
    m.on_key(key(KeyCode::Enter));
    m.on_key(key(KeyCode::Right));
    m.on_key(key(KeyCode::Char('z')));
    let horizontal = m.search.inspection.horizontal;
    let scroll = m.search.preview_scroll;
    let entry = crate::modes::search::browse::NavigationEntry::capture(&m.search);
    m.search.inspection = Default::default();
    m.search.expanded = None;
    m.search.preview_scroll = None;
    entry.restore(&mut m.search);
    assert_eq!(m.search.inspection.term, "fn");
    assert_eq!(m.search.inspection.hits.len(), 2);
    assert_eq!(m.search.inspection.horizontal, horizontal);
    assert_eq!(m.search.preview_scroll, scroll);
    assert_eq!(m.search.expanded, Some(SearchPanel::Context));
}

#[test]
fn inspection_prompts_allow_focus_navigation_and_ignore_hidden_source() {
    let mut m = searched();
    m.update(Action::FocusNth(4));
    m.on_key(key(KeyCode::Char('/')));
    typed(&mut m, "fn");
    m.on_key(key(KeyCode::Tab));
    assert_eq!(m.search.focus, SearchPanel::Body);
    assert!(m.search.inspection.edit.is_none());
    assert_eq!(m.search.inspection.term, "fn");
    m.on_key(key(KeyCode::Char(':')));
    typed(&mut m, "66");
    m.on_key(key(KeyCode::BackTab));
    assert_eq!(m.search.focus, SearchPanel::Context);
    assert!(m.search.body.inspection.edit.is_none());
    assert!(m.search.body.inspection.line.is_none());
    m.search.results.matches = Default::default();
    assert!(m.search.displayed_source().is_none());
    m.update(Action::InspectFind);
    assert!(m.search.inspection.edit.is_none());
}
