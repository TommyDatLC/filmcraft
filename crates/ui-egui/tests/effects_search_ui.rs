use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use serde_json::json;

fn key(h: &mut Harness<'_, FilmcraftApp>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.step();
    }
    h.run_steps(2);
    assert!(h.state().ui_error.is_none());
}

#[test]
fn shift_space_opens_effect_search_without_starting_playback_and_escape_closes() {
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let app = FilmcraftApp::new(session);
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_eframe(move |_| app);
    h.run_steps(4);
    key(&mut h, Key::Space, Modifiers::SHIFT);
    assert!(h.state().dialog.is_some(), "Shift+Space should open effect search");
    assert!(!h.state().playback.playing, "Shift+Space must not trigger playback");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(h.state().playback.playing, "Unmodified Space remains Play/Pause");
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(!h.state().playback.playing);
}

#[test]
fn typing_and_enter_applies_one_effect_and_reopening_shows_recent_history() {
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let clip = session.active_sequence().unwrap().video_tracks[0].items[0].id;
    session.execute("timeline.select", json!({"clips": [clip.0], "add": false, "toggle": false})).unwrap();
    let app = FilmcraftApp::new(session);
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_eframe(move |_| app);
    h.run_steps(4);
    key(&mut h, Key::Space, Modifiers::SHIFT);
    assert!(h.state().auto.find("effectSearch.query").is_some());
    h.event(Event::Text("gaussian".into()));
    h.run_steps(3);
    assert!(h.state().auto.find("effectSearch.item.gaussian_blur").is_some());
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    let item = &h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1;
    assert_eq!(item.effects.iter().filter(|effect| effect.effect == "gaussian_blur").count(), 1);
    assert_eq!(h.state_mut().session.execute("prefs.get", json!({"key": "recentEffects"})).unwrap(), json!(["gaussian_blur"]));
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert!(!h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1.effects.iter().any(|effect| effect.effect == "gaussian_blur"));
    key(&mut h, Key::Space, Modifiers::SHIFT);
    assert!(h.state().auto.find("effectSearch.recent").is_some());
    assert!(h.state().auto.find("effectSearch.item.gaussian_blur").is_some());
}

#[test]
fn space_in_query_is_text_and_missing_selection_keeps_dialog_open() {
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let app = FilmcraftApp::new(session);
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_eframe(move |_| app);
    h.run_steps(4);
    key(&mut h, Key::Space, Modifiers::SHIFT);
    assert!(h.state().auto.find("effectSearch.query").is_some());
    h.event(Event::Text("gaussian blur".into()));
    h.run_steps(3);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(!h.state().playback.playing);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().dialog.is_some());
    assert!(h.state().auto.find("effectSearch.message").is_some());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
}

#[test]
fn compatible_presets_keep_shift_space_playback_and_default_uses_effect_search() {
    use filmcraft_engine::shortcuts::{AVID_PRESET, DEFAULT_PRESET, FCP_PRESET, PREMIERE_PRESET};
    let mut session = Session::default();
    session.shortcuts.register_external(filmcraft_ui_egui::menus::external_commands());
    for preset in [DEFAULT_PRESET, PREMIERE_PRESET, FCP_PRESET, AVID_PRESET] {
        session.shortcuts.load_preset(preset).unwrap();
        let resolved = session.execute("shortcuts.resolve", json!({"keys": "Shift+Space"})).unwrap();
        let expected = if preset == DEFAULT_PRESET { "effects.openSearch" } else { "playback.inToOutPreroll" };
        assert_eq!(resolved["command"], expected, "{preset}");
    }
}

#[test]
fn incompatible_or_empty_selection_does_not_apply_effects_at_the_playhead() {
    use std::sync::Arc;
    for (select_audio, query) in [(true, "Gaussian Blur"), (false, "Cross Dissolve")] {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).unwrap();
        let seq = session.active_sequence().unwrap();
        let cut = seq.video_tracks[0].items[0].end();
        let audio_clip = seq.audio_tracks[0].items[0].id.0;
        session.execute("playhead.set", json!({"time": cut.0})).unwrap();
        if select_audio {
            // timeline.select expands A/V linked pairs. Model the audio-only selection produced
            // by an individual timeline selection instead of selecting its picture as well.
            session.state.selection = vec![filmcraft_project::ClipId(audio_clip)];
        }
        let project = session.project.clone();
        let app = FilmcraftApp::new(session);
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_eframe(move |_| app);
        h.run_steps(4);
        key(&mut h, Key::Space, Modifiers::SHIFT);
        h.event(Event::Text(query.into()));
        h.run_steps(3);
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert!(h.state().dialog.is_some(), "Invalid selection must keep search open: {query}");
        assert!(Arc::ptr_eq(&h.state().session.project, &project), "Search must not edit unrelated clips: {query}");
        assert!(h.state().auto.find("effectSearch.message").is_some());
    }
}
