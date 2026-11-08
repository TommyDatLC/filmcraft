use std::sync::Arc;

use filmcraft_engine::{EffectTarget, Session};
use filmcraft_media::{Generator, generators::GeneratorSource};
use filmcraft_project::{ClipId, ItemId, Label, Project, SequenceSettings, TrackId, TrackKind};
use filmcraft_render::{Image, RenderOptions, plan};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use serde_json::json;

fn fixture() -> (Session, ItemId, TrackId, ClipId) {
    let mut s = Session::default();
    let mut p = Project::new("Video FX roundtrip");
    let rate = FrameRate::FPS_60;
    let seq = p.new_sequence("Synthetic", SequenceSettings { width: 320, height: 180, frame_rate: rate, ..Default::default() }, 2, 0, None);
    let mut foreground = ClipId(0);
    for (lane, generator) in [Generator::ColorMatte { color: [0.04, 0.2, 0.08, 1.0] }, Generator::BarsAndTone].into_iter().enumerate() {
        let source = GeneratorSource::new(generator, 320, 180, rate, rate.tick_of(120));
        let id = filmcraft_engine::demo::add_generator(&mut p, &s.media, source, "Synthetic generator", Label::Iris, None);
        let mut item = p.make_track_item(id, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, rate.tick_of(60)), rate).unwrap();
        for effect in &mut item.effects {
            filmcraft_project::resolve_auto_points(effect, (320, 180), (320, 180));
        }
        foreground = item.id;
        p.sequence_mut(seq).unwrap().video_tracks[lane].items.push(item);
    }
    let track = p.sequence(seq).unwrap().video_tracks[1].id;
    s.project = Arc::new(p);
    s.state.active_sequence = Some(seq);
    s.state.open_sequences = vec![seq];
    s.execute("effects.setParam", json!({"clip":foreground.0,"effect":"opacity","param":"blend","value":7})).unwrap();
    s.execute("effects.apply", json!({"clips":[foreground.0],"effect":"dynamic_zoom"})).unwrap();
    for (param, value) in [("end_center", json!([0.65, 0.4])), ("end_width", json!(0.7)), ("easing", json!(3))] {
        s.execute("effects.setParam", json!({"clip":foreground.0,"effect":"dynamic_zoom","param":param,"value":value})).unwrap();
    }
    for effect in ["vignette_blur", "three_layer_bloom", "invert"] {
        s.execute("effects.apply", json!({"track":track.0,"effect":effect})).unwrap();
    }
    s.execute("effects.toggleEnabled", json!({"track":track.0,"index":2})).unwrap();
    (s, seq, track, foreground)
}

fn difference(a: &Image, b: &Image) -> f32 {
    assert_eq!((a.w, a.h), (b.w, b.h));
    a.px.iter().zip(&b.px).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
}

#[test]
fn track_stack_dynamic_zoom_lighten_and_bypass_survive_native_save_open() {
    let (mut original, seq, track, _) = fixture();
    let dir = std::env::temp_dir().join(format!("filmcraft-video-fx-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for bypass in [false, true] {
        original.execute("timeline.setVideoFxBypass", json!({"track":track.0,"bypass":bypass})).unwrap();
        let path = dir.join(format!("bypass-{bypass}.fcproj"));
        original.execute("file.saveAs", json!({"path":path})).unwrap();
        let mut reopened = Session::default();
        reopened.execute("file.open", json!({"path":path})).unwrap();
        assert_eq!(*original.project, *reopened.project);
        assert_eq!(reopened.active_sequence().unwrap().track(track).unwrap().video_effects_bypassed, bypass);
        assert!(!reopened.active_sequence().unwrap().track(track).unwrap().effects[2].enabled);
        for frame in [0, 30, 59] {
            let time = FrameRate::FPS_60.tick_of(frame);
            for scale in [0.5, 1.0] {
                let expected = original.try_render_program_at(scale, time).unwrap();
                let actual = reopened.try_render_program_at(scale, time).unwrap();
                assert!(difference(&expected, &actual) < 1e-6);
                let provider = reopened.media.provider(reopened.project.clone(), reopened.services.clone());
                let preview = plan::execute_cpu(&plan::plan_frame(
                    &reopened.project,
                    seq,
                    time,
                    RenderOptions { scale, ..Default::default() },
                    plan::DeviceCaps::ALL,
                    &provider,
                ));
                assert!(difference(&actual, &preview) < 1e-6, "preview/export frame {frame}");
                if scale == 1.0 {
                    let png = dir.join(format!("frame-{bypass}-{frame}.png"));
                    image::save_buffer(&png, &actual.to_rgba8(), actual.w as u32, actual.h as u32, image::ColorType::Rgba8).unwrap();
                    assert_eq!(image::open(&png).unwrap().to_rgba8().as_raw(), &actual.to_rgba8());
                }
            }
        }
        reopened.execute("timeline.setVideoFxBypass", json!({"track":track.0,"bypass":false})).unwrap();
        assert!(!reopened.active_sequence().unwrap().track(track).unwrap().effects[2].enabled);
    }
}

#[test]
fn legacy_without_new_fields_renders_with_fx_enabled() {
    let (mut s, seq, _, _) = fixture();
    s.edit_sequence("legacy fixture", |q, _, _| {
        for tr in &mut q.video_tracks {
            tr.effects.clear();
            for it in &mut tr.items {
                it.effects.retain(|e| e.effect != "dynamic_zoom");
            }
        }
        Ok(())
    })
    .unwrap();
    let expected = s.try_render_program_at(1.0, FrameRate::FPS_60.tick_of(30)).unwrap();
    let mut file: serde_json::Value = serde_json::from_slice(&filmcraft_format::encode(&s.project, false)).unwrap();
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(o) => {
                o.remove("video_effects_bypassed");
                for value in o.values_mut() {
                    strip(value);
                }
            }
            serde_json::Value::Array(a) => {
                for value in a {
                    strip(value)
                }
            }
            _ => {}
        }
    }
    strip(&mut file);
    let loaded = filmcraft_format::decode(&serde_json::to_vec(&file).unwrap()).unwrap();
    assert!(loaded.project.sequence(seq).unwrap().video_tracks.iter().all(|tr| !tr.video_effects_bypassed));
    s.project = Arc::new(loaded.project);
    assert!(difference(&expected, &s.try_render_program_at(1.0, FrameRate::FPS_60.tick_of(30)).unwrap()) < 1e-6);
}

#[test]
fn explicit_clip_effect_selection_replaces_track_inspector_target() {
    let (mut s, _, track, clip) = fixture();
    s.execute("timeline.selectVideoTrack", json!({"track":track.0})).unwrap();
    s.execute("effects.select", json!({"clip":clip.0,"effect":"motion"})).unwrap();
    assert_eq!(s.state.effect_controls_target, None);
    assert_eq!(s.state.selection, vec![clip]);
    assert_eq!(s.state.selected_effect.as_ref().unwrap().target, EffectTarget::Clip(clip));
}

#[test]
fn choosing_an_effect_keeps_other_selected_clips() {
    let (mut s, _, _, clip) = fixture();
    let background = s.active_sequence().unwrap().video_tracks[0].items[0].id;
    s.execute("timeline.select", json!({"clips":[background.0,clip.0]})).unwrap();
    let selected = s.state.selection.clone();
    s.execute("effects.select", json!({"clip":clip.0,"effect":"motion"})).unwrap();
    assert_eq!(s.state.selection, selected);
}

#[test]
fn track_animation_rejects_unsupported_mask_without_editing() {
    let (mut s, _, track, _) = fixture();
    let before = s.project.clone();
    let undo = s.history.undo.len();
    assert!(s.execute("effects.toggleAnimation", json!({"track":track.0,"effect":0,"param":"width","mask":0})).is_err());
    assert_eq!(*s.project, *before);
    assert_eq!(s.history.undo.len(), undo);
}

fn extraction_fixture() -> (Session, TrackId, ClipId) {
    let (mut session, _, track, clip) = fixture();
    session
        .edit_sequence("isolate extraction fixture", |q, _, _| {
            for tr in &mut q.video_tracks {
                tr.effects.clear();
                tr.items.retain(|item| item.id == clip);
                tr.transitions.clear();
            }
            Ok(())
        })
        .unwrap();
    session.execute("timeline.select", json!({"clips":[clip.0],"linked":false})).unwrap();
    (session, track, clip)
}

#[test]
fn nesting_and_subsequence_preserve_bypassed_clip_filters() {
    for command in ["clip.nest", "sequence.makeSubsequence"] {
        let (mut session, track, clip) = extraction_fixture();
        session.execute("effects.apply", json!({"clips":[clip.0],"effect":"invert"})).unwrap();
        session.execute("timeline.setVideoFxBypass", json!({"track":track.0,"bypass":true})).unwrap();
        let original = session.project.clone();
        let time = FrameRate::FPS_60.tick_of(30);
        let before = session.try_render_program_at(0.5, time).unwrap();
        let result = session.execute(command, json!({"name":"Bypassed extraction"})).unwrap();
        if command == "sequence.makeSubsequence" {
            session.state.active_sequence = Some(ItemId(result["sequence"].as_u64().unwrap()));
        }
        let after = session.try_render_program_at(0.5, time).unwrap();
        assert!(difference(&before, &after) < 1e-6, "{command} reactivated bypassed clip filters");
        session.execute("edit.undo", json!({})).unwrap();
        assert!(*session.project == *original);
    }
}

#[test]
fn extraction_with_track_inserts_is_rejected_without_losing_state() {
    for command in ["clip.nest", "sequence.makeSubsequence"] {
        for animated in [false, true] {
            let (mut session, track, clip) = extraction_fixture();
            session.execute("effects.apply", json!({"track":track.0,"effect":"invert"})).unwrap();
            if animated {
                let offset = FrameRate::FPS_60.tick_of(60);
                session.execute("timeline.move", json!({"moves":[{"clip":clip.0,"track":track.0,"time":offset.0}],"insert":false,"linked":false})).unwrap();
                session.execute("playhead.set", json!({"time":offset.0})).unwrap();
                session.execute("effects.toggleAnimation", json!({"track":track.0,"effect":0,"param":"blend"})).unwrap();
            }
            let before = session.project.clone();
            let undo = session.history.undo.len();
            let result = session.execute(command, json!({"name":"Unsupported extraction"}));
            assert!(result.is_err(), "{command} must preserve inserts or explicitly reject migration");
            assert!(result.unwrap_err().to_string().contains("video track effects"));
            assert!(*session.project == *before);
            assert_eq!(session.history.undo.len(), undo);
        }
    }
}

#[test]
fn gradient_layer_filter_cannot_be_applied_to_a_video_track() {
    let (mut session, _, track, _) = fixture();
    let before = session.project.clone();
    let undo = session.history.undo.len();
    assert!(session.execute("effects.apply", json!({"track":track.0,"effect":"gradient_wipe_legacy"})).is_err());
    assert!(*session.project == *before);
    assert_eq!(session.history.undo.len(), undo);
}
