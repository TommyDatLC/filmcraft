//! Behavior of video-track inserts, inspector targets and reversible FX bypass.
use super::*;
use filmcraft_project::{Track, TrackId, TrackKind};
use serde_json::json;

fn demo_track() -> (Session, TrackId, ClipId) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let q = s.active_sequence().unwrap();
    let track = q.video_tracks[0].id;
    let clip = q.video_tracks[0].items[0].id;
    (s, track, clip)
}
fn track(s: &Session, id: TrackId) -> &Track {
    s.active_sequence().unwrap().track(id).unwrap()
}
fn add_blur(s: &mut Session, id: TrackId) {
    s.execute("effects.apply", json!({"track":id.0,"effect":"gaussian_blur"})).unwrap();
}

#[test]
fn held_clip_parameter_commands_follow_the_effect_clock() {
    for hold_filters in [false, true] {
        let (mut s, _, clip) = demo_track();
        let rate = s.active_sequence().unwrap().settings.frame_rate;
        let held = rate.tick_of(6);
        let moving = rate.tick_of(36);
        s.edit_sequence("fixture hold", |q, _, _| {
            let item = q.find_item_mut(clip).unwrap().1;
            item.source_in = Tick::ZERO;
            item.frame_hold = Some(held);
            item.hold_filters = hold_filters;
            Ok(())
        })
        .unwrap();
        s.execute("effects.apply", json!({"clips":[clip.0],"effect":"gaussian_blur"})).unwrap();
        s.execute("playhead.set", json!({"time":moving.0})).unwrap();
        s.execute("effects.toggleAnimation", json!({"clip":clip.0,"effect":"gaussian_blur","param":"blurriness"})).unwrap();
        s.execute("effects.setParam", json!({"clip":clip.0,"effect":"gaussian_blur","param":"blurriness","value":9.0})).unwrap();
        let expected = if hold_filters { held } else { moving };
        let parameter = &s.active_sequence().unwrap().find_item(clip).unwrap().1.effect("gaussian_blur").unwrap().params["blurriness"];
        assert_eq!(parameter.keyframes[0].time, expected);
        s.execute("playhead.set", json!({"time":rate.tick_of(60).0})).unwrap();
        s.execute("effects.addKeyframe", json!({"clip":clip.0,"effect":"gaussian_blur","param":"blurriness"})).unwrap();
        let keys = &s.active_sequence().unwrap().find_item(clip).unwrap().1.effect("gaussian_blur").unwrap().params["blurriness"].keyframes;
        assert_eq!(keys.iter().map(|k| k.time).collect::<Vec<_>>(), if hold_filters { vec![] } else { vec![moving, rate.tick_of(60)] });
    }
}

#[test]
fn track_apply_params_undo_redo() {
    let (mut s, id, clip) = demo_track();
    let clip_before = s.active_sequence().unwrap().find_item(clip).unwrap().1.clone();
    add_blur(&mut s, id);
    assert_eq!(track(&s, id).effects.len(), 1);
    s.execute("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":17.0})).unwrap();
    assert_eq!(track(&s, id).effects[0].f64_at("blurriness", Tick::ZERO), 17.0);
    s.execute("effects.toggleEnabled", json!({"track":id.0,"index":0})).unwrap();
    assert!(!track(&s, id).effects[0].enabled);
    let before = s.project.clone();
    s.execute("timeline.setVideoFxBypass", json!({"track":id.0,"bypass":true})).unwrap();
    assert_eq!(serde_json::to_value(track(&s, id)).unwrap()["video_effects_bypassed"], true);
    assert!(!track(&s, id).effects[0].enabled);
    assert_eq!(s.active_sequence().unwrap().find_item(clip).unwrap().1, &clip_before);
    let after = s.project.clone();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.project, *before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*s.project, *after);
}

#[test]
fn track_keyframes_use_sequence_ticks_after_clip_move() {
    let (mut s, id, clip) = demo_track();
    let rate = s.active_sequence().unwrap().settings.frame_rate;
    add_blur(&mut s, id);
    for (frame, value) in [(120, 20.0), (240, 40.0)] {
        s.execute("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":value,"time":rate.tick_of(frame).0,"keyframe":true}))
            .unwrap();
    }
    s.edit_sequence("Retime clip", |q, _, _| {
        let (_, it) = q.find_item_mut(clip).unwrap();
        it.source_in = rate.tick_of(4);
        it.speed = 0.5;
        it.reverse = true;
        Ok(())
    })
    .unwrap();
    let effect = &track(&s, id).effects[0];
    let param = effect.param("blurriness").unwrap();
    assert_eq!(param.keyframes.iter().map(|k| k.time).collect::<Vec<_>>(), vec![rate.tick_of(120), rate.tick_of(240)]);
    assert!((effect.f64_at("blurriness", rate.tick_of(180)) - 30.0).abs() < 1e-9);
}

#[test]
fn reject_ambiguous_audio_locked_missing_and_large_index_targets() {
    let (mut s, id, clip) = demo_track();
    let audio = s.active_sequence().unwrap().audio_tracks[0].id;
    add_blur(&mut s, id);
    for (cmd, p) in [
        ("effects.apply", json!({"track":id.0,"clips":[clip.0],"effect":"gaussian_blur"})),
        ("effects.apply", json!({"track":audio.0,"effect":"gaussian_blur"})),
        ("effects.apply", json!({"track":u64::MAX,"effect":"gaussian_blur"})),
        ("effects.apply", json!({"track":id.0,"effect":"motion"})),
        ("effects.apply", json!({"track":id.0,"effect":"echo"})),
        ("effects.apply", json!({"track":id.0,"effect":"cross_dissolve"})),
        ("effects.remove", json!({"track":id.0,"index":u64::MAX})),
        ("effects.reset", json!({"track":id.0,"index":u64::MAX})),
        ("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":"NaN"})),
        ("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":"Infinity"})),
        ("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":1e99})),
    ] {
        let before = s.project.clone();
        let undo = s.history.undo.len();
        assert!(s.execute(cmd, p).is_err(), "{cmd} must reject invalid track request");
        assert_eq!(*s.project, *before);
        assert_eq!(s.history.undo.len(), undo);
    }
    s.execute("timeline.setTrack", json!({"track":id.0,"locked":true})).unwrap();
    assert!(s.execute("effects.apply", json!({"track":id.0,"effect":"gaussian_blur"})).is_err());
}

#[test]
fn same_id_instances_keep_selection_after_reorder() {
    let (mut s, id, _) = demo_track();
    add_blur(&mut s, id);
    add_blur(&mut s, id);
    for (effect, value) in [(0, 2.0), (1, 9.0)] {
        s.execute("effects.setParam", json!({"track":id.0,"effect":effect,"param":"blurriness","value":value})).unwrap();
    }
    s.execute("effects.select", json!({"track":id.0,"effect":1})).unwrap();
    s.execute("effects.reorder", json!({"track":id.0,"from":1,"to":0})).unwrap();
    assert_eq!(track(&s, id).effects[0].f64_at("blurriness", Tick::ZERO), 9.0);
    assert_eq!(track(&s, id).effects[1].f64_at("blurriness", Tick::ZERO), 2.0);
    assert_eq!(s.state.selected_effect.as_ref().unwrap().index(&track(&s, id).effects), Some(0));
    s.execute("effects.remove", json!({"track":id.0,"index":0})).unwrap();
    assert!(s.state.selected_effect.is_none());
    assert_eq!(track(&s, id).effects.len(), 1);
}

#[test]
fn legacy_effect_selection_and_track_defaults_deserialize() {
    let old = json!({"clip":123,"effect":"motion","instance":0});
    let selected: EffectSel = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(selected).unwrap(), old, "legacy clip wire format remains compatible");
    let mut legacy = serde_json::to_value(Track::new(TrackId(7), TrackKind::Video, "V1".into())).unwrap();
    legacy.as_object_mut().unwrap().remove("video_effects_bypassed");
    let restored: Track = serde_json::from_value(legacy).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap()["video_effects_bypassed"], false);
}

#[test]
fn track_parameter_drag_merges_to_single_undo() {
    let (mut s, id, _) = demo_track();
    add_blur(&mut s, id);
    let undo = s.history.undo.len();
    for (i, value) in [3.0, 6.0, 9.0].into_iter().enumerate() {
        s.execute("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":value,"merge":"trackGizmo","begin":i==0})).unwrap();
    }
    assert_eq!(s.history.undo.len(), undo + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(track(&s, id).effects[0].f64_at("blurriness", Tick::ZERO), 0.0);
}

#[test]
fn track_and_clip_keys_move_together_with_their_own_time_domains() {
    let (mut s, id, clip) = demo_track();
    add_blur(&mut s, id);
    let rate = s.sequence_rate();
    s.execute("effects.setParam", json!({"track":id.0,"effect":0,"param":"blurriness","value":20.0,"time":rate.tick_of(24).0,"keyframe":true})).unwrap();
    s.execute("effects.setParam", json!({"clip":clip.0,"effect":"motion","param":"scale","value":110.0,"time":rate.tick_of(24).0,"keyframe":true})).unwrap();
    let it = s.active_sequence().unwrap().find_item(clip).unwrap().1;
    let source_time = it.source_time_at(rate.tick_of(24));
    let effect = it.effects.iter().position(|e| e.effect == "motion").unwrap();
    let before = s.project.clone();
    let undo = s.history.undo.len();
    let result = s
        .execute(
            "effects.moveKeyframes",
            json!({"keyframes":[
        {"track":id.0,"effect":0,"param":"blurriness","time":rate.tick_of(24).0},
        {"clip":clip.0,"effect":effect,"param":"scale","time":source_time.0}
    ],"delta":rate.tick_of(6).0}),
        )
        .unwrap();
    assert_eq!(result["delta"], rate.tick_of(6).0);
    assert_eq!(track(&s, id).effects[0].params["blurriness"].keyframes[0].time, rate.tick_of(30));
    let it = s.active_sequence().unwrap().find_item(clip).unwrap().1;
    assert_eq!(it.effects[effect].params["scale"].keyframes[0].time, source_time + rate.tick_of(6));
    assert_eq!(s.history.undo.len(), undo + 1);
    s.undo();
    assert_eq!(*s.project, *before);
}

#[test]
fn track_selection_and_clip_selection_are_exclusive() {
    let (mut s, id, clip) = demo_track();
    s.execute("timeline.select", json!({"clips":[clip.0]})).unwrap();
    s.execute("timeline.selectVideoTrack", json!({"track":id.0})).unwrap();
    assert!(s.state.selection.is_empty());
    assert_eq!(s.state.effect_controls_target, Some(EffectTarget::VideoTrack(id)));
    s.execute("timeline.select", json!({"clips":[clip.0]})).unwrap();
    assert!(s.state.effect_controls_target.is_none());
    s.execute("timeline.selectVideoTrack", json!({"track":id.0})).unwrap();
    s.execute("edit.selectAll", json!({})).unwrap();
    assert!(s.state.selection.contains(&clip));
    assert!(s.state.effect_controls_target.is_none());
    s.execute("timeline.selectVideoTrack", json!({"track":id.0})).unwrap();
    s.execute("edit.deselectAll", json!({})).unwrap();
    assert!(s.state.selection.is_empty());
    assert!(s.state.effect_controls_target.is_none());
}
