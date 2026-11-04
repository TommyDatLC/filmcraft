#[path = "support/video_fx.rs"]
mod support;
use filmcraft_geom::Vec2;
use filmcraft_project::{Label, ParamValue, SequenceSettings, TrackKind, resolve_auto_points};
use filmcraft_render::{RenderOptions, preview, render_sequence};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use support::*;

#[test]
fn adjustment_clips_receive_track_inserts_with_cut_and_transition() {
    for fade in [false, true] {
        let mut f = VideoFxFixture::gradient_and_black_motion();
        let adjustment = f.project.add_item(
            "Adjustment",
            Label::Iris,
            filmcraft_project::ItemKind::AdjustmentLayer { width: 320, height: 180, rate: FrameRate::FPS_60, duration: FrameRate::FPS_60.tick_of(240) },
            None,
        );
        f.clip().item = adjustment;
        f.clip().effects.retain(|e| e.def().is_some_and(|d| d.intrinsic));
        set(f.clip().effect_mut("opacity").unwrap(), "blend", ParamValue::Choice(0));
        if fade {
            let id = filmcraft_project::TransitionId(f.project.alloc_id());
            let to = f.motion;
            f.track().transitions.push(filmcraft_project::Transition {
                id,
                effect: effect("cross_dissolve"),
                from: None,
                to: Some(to),
                start: Tick::ZERO,
                duration: FrameRate::FPS_60.tick_of(60),
                reverse: false,
                align: Default::default(),
            });
        }
        let before = f.frame(30, 1.0);
        f.clip().effects.insert(0, effect("invert"));
        let expected = f.frame(30, 1.0);
        assert!(max_abs_difference(&before, &expected) > 0.1);
        f.clip().effects.remove(0);
        f.track().effects.push(effect("invert"));
        assert!(max_abs_difference(&expected, &f.frame(30, 1.0)) < 1e-6, "adjustment track insert fade={fade}");
        f.track().video_effects_bypassed = true;
        assert!(max_abs_difference(&before, &f.frame(30, 1.0)) < 1e-6);
        f.track().video_effects_bypassed = false;
        assert!(max_abs_difference(&expected, &f.planned(30, 1.0)) < 1e-6);
    }
}

#[test]
fn track_stack_preserves_lighten_over_gradient() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let before = f.frame(30, 1.0);
    f.track().effects.push(blur(16.0));
    let after = f.frame(30, 1.0);
    assert!(max_abs_difference(&before, &after) > 0.03, "track blur must reach the pixels");
    for (a, b) in before.get(10, 10).iter().zip(after.get(10, 10)) {
        assert!((a - b).abs() < 1e-5, "black motion pixels keep the gradient");
    }
    valid_alpha(&after);
}

#[test]
fn bypass_keeps_motion_opacity_masks_blend_and_disabled_flags() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    f.project.sequence_mut(f.sequence).unwrap().video_tracks[0].enabled = false;
    let clip = f.clip();
    set(clip.effect_mut("motion").unwrap(), "scale", ParamValue::Float(80.0));
    set(clip.effect_mut("motion").unwrap(), "position", ParamValue::Vec2(Vec2::new(143.0, 80.0)));
    set(clip.effect_mut("opacity").unwrap(), "opacity", ParamValue::Float(65.0));
    let mut mask = filmcraft_project::Mask::new("Oval", filmcraft_project::MaskPath::ellipse(Vec2::new(160.0, 90.0), Vec2::new(110.0, 70.0)));
    mask.feather.value = ParamValue::Float(12.0);
    clip.effect_mut("opacity").unwrap().masks.push(mask);
    let mut disabled = effect("invert");
    disabled.enabled = false;
    clip.effects.insert(0, disabled);
    clip.effects.insert(0, blur(16.0));
    f.track().effects.push(blur(10.0));
    f.track().effects.push(effect("three_layer_bloom"));
    f.track().effects.push(effect("exponential_bloom"));
    let enabled = f.frame(30, 1.0);
    valid_alpha(&enabled);
    assert!(enabled.get(319, 179)[3] < 1e-6, "transparent margins stay transparent");
    assert!(enabled.px.chunks_exact(4).any(|p| p[3] > 0.0 && p[3] < 1.0), "feather produces soft alpha");
    let saved = f.track().clone();
    f.track().video_effects_bypassed = true;
    let bypass = f.frame(30, 1.0);
    valid_alpha(&bypass);
    assert!(max_abs_difference(&enabled, &bypass) > 0.02);
    assert_eq!(f.track().items, saved.items, "bypass never changes effect data");
    let mut reference = f.project.clone();
    let tr = &mut reference.sequence_mut(f.sequence).unwrap().video_tracks[1];
    tr.video_effects_bypassed = false;
    tr.effects.clear();
    tr.items[0].effects.retain(|e| e.def().is_some_and(|d| d.intrinsic));
    let expected = render_sequence(&reference, f.sequence, FrameRate::FPS_60.tick_of(30), RenderOptions::default(), &f.sources);
    assert!(max_abs_difference(&bypass, &expected) < 1e-6);
    f.track().video_effects_bypassed = false;
    assert!(max_abs_difference(&enabled, &f.frame(30, 1.0)) < 1e-6);
}

#[test]
#[ignore = "manual single-frame 1080p allocation/time smoke"]
fn one_full_hd_track_filter_frame_is_bounded() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let q = f.project.sequence_mut(f.sequence).unwrap();
    q.settings.width = 1920;
    q.settings.height = 1080;
    f.track().effects.push(blur(12.0));
    let start = std::time::Instant::now();
    let image = f.frame(30, 1.0);
    assert_eq!((image.w, image.h), (1920, 1080));
    assert_eq!(image.px.len() * std::mem::size_of::<f32>(), 33_177_600);
    valid_alpha(&image);
    eprintln!("1080p track filter: {:?}; output RGBA buffer 33,177,600 bytes", start.elapsed());
}

#[test]
fn parent_bypass_keeps_nested_filters() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    f.project.sequence_mut(f.sequence).unwrap().video_tracks[0].enabled = false;
    f.clip().effects.insert(0, effect("invert"));
    set(f.clip().effect_mut("opacity").unwrap(), "blend", ParamValue::Choice(0));
    let inner = f.sequence;
    let reference = f.frame(30, 1.0);
    let settings = SequenceSettings { width: 320, height: 180, frame_rate: FrameRate::FPS_60, ..Default::default() };
    let outer = f.project.new_sequence("parent", settings, 1, 0, None);
    let mut nest =
        f.project.make_track_item(inner, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, FrameRate::FPS_60.tick_of(240)), FrameRate::FPS_60).unwrap();
    for e in &mut nest.effects {
        resolve_auto_points(e, (320, 180), (320, 180));
    }
    nest.effects.insert(0, blur(20.0));
    let tr = &mut f.project.sequence_mut(outer).unwrap().video_tracks[0];
    tr.items.push(nest);
    tr.video_effects_bypassed = true;
    f.sequence = outer;
    assert!(max_abs_difference(&reference, &f.frame(30, 1.0)) < 1e-6, "parent bypass retains the child's invert and skips only parent blur");
}

#[test]
fn track_keyframe_ignores_clip_speed() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let mut e = blur(0.0);
    let p = e.param_mut("blurriness").unwrap();
    p.put_keyframe(Tick::ZERO, ParamValue::Float(0.0));
    p.put_keyframe(FrameRate::FPS_60.tick_of(120), ParamValue::Float(24.0));
    f.track().effects.push(e);
    let early = f.frame(0, 1.0);
    let middle = f.frame(60, 1.0);
    assert!(max_abs_difference(&early, &middle) > 0.03, "track animation must run");
    let clip = f.clip();
    clip.source_in = FrameRate::FPS_60.tick_of(24);
    clip.speed = 0.5;
    clip.reverse = true;
    clip.start = FrameRate::FPS_60.tick_of(10);
    assert!(max_abs_difference(&middle, &f.frame(60, 1.0)) < 1e-6);
}

fn transition_fixture(id: &str, mixed: bool) -> VideoFxFixture {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let rate = FrameRate::FPS_60;
    let item = f.clip().item;
    f.clip().duration = rate.tick_of(120);
    set(f.clip().effect_mut("opacity").unwrap(), "blend", ParamValue::Choice(if mixed { 0 } else { 7 }));
    let mut second = f.project.make_track_item(item, TrackKind::Video, rate.tick_of(120), TimeRange::new(Tick::ZERO, rate.tick_of(120)), rate).unwrap();
    for e in &mut second.effects {
        resolve_auto_points(e, (320, 180), (320, 180));
    }
    set(second.effect_mut("opacity").unwrap(), "blend", ParamValue::Choice(7));
    let to = second.id;
    let from = f.motion;
    let tid = filmcraft_project::TransitionId(f.project.alloc_id());
    let tr = f.track();
    tr.items.push(second);
    tr.transitions.push(filmcraft_project::Transition {
        id: tid,
        effect: effect(id),
        start: rate.tick_of(90),
        duration: rate.tick_of(60),
        from: Some(from),
        to: Some(to),
        align: Default::default(),
        reverse: false,
    });
    f
}

#[test]
fn lighten_transition_keeps_background() {
    for id in ["cross_dissolve", "push", "slide", "cross_zoom", "morph_cut"] {
        let mut f = transition_fixture(id, false);
        f.track().effects.push(blur(4.0));
        f.track().enabled = false;
        let background = f.frame(120, 1.0);
        f.track().enabled = true;
        for reverse in [false, true] {
            f.track().transitions[0].reverse = reverse;
            for frame in [90, 105, 120, 135, 149, 150] {
                let image = f.frame(frame, 1.0);
                for (pixel, bg) in image.px.chunks_exact(4).zip(background.px.chunks_exact(4)) {
                    for channel in 0..3 {
                        assert!(pixel[channel] + 1e-5 >= bg[channel], "{id} at {frame}: Lighten cannot darken the background");
                    }
                }
                assert!(max_abs_difference(&image, &f.planned(frame, 1.0)) < 1e-6, "preview parity for {id}");
            }
        }
    }
}

#[test]
fn mixed_blend_transition_has_exact_branch_endpoints() {
    let mut f = transition_fixture("cross_dissolve", true);
    f.track().effects.push(blur(6.0));
    let options = RenderOptions { working_output: true, ..Default::default() };
    let clips = f.track().items.clone();
    let branch = |clip: filmcraft_project::TrackItem, frame: i64| {
        let mut project = f.project.clone();
        let tr = &mut project.sequence_mut(f.sequence).unwrap().video_tracks[1];
        let mut clip = clip;
        clip.start = Tick::ZERO;
        clip.duration = FrameRate::FPS_60.tick_of(240);
        tr.items = vec![clip];
        tr.transitions.clear();
        render_sequence(&project, f.sequence, FrameRate::FPS_60.tick_of(frame), options, &f.sources)
    };
    for (frame, progress) in [(90, 0.0), (120, 0.5), (150, 1.0)] {
        let expected = branch(clips[0].clone(), frame).lerp(&branch(clips[1].clone(), frame), progress);
        let actual = render_sequence(&f.project, f.sequence, FrameRate::FPS_60.tick_of(frame), options, &f.sources);
        assert!(max_abs_difference(&expected, &actual) < 1e-6, "mixed blend at {progress}");
    }
}

#[test]
fn sequence_timed_track_stack_changes_hash_when_the_clip_moves() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    f.track().effects.push(blur(6.0));
    f.project.sequence_mut(f.sequence).unwrap().video_tracks[0].enabled = false;
    let before = preview::video_segments(&f.project, f.sequence)[0].hash.clone();
    f.clip().start = FrameRate::FPS_60.tick_of(120);
    assert_ne!(before, preview::video_segments(&f.project, f.sequence)[0].hash);
}

#[test]
fn nested_track_stack_changes_pixels_hash_and_preview_cost() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let inner = f.sequence;
    let outer = f.project.new_sequence("outer", SequenceSettings { width: 320, height: 180, frame_rate: FrameRate::FPS_60, ..Default::default() }, 1, 0, None);
    let mut clip =
        f.project.make_track_item(inner, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, FrameRate::FPS_60.tick_of(240)), FrameRate::FPS_60).unwrap();
    for e in &mut clip.effects {
        resolve_auto_points(e, (320, 180), (320, 180));
    }
    f.project.sequence_mut(outer).unwrap().video_tracks[0].items.push(clip);
    let before = preview::video_segments(&f.project, outer);
    let image = render_sequence(&f.project, outer, FrameRate::FPS_60.tick_of(30), RenderOptions::default(), &f.sources);
    f.track().effects.push(blur(16.0));
    let after = preview::video_segments(&f.project, outer);
    let changed = render_sequence(&f.project, outer, FrameRate::FPS_60.tick_of(30), RenderOptions::default(), &f.sources);
    assert!(max_abs_difference(&image, &changed) > 0.03);
    assert_ne!(before[0].hash, after[0].hash);
    assert!(after[0].cost_ms > before[0].cost_ms, "nested track filters contribute to the render bar");
}

#[test]
fn cpu_plan_matches_export_path_and_empty_track_is_transparent() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let before = f.frame(30, 1.0);
    f.project.sequence_mut(f.sequence).unwrap().video_tracks[2].effects.push(effect("invert"));
    assert!(max_abs_difference(&before, &f.frame(30, 1.0)) < 1e-6);
    f.track().effects.push(blur(12.0));
    for bypass in [false, true] {
        f.track().video_effects_bypassed = bypass;
        for scale in [1.0, 0.5, 0.25] {
            let cpu = f.frame(30, scale);
            let plan = f.planned(30, scale);
            assert!(max_abs_difference(&cpu, &plan) < 1e-6, "CPU and preview plan agree");
            valid_alpha(&cpu);
        }
    }
}

#[test]
fn track_fx_change_invalidates_segment_hash() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let before = preview::video_segments(&f.project, f.sequence);
    f.track().effects.push(blur(12.0));
    let after = preview::video_segments(&f.project, f.sequence);
    assert_ne!(before[0].hash, after[0].hash);
    assert!(after[0].cost_ms > before[0].cost_ms);
    f.track().video_effects_bypassed = true;
    assert_ne!(after[0].hash, preview::video_segments(&f.project, f.sequence)[0].hash);
    f.track().items[0].label = Label::Mango;
    let hash = preview::video_segments(&f.project, f.sequence)[0].hash.clone();
    f.track().name = "renamed".into();
    assert_eq!(hash, preview::video_segments(&f.project, f.sequence)[0].hash);
}
