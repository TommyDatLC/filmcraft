use egui::{Event, Key, Modifiers, PointerButton, pos2, vec2};
use egui_kittest::Harness;
use filmcraft_engine::{EffectTarget, Session};
use filmcraft_project::{ClipId, TrackId};
use filmcraft_ui_egui::FilmcraftApp;
use serde_json::json;

fn demo() -> (Harness<'static, FilmcraftApp>, TrackId, ClipId) {
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let track = session.active_sequence().unwrap().video_tracks[0].id;
    let clip = session.active_sequence().unwrap().video_tracks[0].items[0].id;
    let mut app = FilmcraftApp::new(session);
    app.show_panel(filmcraft_ui_egui::dock::PanelKind::EffectControls);
    let mut builder = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0);
    if std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR").is_some() {
        builder = builder.wgpu();
    }
    let mut h = builder.build_eframe(move |_| app);
    h.run_steps(4);
    (h, track, clip)
}
fn key(h: &mut Harness<'_, FilmcraftApp>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.step();
    }
    h.run_steps(2);
    assert!(h.state().ui_error.is_none());
}
fn click(h: &mut Harness<'_, FilmcraftApp>, id: &str) {
    let r = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}")).rect;
    let pos = pos2(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(3);
}
fn search(h: &mut Harness<'_, FilmcraftApp>, query: &str) {
    key(h, Key::Space, Modifiers::SHIFT);
    h.event(Event::Text(query.into()));
    h.run_steps(3);
    key(h, Key::Enter, Modifiers::NONE);
}

#[test]
fn header_search_and_clip_selection_route_to_separate_stacks() {
    let (mut h, track, clip) = demo();
    click(&mut h, &format!("timeline.track.{}.effectTarget", track.0));
    assert_eq!(h.state().session.state.effect_controls_target, Some(EffectTarget::VideoTrack(track)));
    search(&mut h, "Gaussian Blur");
    assert!(h.state().dialog.is_none());
    let q = h.state().session.active_sequence().unwrap();
    assert_eq!(q.track(track).unwrap().effects.iter().filter(|e| e.effect == "gaussian_blur").count(), 1);
    assert!(!q.find_item(clip).unwrap().1.effects.iter().any(|e| e.effect == "gaussian_blur"));
    assert!(h.state().auto.find(&format!("effectControls.track.{}.effect.0", track.0)).is_some());
    if let Some(dir) = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(std::path::PathBuf::from(dir).join("video-track-fx.png")).unwrap();
    }
    h.state_mut().session.execute("timeline.select", json!({"clips":[clip.0]})).unwrap();
    h.run_steps(3);
    search(&mut h, "Gaussian Blur");
    assert!(h.state().dialog.is_none());
    assert!(h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1.effects.iter().any(|e| e.effect == "gaussian_blur"));
    assert_eq!(h.state_mut().session.execute("prefs.get", json!({"key":"recentEffects"})).unwrap(), json!(["gaussian_blur"]));
}

#[test]
fn track_search_filters_unsupported_effects_and_space_still_plays() {
    let (mut h, track, _) = demo();
    h.state_mut().session.execute("timeline.selectVideoTrack", json!({"track":track.0})).unwrap();
    for (query, id) in [
        ("Dynamic Zoom", "dynamic_zoom"),
        ("Echo", "echo"),
        ("Cross Dissolve", "cross_dissolve"),
        ("Parametric Equalizer", "parametric_equalizer"),
        ("Gradient Wipe", "gradient_wipe_legacy"),
    ] {
        key(&mut h, Key::Space, Modifiers::SHIFT);
        h.event(Event::Text(query.into()));
        h.run_steps(3);
        assert!(h.state().auto.find(&format!("effectSearch.item.{id}")).is_none(), "unsupported {query}");
        key(&mut h, Key::Escape, Modifiers::NONE);
    }
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(h.state().playback.playing);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(!h.state().playback.playing);
}

#[test]
fn fx_button_preserves_selection_eye_lock_and_disabled_instances() {
    let (mut h, track, _) = demo();
    h.state_mut().session.execute("timeline.selectVideoTrack", json!({"track":track.0})).unwrap();
    h.state_mut().session.execute("effects.apply", json!({"track":track.0,"effect":"gaussian_blur"})).unwrap();
    h.state_mut().session.execute("effects.toggleEnabled", json!({"track":track.0,"index":0})).unwrap();
    h.run_steps(3);
    let original = h.state().session.active_sequence().unwrap().track(track).unwrap().clone();
    click(&mut h, &format!("timeline.track.{}.fx", track.0));
    let tr = h.state().session.active_sequence().unwrap().track(track).unwrap();
    assert!(tr.video_effects_bypassed);
    assert_eq!((tr.enabled, tr.locked, &tr.effects), (original.enabled, original.locked, &original.effects));
    assert_eq!(h.state().session.state.effect_controls_target, Some(EffectTarget::VideoTrack(track)));
    click(&mut h, &format!("timeline.track.{}.fx", track.0));
    assert!(!h.state().session.active_sequence().unwrap().track(track).unwrap().video_effects_bypassed);
    assert!(!h.state().session.active_sequence().unwrap().track(track).unwrap().effects[0].enabled);
}

#[test]
fn inspector_edits_duplicate_instances_and_sequence_keyframes() {
    let (mut h, track, _) = demo();
    for _ in 0..2 {
        h.state_mut().session.execute("effects.apply", json!({"track":track.0,"effect":"gaussian_blur"})).unwrap();
    }
    h.state_mut().session.execute("effects.setParam", json!({"track":track.0,"effect":1,"param":"blurriness","value":37.0})).unwrap();
    h.state_mut().session.execute("effects.select", json!({"track":track.0,"effect":1})).unwrap();
    h.run_steps(3);
    click(&mut h, &format!("effectControls.track.{}.effect.1.up", track.0));
    let q = h.state().session.active_sequence().unwrap();
    let effects = &q.track(track).unwrap().effects;
    assert_eq!(h.state().session.state.selected_effect.as_ref().unwrap().index(effects), Some(0));
    assert_eq!(effects[0].params["blurriness"].value, filmcraft_project::ParamValue::Float(37.0));
    let time = q.settings.frame_rate.tick_of(120);
    h.state_mut().session.execute("playhead.set", json!({"time":time.0})).unwrap();
    click(&mut h, &format!("effectControls.track.{}.effect.0.param.blurriness.stopwatch", track.0));
    assert_eq!(h.state().session.active_sequence().unwrap().track(track).unwrap().effects[0].params["blurriness"].keyframes[0].time, time);
    click(&mut h, &format!("effectControls.track.{}.effect.1.remove", track.0));
    assert_eq!(h.state().session.active_sequence().unwrap().track(track).unwrap().effects.len(), 1);
}

#[test]
fn locked_track_search_reports_error_and_does_not_edit() {
    let (mut h, track, _) = demo();
    h.state_mut().session.execute("timeline.setTrack", json!({"track":track.0,"locked":true})).unwrap();
    h.state_mut().session.execute("timeline.selectVideoTrack", json!({"track":track.0})).unwrap();
    let project = h.state().session.project.clone();
    search(&mut h, "Gaussian Blur");
    assert!(h.state().dialog.is_some());
    let message = h.state().auto.find("effectSearch.message").unwrap();
    assert!(message.label.to_lowercase().contains("locked"), "{}", message.label);
    assert!(std::sync::Arc::ptr_eq(&project, &h.state().session.project));
}

#[test]
fn track_keyframe_drag_is_staged_and_escape_does_not_undo_other_edits() {
    let (mut h, track, _) = demo();
    h.state_mut().session.execute("effects.apply", json!({"track":track.0,"effect":"gaussian_blur"})).unwrap();
    let time = h.state().session.active_sequence().unwrap().settings.frame_rate.tick_of(60);
    h.state_mut().session.execute("playhead.set", json!({"time":time.0})).unwrap();
    h.state_mut().session.execute("effects.toggleAnimation", json!({"track":track.0,"effect":0,"param":"blurriness"})).unwrap();
    h.run_steps(3);
    let id = format!("effectControls.track.{}.effect.0.param.blurriness.key.{}", track.0, time.0);
    let r = h.state().auto.find(&id).unwrap().rect;
    let origin = pos2(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    let before = h.state().session.project.clone();
    h.event(Event::PointerMoved(origin));
    h.step();
    h.event(Event::PointerButton { pos: origin, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerMoved(origin + vec2(15.0, 0.0)));
    h.run_steps(2);
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.project), "drag draft must not edit project");
    h.state_mut().session.execute("timeline.setTrack", json!({"track":track.0,"name":"Independent edit"})).unwrap();
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.event(Event::PointerButton { pos: origin + vec2(15.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let tr = h.state().session.active_sequence().unwrap().track(track).unwrap();
    assert_eq!(tr.name, "Independent edit");
    assert_eq!(tr.effects[0].params["blurriness"].keyframes[0].time, time);
}

#[test]
fn deleting_selected_track_clears_inspector_target() {
    let (mut h, track, _) = demo();
    h.state_mut().session.execute("timeline.selectVideoTrack", json!({"track":track.0})).unwrap();
    h.state_mut().session.execute("sequence.deleteTrack", json!({"track":track.0})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.state.effect_controls_target.is_none());
    assert!(h.state().auto.find(&format!("effectControls.track.{}", track.0)).is_none());
}

#[test]
fn identical_track_instances_cancel_key_drag_after_reorder_or_remove() {
    for command in ["effects.reorder", "effects.remove"] {
        let (mut h, track, _) = demo();
        let time = h.state().session.active_sequence().unwrap().settings.frame_rate.tick_of(60);
        h.state_mut().session.execute("playhead.set", json!({"time":time.0})).unwrap();
        for index in 0..2 {
            h.state_mut().session.execute("effects.apply", json!({"track":track.0,"effect":"gaussian_blur"})).unwrap();
            h.state_mut().session.execute("effects.toggleAnimation", json!({"track":track.0,"effect":index,"param":"blurriness"})).unwrap();
        }
        h.state_mut().session.execute("effects.select", json!({"track":track.0,"effect":0})).unwrap();
        h.run_steps(3);
        let id = format!("effectControls.track.{}.effect.0.param.blurriness.key.{}", track.0, time.0);
        let rect = h.state().auto.find(&id).unwrap().rect;
        let origin = pos2(rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5);
        h.event(Event::PointerMoved(origin));
        h.step();
        h.event(Event::PointerButton { pos: origin, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        h.event(Event::PointerMoved(origin + vec2(20.0, 0.0)));
        h.run_steps(2);
        h.state_mut().session.execute(command, json!({"track":track.0,"from":0,"to":1,"index":0})).unwrap();
        h.run_steps(2);
        let after_command = h.state().session.project.clone();
        let undo_count = h.state().session.history.undo.len();
        h.event(Event::PointerButton { pos: origin + vec2(20.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        assert!(*h.state().session.project == *after_command, "{command} must cancel the draft rather than edit a replacement instance");
        assert_eq!(h.state().session.history.undo.len(), undo_count);
    }
}

#[test]
fn effects_panel_double_click_uses_track_target_and_compact_headers_stay_clickable() {
    let (mut h, track, clip) = demo();
    h.state_mut().ui.timeline.video_track_h = 22.0;
    h.run_steps(3);
    click(&mut h, &format!("timeline.track.{}.effectTarget", track.0));
    h.state_mut().show_panel(filmcraft_ui_egui::dock::PanelKind::Effects);
    h.state_mut().ui.effects_search = "Gaussian Blur".into();
    // Separate the header click from the double click (egui's global triple-click window).
    h.run_steps(40);
    click(&mut h, "effects.item.gaussian_blur");
    click(&mut h, "effects.item.gaussian_blur");
    if let Some(dir) = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR") {
        h.render().unwrap().save(std::path::PathBuf::from(dir).join("video-track-effects-panel.png")).unwrap();
    }
    let q = h.state().session.active_sequence().unwrap();
    assert_eq!(q.track(track).unwrap().effects.len(), 1);
    assert!(!q.find_item(clip).unwrap().1.effects.iter().any(|e| e.effect == "gaussian_blur"));
}
