use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use filmcraft_engine::{EffectTarget, Session};
use filmcraft_geom::Vec2;
use filmcraft_media::{Generator, generators::GeneratorSource};
use filmcraft_project::{ClipId, EffectInstance, Label, ParamValue, Project, SequenceSettings, TrackId, TrackKind};
use filmcraft_time::{FrameRate, TimeRange};
use filmcraft_ui_egui::FilmcraftApp;
use serde_json::{Value, json};
use std::sync::Arc;

struct Driver {
    h: Harness<'static, FilmcraftApp>,
    clip: ClipId,
    track: TrackId,
    target: EffectTarget,
    index: usize,
    sources: filmcraft_render::SourceMap,
}
impl Driver {
    fn new(track_fx: bool) -> Self {
        let mut s = Session::default();
        let mut p = Project::new("Vignette synthetic");
        let source = || GeneratorSource::new(Generator::BarsAndTone, 640, 360, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(720));
        let media = filmcraft_engine::demo::add_generator(&mut p, &s.media, source(), "Synthetic bars", Label::Iris, None);
        let seq = p.new_sequence("Gizmo", SequenceSettings { width: 320, height: 180, frame_rate: FrameRate::FPS_60, ..Default::default() }, 1, 0, None);
        let track = p.sequence(seq).unwrap().video_tracks[0].id;
        let mut it = p
            .make_track_item(
                media,
                TrackKind::Video,
                FrameRate::FPS_60.tick_of(60),
                TimeRange::new(FrameRate::FPS_60.tick_of(240), FrameRate::FPS_60.tick_of(240)),
                FrameRate::FPS_60,
            )
            .unwrap();
        it.speed = 2.0;
        it.duration = FrameRate::FPS_60.tick_of(120);
        for fx in &mut it.effects {
            filmcraft_project::resolve_auto_points(fx, (320, 180), (640, 360));
        }
        let motion = it.effect_mut("motion").unwrap();
        motion.params.get_mut("scale").unwrap().value = ParamValue::Float(40.0);
        motion.params.get_mut("rotation").unwrap().value = ParamValue::Float(12.0);
        motion.params.get_mut("position").unwrap().value = ParamValue::Vec2(Vec2::new(170.0, 95.0));
        let clip = it.id;
        p.sequence_mut(seq).unwrap().video_tracks[0].items.push(it);
        s.project = Arc::new(p);
        s.state.active_sequence = Some(seq);
        s.state.open_sequences = vec![seq];
        s.execute("playhead.set", json!({"time":FrameRate::FPS_60.tick_of(120).0})).unwrap();
        s.execute("timeline.select", json!({"clips":[clip.0]})).unwrap();
        s.execute("effects.apply", json!({"clips":[clip.0],"effect":"dynamic_zoom"})).unwrap();
        s.execute("effects.setParam", json!({"clip":clip.0,"effect":"dynamic_zoom","param":"end_width","value":0.85})).unwrap();
        s.execute("effects.setParam", json!({"clip":clip.0,"effect":"dynamic_zoom","param":"end_center","value":[0.55,0.5]})).unwrap();
        let target = if track_fx { EffectTarget::VideoTrack(track) } else { EffectTarget::Clip(clip) };
        let mut args = json!({"effect":"vignette_blur"});
        put_target(&mut args, target);
        s.execute("effects.apply", args).unwrap();
        let stack = if track_fx {
            &s.active_sequence().unwrap().track(track).unwrap().effects
        } else {
            &s.active_sequence().unwrap().find_item(clip).unwrap().1.effects
        };
        let index = stack.iter().position(|e| e.effect == "vignette_blur").unwrap();
        for (param, value) in [
            ("center", if track_fx { json!([160.0, 90.0]) } else { json!([320.0, 180.0]) }),
            ("width", json!(0.5)),
            ("height", json!(0.5)),
            ("angle", json!(0.0)),
        ] {
            let mut args = json!({"effect":index,"param":param,"value":value});
            put_target(&mut args, target);
            s.execute("effects.setParam", args).unwrap();
        }
        let mut selected = json!({"effect":index});
        put_target(&mut selected, target);
        s.execute("effects.select", selected).unwrap();
        let mut app = FilmcraftApp::new(s);
        app.show_panel(filmcraft_ui_egui::dock::PanelKind::EffectControls);
        for (i, fx) in app.session.active_sequence().unwrap().find_item(clip).unwrap().1.effects.iter().enumerate() {
            if fx.effect != "vignette_blur" {
                app.ui.collapsed_fx.push(format!("{}:{i}", clip.0));
            }
        }
        let mut b = Harness::builder().with_size(vec2(1440.0, 960.0)).with_step_dt(1.0 / 60.0).with_pixels_per_point(1.0);
        if std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR").is_some() {
            b = b.wgpu();
        }
        let mut h = b.build_eframe(move |_| app);
        h.run_steps(5);
        let mut sources = filmcraft_render::SourceMap::default();
        sources.0.insert(media, Arc::new(source()));
        Self { h, clip, track, target, index, sources }
    }
    fn params(&self, param: &str, value: Value) -> Value {
        let mut v = json!({"effect":self.index,"param":param,"value":value});
        put_target(&mut v, self.target);
        v
    }
    fn set(&mut self, param: &str, value: Value) {
        let v = self.params(param, value);
        self.h.state_mut().session.execute("effects.setParam", v).unwrap();
        self.h.run_steps(3);
    }
    fn stack(&self) -> &[EffectInstance] {
        match self.target {
            EffectTarget::Clip(c) => &self.h.state().session.active_sequence().unwrap().find_item(c).unwrap().1.effects,
            EffectTarget::VideoTrack(t) => &self.h.state().session.active_sequence().unwrap().track(t).unwrap().effects,
        }
    }
    fn fx(&self) -> &EffectInstance {
        &self.stack()[self.index]
    }
    fn p(&self, id: &str) -> Pos2 {
        let r = self.h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}")).rect;
        pos2(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5)
    }
    fn button(&mut self, p: Pos2, pressed: bool) {
        self.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        self.h.step();
    }
    fn drag(&mut self, id: &str, delta: egui::Vec2) {
        let p = self.p(id);
        self.h.event(Event::PointerMoved(p));
        self.h.step();
        self.button(p, true);
        for f in [0.4, 0.7, 1.0] {
            self.h.event(Event::PointerMoved(p + delta * f));
            self.h.step();
        }
        self.button(p + delta, false);
        self.h.run_steps(3);
    }
    fn click(&mut self, id: &str) {
        let p = self.p(id);
        self.h.event(Event::PointerMoved(p));
        self.h.step();
        self.button(p, true);
        self.button(p, false);
        self.h.run_steps(3);
    }
    fn image(&self, p: &Project) -> filmcraft_render::Image {
        filmcraft_render::render_sequence(
            p,
            self.h.state().session.state.active_sequence.unwrap(),
            self.h.state().session.playhead(),
            Default::default(),
            &self.sources,
        )
    }
}
fn put_target(value: &mut Value, target: EffectTarget) {
    match target {
        EffectTarget::Clip(c) => value["clip"] = json!(c.0),
        EffectTarget::VideoTrack(t) => value["track"] = json!(t.0),
    }
}

#[test]
fn drag_center_width_height_rotation_softness() {
    for track in [false, true] {
        let mut d = Driver::new(track);
        let original = d.fx().params["center"].value.clone();
        d.drag("program.vignette.center", vec2(20.0, 12.0));
        let center = d.fx().params["center"].value.as_vec2().unwrap();
        assert_ne!(d.fx().params["center"].value, original);
        assert!(if track { center.x > 160.0 && center.x < 210.0 } else { center.x > 320.0 && center.x < 420.0 }, "target coordinate units: {center:?}");
        for (id, delta, param) in [
            ("width", vec2(30.0, 0.0), "width"),
            ("height", vec2(0.0, 25.0), "height"),
            ("rotation", vec2(30.0, 30.0), "angle"),
            ("softness", vec2(30.0, -20.0), "softness"),
        ] {
            let project = d.h.state().session.project.clone();
            let before = d.fx().params[param].value.clone();
            d.drag(&format!("program.vignette.{id}"), delta);
            assert_ne!(d.fx().params[param].value, before, "{id}");
            d.h.state_mut().session.execute("edit.undo", json!({})).unwrap();
            assert_eq!(*d.h.state().session.project, *project);
            d.h.state_mut().session.execute("edit.redo", json!({})).unwrap();
            d.h.run_steps(3);
        }
        for param in ["width", "height"] {
            assert!((0.0..=1.0).contains(&d.fx().f64_at(param, FrameRate::FPS_60.tick_of(120))));
        }
        assert!((0.0..=0.25).contains(&d.fx().f64_at("softness", FrameRate::FPS_60.tick_of(120))));
        assert!(d.fx().params.values().all(|p| p.keyframes.is_empty()), "static values stay static");
        if let Some(dir) = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR") {
            loop_picture(&mut d, false);
            std::fs::create_dir_all(&dir).unwrap();
            d.h.render().unwrap().save(std::path::PathBuf::from(dir).join(if track { "vignette-track-fit.png" } else { "vignette-clip-fit.png" })).unwrap();
        }
    }
}

#[test]
fn oval_and_round_rect_handles_match_mask() {
    let mut d = Driver::new(false);
    d.h.state_mut().ui.program.zoom = Some(2.0);
    d.h.state_mut().ui.program.pan = [-30.0, 18.0];
    for (shape, expected) in [(0, Vec2::new(444.4507935, 109.9964287)), (1, Vec2::new(481.0918831, 88.9081169))] {
        d.set("shape", json!(shape));
        let q = d.h.state().session.active_sequence().unwrap();
        let it = q.find_item(d.clip).unwrap().1;
        let m = filmcraft_render::dynamic_zoom::clip_matrix(&d.h.state().session.project, q, it, d.h.state().session.playhead(), true).unwrap();
        let difference = m.apply(expected) - m.apply(Vec2::new(320.0, 180.0));
        let actual = d.p("program.vignette.softness");
        let center = d.p("program.vignette.center");
        let expected = center + vec2(difference.x as f32 * 2.0, difference.y as f32 * 2.0);
        assert!(actual.distance(expected) < 0.02, "shape {shape}: {actual:?} vs {expected:?}");
        if let Some(dir) = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            d.h.render().unwrap().save(std::path::PathBuf::from(dir).join(format!("vignette-shape-{shape}-zoom-pan.png"))).unwrap();
        }
    }
}

#[test]
fn animated_param_writes_at_playhead() {
    for track in [false, true] {
        let mut d = Driver::new(track);
        let mut args = d.params("width", Value::Null);
        args.as_object_mut().unwrap().remove("value");
        d.h.state_mut().session.execute("effects.toggleAnimation", args).unwrap();
        d.h.run_steps(3);
        d.drag("program.vignette.width", vec2(25.0, 0.0));
        let keys = &d.fx().params["width"].keyframes;
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].time, FrameRate::FPS_60.tick_of(if track { 120 } else { 360 }), "sequence ticks vs source mapping");
        assert_ne!(keys[0].value, ParamValue::Float(0.5));
    }
}

#[test]
fn track_keyframe_navigation_does_not_overlap_sequence_keys() {
    let mut d = Driver::new(true);
    let mut args = d.params("width", Value::Null);
    args.as_object_mut().unwrap().remove("value");
    d.h.state_mut().session.execute("effects.toggleAnimation", args).unwrap();
    let mut args = d.params("width", json!(0.3));
    args["time"] = json!(0);
    args["keyframe"] = json!(true);
    d.h.state_mut().session.execute("effects.setParam", args).unwrap();
    d.h.run_steps(3);
    let base = format!("effectControls.track.{}.effect.{}.param.width", d.track.0, d.index);
    let rect = |id: &str| {
        let r = d.h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}")).rect;
        egui::Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]))
    };
    let key = rect(&format!("{base}.key.0"));
    for control in ["previous", "next", "keyframe"] {
        let nav = rect(&format!("{base}.{control}"));
        assert!(!key.intersects(nav), "{control} overlaps draggable sequence key: {nav:?} vs {key:?}");
    }
}

#[test]
fn held_frame_filters_use_the_renderers_effect_time() {
    for hold_filters in [false, true] {
        let mut d = Driver::new(false);
        let clip = d.clip;
        d.h.state_mut()
            .session
            .edit_sequence("fixture hold", |q, _, _| {
                let it = q.find_item_mut(clip).unwrap().1;
                it.frame_hold = Some(FrameRate::FPS_60.tick_of(300));
                it.hold_filters = hold_filters;
                Ok(())
            })
            .unwrap();
        let mut args = d.params("width", Value::Null);
        args.as_object_mut().unwrap().remove("value");
        d.h.state_mut().session.execute("effects.toggleAnimation", args).unwrap();
        d.h.run_steps(3);
        d.drag("program.vignette.width", vec2(20.0, 0.0));
        let keys = &d.fx().params["width"].keyframes;
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].time, FrameRate::FPS_60.tick_of(if hold_filters { 300 } else { 360 }), "hold_filters {hold_filters}");
        assert_ne!(keys[0].value, ParamValue::Float(0.5));
    }
}

#[test]
fn draft_preview_and_escape_keep_other_edits() {
    for track in [false, true] {
        let mut d = Driver::new(track);
        let normal = loop_picture(&mut d, false);
        let before = d.h.state().frames.get(&normal).unwrap();
        let project = d.h.state().session.project.clone();
        let fx = d.fx().clone();
        let p = d.p("program.vignette.center");
        d.h.event(Event::PointerMoved(p));
        d.h.step();
        d.button(p, true);
        d.h.event(Event::PointerMoved(p + vec2(35.0, 16.0)));
        d.h.run_steps(2);
        assert!(Arc::ptr_eq(&project, &d.h.state().session.project));
        let preview = loop_picture(&mut d, true);
        let preview = d.h.state().frames.get(&preview).unwrap();
        assert_eq!((before.w, before.h), (preview.w, preview.h));
        assert_ne!(before.px, preview.px, "blur follows draft before committing the model");
        d.h.state_mut().session.execute("timeline.setTrack", json!({"track":d.track.0,"name":"Independent edit"})).unwrap();
        d.h.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        d.h.step();
        d.button(p + vec2(35.0, 16.0), false);
        d.h.run_steps(3);
        assert_eq!(d.fx(), &fx);
        assert_eq!(d.h.state().session.active_sequence().unwrap().track(d.track).unwrap().name, "Independent edit");
    }
}
fn loop_picture(d: &mut Driver, preview: bool) -> filmcraft_ui_egui::frames::FrameKey {
    for _ in 0..100 {
        if let Some(key) = d.h.state().program_picture() {
            let is_preview = matches!(key.target, filmcraft_ui_egui::frames::Target::EffectPreview(_));
            if is_preview == preview && (preview || key.revision == d.h.state().session.revision) && d.h.state().frames.get(&key).is_some() {
                return key;
            }
        }
        d.h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    panic!("missing ready picture; preview={preview}");
}

#[test]
fn singular_transform_and_deleted_target_cancel_cleanly() {
    let mut d = Driver::new(false);
    d.h.state_mut().session.execute("effects.setParam", json!({"clip":d.clip.0,"effect":"motion","param":"scale","value":0.0})).unwrap();
    d.h.run_steps(3);
    assert!(d.h.state().auto.elements.iter().all(|e| !e.id.starts_with("program.vignette.")));
    assert!(d.h.state().auto.find("effectControls.vignette_blur.width.reset").is_some(), "numeric control remains available");
    d.h.state_mut().session.execute("effects.setParam", json!({"clip":d.clip.0,"effect":"motion","param":"scale","value":40.0})).unwrap();
    d.h.run_steps(3);
    let p = d.p("program.vignette.center");
    d.h.event(Event::PointerMoved(p));
    d.h.step();
    d.button(p, true);
    d.h.event(Event::PointerMoved(p + vec2(25.0, 10.0)));
    d.h.run_steps(2);
    let mut args = json!({"index":d.index});
    put_target(&mut args, d.target);
    d.h.state_mut().session.execute("effects.remove", args).unwrap();
    d.h.run_steps(2);
    let project = d.h.state().session.project.clone();
    d.button(p + vec2(25.0, 10.0), false);
    d.h.run_steps(3);
    assert!(Arc::ptr_eq(&project, &d.h.state().session.project));
    assert!(d.h.state().auto.elements.iter().all(|e| !e.id.starts_with("program.vignette.")));
}

#[test]
fn second_instance_bypass_and_show_controls_preserve_parameter_ownership() {
    for track in [false, true] {
        let mut d = Driver::new(track);
        let mut args = json!({"effect":"vignette_blur"});
        put_target(&mut args, d.target);
        d.h.state_mut().session.execute("effects.apply", args).unwrap();
        d.index = d.stack().iter().enumerate().filter(|(_, e)| e.effect == "vignette_blur").nth(1).unwrap().0;
        d.set("center", if track { json!([160.0, 90.0]) } else { json!([320.0, 180.0]) });
        let first = d.stack()[d.index - 1].clone();
        let mut selected = json!({"effect":d.index});
        put_target(&mut selected, d.target);
        d.h.state_mut().session.execute("effects.select", selected).unwrap();
        if !track {
            let keys = d.stack().iter().enumerate().filter(|(i, _)| *i != d.index).map(|(i, _)| format!("{}:{i}", d.clip.0)).collect();
            d.h.state_mut().ui.collapsed_fx = keys;
        }
        d.h.state_mut().session.execute("timeline.setVideoFxBypass", json!({"track":d.track.0,"bypass":true})).unwrap();
        d.h.run_steps(3);
        d.drag("program.vignette.center", vec2(15.0, 7.0));
        assert_eq!(&d.stack()[d.index - 1], &first);
        assert!(d.h.state().session.active_sequence().unwrap().track(d.track).unwrap().video_effects_bypassed);
        d.click("effectControls.vignetteBlur.showControls");
        assert!(d.h.state().auto.elements.iter().all(|e| !e.id.starts_with("program.vignette.")));
        d.click("effectControls.vignetteBlur.showControls");
        assert!(d.h.state().auto.find("program.vignette.center").is_some());
    }
}

#[test]
fn clear_blur_boundary_agrees_with_the_width_handle_within_one_output_pixel() {
    for track in [false, true] {
        let mut d = Driver::new(track);
        d.h.state_mut().session.execute("effects.setParam", json!({"clip":d.clip.0,"effect":"motion","param":"rotation","value":0.0})).unwrap();
        let q = d.h.state().session.active_sequence().unwrap();
        let it = q.find_item(d.clip).unwrap().1;
        let edge = filmcraft_render::dynamic_zoom::clip_matrix(&d.h.state().session.project, q, it, d.h.state().session.playhead(), true)
            .unwrap()
            .apply(Vec2::new(640.0 * 5.0 / 7.0, 180.0));
        d.set("width", json!(if track { (edge.x - 160.0) * 2.0 / 320.0 } else { 3.0 / 7.0 }));
        for shape in [0, 1] {
            d.set("shape", json!(shape));
            let p = d.p("program.vignette.width");
            let picture = d.h.state().auto.find("program.picture").unwrap().rect;
            let point = Vec2::new(f64::from((p.x - picture[0]) / picture[2] * 320.0), f64::from((p.y - picture[1]) / picture[3] * 180.0));
            let project = d.h.state().session.project.clone();
            let actual = d.image(&project);
            let mut plain = (*project).clone();
            stack_mut(&mut plain, &d)[d.index].enabled = false;
            let plain = d.image(&plain);
            let mut blurred = (*project).clone();
            for name in ["width", "height"] {
                stack_mut(&mut blurred, &d)[d.index].params.get_mut(name).unwrap().value = ParamValue::Float(0.0);
            }
            let blurred = d.image(&blurred);
            let mut best = (f32::INFINITY, 0.0f32);
            for step in -8..=8 {
                let delta = step as f32 / 4.0;
                let (x, y) = (point.x as f32 + delta, point.y as f32);
                let (a, b, c) = (actual.sample_bilinear(x, y), plain.sample_bilinear(x, y), blurred.sample_bilinear(x, y));
                let denom = (0..3).map(|i| (c[i] - b[i]).powi(2)).sum::<f32>();
                if denom < 1e-6 {
                    continue;
                }
                let weight = (0..3).map(|i| (a[i] - b[i]) * (c[i] - b[i])).sum::<f32>() / denom;
                let score = (weight - 0.5).abs();
                if score < best.0 {
                    best = (score, delta);
                }
            }
            assert!(best.0 < 0.12 && best.1.abs() <= 1.0, "shape {shape}, track {track}, boundary error {best:?}");
        }
    }
}
fn stack_mut<'a>(p: &'a mut Project, d: &Driver) -> &'a mut Vec<EffectInstance> {
    let q = p.sequence_mut(d.h.state().session.state.active_sequence.unwrap()).unwrap();
    match d.target {
        EffectTarget::Clip(c) => &mut q.find_item_mut(c).unwrap().1.effects,
        EffectTarget::VideoTrack(t) => &mut q.track_mut(t).unwrap().effects,
    }
}
