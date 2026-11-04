use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_geom::{Affine, Vec2};
use filmcraft_media::{Generator, generators::GeneratorSource};
use filmcraft_project::{ClipId, Label, ParamValue, Project, SequenceSettings, TrackKind, video_fx::DynamicZoomSettings};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use filmcraft_ui_egui::{FilmcraftApp, panels::preview_gizmos::GizmoSpace};
use serde_json::json;
use std::sync::Arc;

fn fixture() -> (Harness<'static, FilmcraftApp>, ClipId, filmcraft_render::SourceMap) {
    let mut session = Session::default();
    let mut p = Project::new("Dynamic Zoom synthetic portrait");
    let source = || GeneratorSource::new(Generator::BarsAndTone, 180, 320, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(600));
    let item = filmcraft_engine::demo::add_generator(&mut p, &session.media, source(), "Portrait bars", Label::Iris, None);
    let sequence = p.new_sequence("Gizmo", SequenceSettings { width: 320, height: 180, frame_rate: FrameRate::FPS_60, ..Default::default() }, 1, 0, None);
    let mut clip =
        p.make_track_item(item, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, FrameRate::FPS_60.tick_of(240)), FrameRate::FPS_60).unwrap();
    for effect in &mut clip.effects {
        filmcraft_project::resolve_auto_points(effect, (320, 180), (180, 320));
    }
    let motion = clip.effect_mut("motion").unwrap();
    motion.params.get_mut("scale").unwrap().value = ParamValue::Float(45.0);
    motion.params.get_mut("rotation").unwrap().value = ParamValue::Float(7.0);
    motion.params.get_mut("position").unwrap().put_keyframe(Tick::ZERO, ParamValue::Vec2(Vec2::new(190.0, 95.0)));
    let id = clip.id;
    p.sequence_mut(sequence).unwrap().video_tracks[0].items.push(clip);
    session.project = Arc::new(p);
    session.state.active_sequence = Some(sequence);
    session.state.open_sequences = vec![sequence];
    session.execute("timeline.select", json!({"clips":[id.0]})).unwrap();
    session.execute("effects.apply", json!({"clips":[id.0],"effect":"dynamic_zoom"})).unwrap();
    let mut app = FilmcraftApp::new(session);
    app.show_panel(filmcraft_ui_egui::dock::PanelKind::EffectControls);
    for (index, effect) in app.session.active_sequence().unwrap().find_item(id).unwrap().1.effects.iter().enumerate() {
        if effect.effect != "dynamic_zoom" {
            app.ui.collapsed_fx.push(format!("{}:{index}", id.0));
        }
    }
    let mut builder = Harness::builder().with_size(vec2(1440.0, 960.0)).with_step_dt(1.0 / 60.0);
    if std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR").is_some() {
        builder = builder.wgpu();
    }
    let mut h = builder.build_eframe(move |_| app);
    h.run_steps(5);
    let mut sources = filmcraft_render::SourceMap::default();
    sources.0.insert(item, Arc::new(source()));
    (h, id, sources)
}
fn center(h: &Harness<'_, FilmcraftApp>, id: &str) -> Pos2 {
    let r = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}")).rect;
    pos2(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5)
}
fn button(h: &mut Harness<'_, FilmcraftApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.step();
}
fn click(h: &mut Harness<'_, FilmcraftApp>, id: &str) {
    let p = center(h, id);
    h.event(Event::PointerMoved(p));
    h.step();
    button(h, p, true);
    button(h, p, false);
    h.run_steps(3);
}
fn settings(h: &Harness<'_, FilmcraftApp>, clip: ClipId) -> DynamicZoomSettings {
    let it = h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1;
    DynamicZoomSettings::from_effect(it.effect("dynamic_zoom").unwrap()).unwrap()
}
fn drag(h: &mut Harness<'_, FilmcraftApp>, id: &str, delta: egui::Vec2) {
    let p = center(h, id);
    h.event(Event::PointerMoved(p));
    h.step();
    button(h, p, true);
    for fraction in [0.4, 0.7, 1.0] {
        h.event(Event::PointerMoved(p + delta * fraction));
        h.step();
    }
    button(h, p + delta, false);
    h.run_steps(3);
}

#[test]
fn preview_pan_zoom_par_roundtrip() {
    // 1440x1080 with 4:3 pixels, viewer zoom 2, nonzero pan. Known screen coordinates.
    let affine = Affine::translate(137.0, -83.0).then_apply(&Affine::scale(8.0 / 3.0, 2.0));
    let space = GizmoSpace::new(affine).unwrap();
    assert_eq!(space.to_screen(Vec2::new(720.0, 540.0)), pos2(2057.0, 997.0));
    for p in [Vec2::new(0.0, 0.0), Vec2::new(450.0, 125.0), Vec2::new(720.0, 540.0), Vec2::new(1440.0, 1080.0)] {
        assert!((space.from_screen(space.to_screen(p)).unwrap() - p).length() < 1e-6);
    }
    assert!(GizmoSpace::new(Affine::scale(0.0, 1.0)).is_none());
    assert!(GizmoSpace::new(Affine { a: f64::NAN, ..Affine::IDENTITY }).is_none());
    assert!(space.from_screen(pos2(f32::INFINITY, 0.0)).is_none());
}

#[test]
fn ordinary_motion_handles_follow_the_dynamic_camera_and_track_bypass() {
    let (mut h, clip, _) = fixture();
    for (param, value) in [("end_width", json!(0.5)), ("end_center", json!([0.6, 0.45]))] {
        h.state_mut().session.execute("effects.setParam", json!({"clip":clip.0,"effect":"dynamic_zoom","param":param,"value":value})).unwrap();
    }
    h.state_mut().session.execute("playhead.set", json!({"time":FrameRate::FPS_60.tick_of(239).0})).unwrap();
    h.state_mut().session.execute("effects.select", json!({"clip":clip.0,"effect":"motion"})).unwrap();
    h.run_steps(4);
    let picture = h.state().auto.find("program.picture").unwrap().rect;
    // Motion puts the anchor at (190,95). End viewport centered at (192,81), width .5:
    // (160,90) + 2*((190,95)-(192,81)) = (156,118).
    let expected = pos2(picture[0] + picture[2] * 156.0 / 320.0, picture[1] + picture[3] * 118.0 / 180.0);
    assert!(center(&h, "program.transform.anchor").distance(expected) < 0.1, "Motion overlay must follow visible camera output");
    let track = h.state().session.active_sequence().unwrap().video_tracks[0].id;
    h.state_mut().session.execute("timeline.setVideoFxBypass", json!({"track":track.0,"bypass":true})).unwrap();
    h.run_steps(3);
    let expected = pos2(picture[0] + picture[2] * 190.0 / 320.0, picture[1] + picture[3] * 95.0 / 180.0);
    assert!(center(&h, "program.transform.anchor").distance(expected) < 0.1);
}

#[test]
fn edit_preview_does_not_change_export_and_playback_uses_normal_target() {
    let (mut h, clip, sources) = fixture();
    let time = FrameRate::FPS_60.tick_of(120);
    h.state_mut().session.execute("playhead.set", json!({"time":time.0})).unwrap();
    let project = h.state().session.project.clone();
    let sequence = h.state().session.state.active_sequence.unwrap();
    let before = filmcraft_render::render_sequence(&project, sequence, time, Default::default(), &sources);
    click(&mut h, "effectControls.dynamicZoom.edit");
    assert!(h.state().auto.find("program.dynamicZoom.start.box").is_some());
    assert!(h.state().auto.find("program.dynamicZoom.end.box").is_some());
    assert!(Arc::ptr_eq(&project, &h.state().session.project));
    let after = filmcraft_render::render_sequence(&h.state().session.project, sequence, time, Default::default(), &sources);
    assert!(before.px.iter().zip(after.px).all(|(a, b)| (a - b).abs() < 1e-6));
    assert_ne!(filmcraft_ui_egui::frames::Target::DynamicZoomEdit { sequence, clip }, filmcraft_ui_egui::frames::Target::Sequence(sequence));
    if let Some(dir) = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(std::path::PathBuf::from(dir).join("dynamic-zoom-portrait.png")).unwrap();
    }
    h.state_mut().playback.playing = true;
    h.run_steps(3);
    assert!(h.state().auto.elements.iter().all(|e| !e.id.starts_with("program.dynamicZoom.")));
    assert!(Arc::ptr_eq(&project, &h.state().session.project));
}

#[test]
fn geometry_has_no_mask_buttons() {
    let (h, _, _) = fixture();
    assert!(!h.state().auto.elements.iter().any(|e| e.id.starts_with("effectControls.dynamic_zoom.mask.")), "camera geometry cannot accept a source mask");
}

#[test]
fn normalized_center_scrubs_with_one_undo() {
    let (mut h, clip, _) = fixture();
    let r = h.state().auto.find("effectControls.dynamicZoom.param.start_center").expect("normalized center value").rect;
    let p = pos2(r[0] + r[2] * 0.25, r[1] + r[3] * 0.5);
    let project = h.state().session.project.clone();
    h.event(Event::PointerMoved(p));
    h.step();
    button(&mut h, p, true);
    for dx in [8.0, 14.0, 20.0] {
        h.event(Event::PointerMoved(p + vec2(dx, 0.0)));
        h.step();
    }
    button(&mut h, p + vec2(20.0, 0.0), false);
    h.run_steps(3);
    let changed = settings(&h, clip).start.center;
    assert!(changed.x > 0.5 && changed.x < 0.8, "normalized coordinates need sub-unit scrubbing: {changed:?}");
    assert_eq!(changed.y, 0.5);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*h.state().session.project, *project);
}

#[test]
fn coincident_rectangles_are_individually_selectable_and_drag_is_one_undo() {
    let (mut h, clip, _) = fixture();
    h.state_mut().session.execute("effects.setParam", json!({"clip":clip.0,"effect":"dynamic_zoom","param":"end_width","value":1.0})).unwrap();
    let motion = h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1.effect("motion").unwrap().clone();
    click(&mut h, "effectControls.dynamicZoom.edit");
    click(&mut h, "effectControls.dynamicZoom.active.start");
    let original = h.state().session.project.clone();
    let before = settings(&h, clip);
    drag(&mut h, "program.dynamicZoom.start.box", vec2(35.0, 17.0));
    let moved = settings(&h, clip);
    assert_ne!(moved.start.center, before.start.center);
    assert_eq!(moved.end, before.end);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*h.state().session.project, *original);
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    assert_eq!(settings(&h, clip), moved);
    click(&mut h, "effectControls.dynamicZoom.active.end");
    drag(&mut h, "program.dynamicZoom.end.handle.2", vec2(-30.0, -22.0));
    let resized = settings(&h, clip);
    assert_ne!(resized.end.width, moved.end.width);
    assert_eq!(resized.start, moved.start);
    assert!(resized.end.is_valid());
    let r = h.state().auto.find("program.dynamicZoom.end.box").unwrap().rect;
    assert!((f64::from(r[2] / r[3]) - 320.0 / 180.0).abs() < 1e-5);
    assert_eq!(h.state().session.active_sequence().unwrap().find_item(clip).unwrap().1.effect("motion").unwrap(), &motion);
}

#[test]
fn escape_and_selection_change_discard_only_the_viewport_draft() {
    let (mut h, clip, _) = fixture();
    click(&mut h, "effectControls.dynamicZoom.edit");
    let before = settings(&h, clip);
    let p = center(&h, "program.dynamicZoom.start.box");
    h.event(Event::PointerMoved(p));
    h.step();
    button(&mut h, p, true);
    h.event(Event::PointerMoved(p + vec2(40.0, 20.0)));
    h.run_steps(2);
    assert_eq!(settings(&h, clip), before, "model remains unchanged while dragging");
    let track = h.state().session.active_sequence().unwrap().video_tracks[0].id;
    h.state_mut().session.execute("timeline.setTrack", json!({"track":track.0,"name":"Unrelated edit"})).unwrap();
    h.event(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    button(&mut h, p + vec2(40.0, 20.0), false);
    h.run_steps(3);
    assert_eq!(settings(&h, clip), before);
    assert_eq!(h.state().session.active_sequence().unwrap().video_tracks[0].name, "Unrelated edit");
    click(&mut h, "effectControls.dynamicZoom.reverse");
    assert!(settings(&h, clip).reverse);
    click(&mut h, "effectControls.dynamicZoom.easing");
    click(&mut h, "effectControls.dynamicZoom.easing.3");
    assert_eq!(settings(&h, clip).easing, filmcraft_project::video_fx::DynamicZoomEase::EaseInOut);
    h.state_mut().session.execute("timeline.select", json!({"clips":[]})).unwrap();
    h.run_steps(3);
    assert!(h.state().auto.elements.iter().all(|e| !e.id.starts_with("program.dynamicZoom.")));
}
