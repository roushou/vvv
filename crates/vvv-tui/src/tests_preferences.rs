// Preferences behavior.

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
fn places_return_to_completed_searches_without_recording_each_keystroke() {
    let mut m = searched();
    m.on_event(Event::Viewport {
        width: 90,
        height: 20,
    });
    m.search.focus = SearchPanel::Context;
    m.search.preview_scroll = Some(13);
    m.search.results.files.filter = "lang".into();
    let original = m.search.results.current().unwrap().id.clone();
    let original_preview = m.search.preview.clone();
    m.search.focus = SearchPanel::Query;
    m.update(Action::Clear);
    typed(&mut m, "Beta");
    m.on_event(Event::Searched {
        generation: m.generation,
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(m.search.trail.position(), (2, 2));
    m.update(Action::Clear);
    typed(&mut m, "Gamma");
    m.on_event(Event::Searched {
        generation: m.generation,
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(m.search.trail.position(), (3, 3));
    m.update(Action::Places);
    m.update(Action::Top);
    let effects = m.update(Action::Enter);
    assert_eq!(m.search.trail.position(), (1, 3));
    assert_eq!(m.search.query.text(), "Language");
    assert_eq!(m.search.results.files.filter, "lang");
    assert_eq!(m.search.results.current().unwrap().id, original);
    assert_eq!(m.search.preview_scroll, Some(13));
    assert_eq!(
        m.search.preview.as_ref().unwrap().text(),
        original_preview.as_ref().unwrap().text()
    );
    assert!(m.search.stale);
    assert!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Follow { .. }))
            .count()
            <= 1
    );
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
        None
    );
    assert_eq!(
        m.action_for(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
        Some(Action::BrowseForward)
    );
    insta::assert_snapshot!("places_returned", FrameFixture::new(&m).render());
}

#[test]
fn places_and_context_help_cancel_without_losing_the_picker_or_browsing_position() {
    let mut m = searched();
    m.search.focus = SearchPanel::Files;
    let selected = m.search.results.current().unwrap().id.clone();
    m.update(Action::Places);
    m.update(Action::PlacesTab);
    typed(&mut m, "Language");
    m.on_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert!(matches!(m.overlay, Some(Overlay::Help { .. })));
    insta::assert_snapshot!(
        "places_help_narrow",
        FrameFixture::new(&m).render_size(50, 18)
    );
    m.on_event(Event::Viewport {
        width: 50,
        height: 18,
    });
    m.on_key(key(KeyCode::End));
    let Some(Overlay::Help { scroll: end, .. }) = &m.overlay else {
        panic!();
    };
    let end = *end;
    assert!(end > 0);
    m.on_key(key(KeyCode::PageUp));
    let Some(Overlay::Help { scroll, .. }) = &m.overlay else {
        panic!();
    };
    assert!(*scroll < end);
    m.on_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    let Some(Overlay::Places(picker)) = &m.overlay else {
        panic!("help restores Places");
    };
    assert!(picker.recent);
    assert_eq!(picker.filter, "Language");
    assert_eq!(m.search.results.current().unwrap().id, selected);
    m.update(Action::Back);
    assert_eq!(m.search.focus, SearchPanel::Files);
    assert_eq!(m.search.results.current().unwrap().id, selected);
    assert_eq!(m.search.trail.position(), (1, 1));
}

#[test]
fn recent_searches_restore_all_restrictions_and_reject_superseded_answers() {
    use crate::modes::search::recall::SearchRecipe;
    let mut m = searched();
    let recipe = SearchRecipe {
        query: "Engine lang:rust".into(),
        location: Some("crates/vvv".into()),
        category: "uses".into(),
        files: "srv".into(),
    };
    m.search.recent.remember(recipe.clone());
    let old_generation = m.generation;
    m.update(Action::Places);
    m.update(Action::PlacesTab);
    typed(&mut m, "Engine");
    insta::assert_snapshot!(
        "places_recent_narrow",
        FrameFixture::new(&m).render_size(50, 18)
    );
    let effects = m.update(Action::Enter);
    let [
        Effect::Search {
            query,
            scope,
            generation,
        },
    ] = effects.as_slice()
    else {
        panic!("recent search executes afresh");
    };
    assert!(*generation > old_generation);
    assert_eq!(query.language().unwrap().as_str(), "rust");
    assert_eq!(scope.paths, vec![vvv_engine::RelPath::from("crates/vvv")]);
    assert_eq!(m.search.query.text(), recipe.query);
    assert_eq!(
        m.search.results.category,
        crate::modes::search::Category::Uses
    );
    assert_eq!(m.search.results.files.filter, "srv");
    m.on_event(Event::Searched {
        generation: old_generation,
        matches: vec![],
        skipped: vec![],
    });
    assert!(!m.search.results.matches.is_empty());
    assert!(m.status.busy);
    m.on_event(Event::Searched {
        generation: *generation,
        matches: vec![],
        skipped: vec![],
    });
    assert!(!m.status.busy);
    assert!(m.search.results.matches.is_empty());
    assert_eq!(m.search.recent.entries()[0], recipe);
}

#[test]
fn forgetting_a_recent_search_survives_picker_reopening_and_preference_round_trip() {
    use crate::preferences::Preferences;
    let mut m = searched();
    m.update(Action::Places);
    m.update(Action::PlacesTab);
    m.update(Action::ForgetSearch);
    let Some(Overlay::Places(p)) = &m.overlay else {
        panic!();
    };
    assert!(p.visible().is_empty());
    m.update(Action::Back);
    m.update(Action::Places);
    m.update(Action::PlacesTab);
    let Some(Overlay::Places(p)) = &m.overlay else {
        panic!();
    };
    assert!(p.visible().is_empty());
    let bytes = Preferences::capture(&m).encode();
    let mut restored = model();
    Preferences::decode(&bytes, &m.root)
        .unwrap()
        .restore(&mut restored);
    assert!(restored.search.recent.entries().is_empty());
    m.update(Action::Back);
    m.update(Action::Refresh);
    m.on_event(Event::Searched {
        generation: m.generation,
        matches: fx::search().matches,
        skipped: vec![],
    });
    assert_eq!(
        m.search.recent.entries().len(),
        1,
        "explicit search records the recipe again"
    );
}

#[test]
fn places_trail_keeps_complete_paths_in_a_short_terminal() {
    let mut m = searched();
    m.search
        .locations
        .select(Some(
            "crates/vvv/src/very/long/path/with/multiple/components",
        ))
        .unwrap();
    m.search.remember_page(false);
    m.update(Action::Places);
    insta::assert_snapshot!(
        "places_trail_narrow",
        FrameFixture::new(&m).render_size(50, 12)
    );
    m.update(Action::PlacesTab);
    m.update(Action::Clear);
    typed(&mut m, "components Language");
    let Some(Overlay::Places(p)) = &m.overlay else {
        panic!();
    };
    assert_eq!(p.visible().len(), 1);
    m.update(Action::Top);
    m.update(Action::Bottom);
    insta::assert_snapshot!(
        "places_long_recipe",
        FrameFixture::new(&m).render_size(50, 12)
    );
}
