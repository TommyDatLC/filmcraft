#[path = "support/video_fx.rs"]
mod support;
use filmcraft_geom::{Affine, Vec2};
use filmcraft_project::{
    ParamValue,
    video_fx::{DynamicZoomEase, DynamicZoomSettings},
};
use filmcraft_render::dynamic_zoom;
use filmcraft_time::{FrameRate, Tick};
use support::*;

fn zoom(f: &mut VideoFxFixture) {
    let mut e = effect("dynamic_zoom");
    set(&mut e, "end_center", ParamValue::Vec2(Vec2::new(0.6, 0.45)));
    set(&mut e, "end_width", ParamValue::Float(0.5));
    f.clip().effects.insert(0, e);
}

#[test]
fn edit_preview_is_the_isolated_clip_after_motion_before_zoom_track_and_opacity() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    set(f.clip().effect_mut("motion").unwrap(), "scale", ParamValue::Float(50.0));
    set(f.clip().effect_mut("opacity").unwrap(), "opacity", ParamValue::Float(25.0));
    f.track().effects.push(effect("invert"));
    let before = f.project.clone();
    let time = FrameRate::FPS_60.tick_of(239);
    let edit = dynamic_zoom::edit_frame(&f.project, f.sequence, f.motion, time, Default::default(), &f.sources).unwrap();
    let p = edit.get(160, 90);
    assert!(p[0] > 0.7 && p[1] > 0.15 && p[1] < 0.4 && p[2] < 0.05, "isolated orange source: {p:?}");
    assert!((p[3] - 1.0).abs() < 1e-6, "parent opacity excluded");
    assert_eq!(edit.get(0, 0), [0.0; 4], "no other track/background");
    assert!(max_abs_difference(&edit, &f.frame(239, 1.0)) > 0.1);
    assert_eq!(f.project, before, "preview must not change project flags");
}

#[test]
fn edit_preview_preserves_clip_reference_context_without_compositing_other_tracks() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    let mut matte = effect("track_matte");
    set(&mut matte, "matte", ParamValue::Choice(1));
    set(&mut matte, "composite", ParamValue::Choice(1));
    f.clip().effects.push(matte);
    let edit = dynamic_zoom::edit_frame(&f.project, f.sequence, f.motion, Tick::ZERO, Default::default(), &f.sources).unwrap();
    let center = edit.get(160, 90);
    assert!(center[3] > 0.1 && center[3] < 0.8, "V1 luma still drives clip filter: {center:?}");
    assert!(center[0] > center[1] && center[1] > center[2]);
    assert!(edit.get(0, 0)[0..3].iter().all(|v| v.abs() < 1e-6), "background not composited into input image");
}
fn finite(m: Affine) {
    assert!([m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite()));
}

#[test]
fn first_last_and_single_frame() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    let q = f.project.sequence(f.sequence).unwrap();
    let item = &q.video_tracks[1].items[0];
    let first = dynamic_zoom::matrix(q, item, Tick::ZERO);
    assert_eq!(first, Affine::IDENTITY);
    let last = dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(239));
    let focus = last.apply(Vec2::new(0.6 * 320.0, 0.45 * 180.0));
    assert!((focus - Vec2::new(160.0, 90.0)).length() < 1e-9);
    assert_eq!(last.a, 2.0);
    let q = f.project.sequence_mut(f.sequence).unwrap();
    q.video_tracks[1].items[0].duration = FrameRate::FPS_60.tick_of(1);
    assert_eq!(dynamic_zoom::matrix(q, &q.video_tracks[1].items[0], Tick::ZERO), Affine::IDENTITY);
}

#[test]
fn easing_values_reach_the_matrix_without_linear_zoom_artifacts() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    f.clip().duration = FrameRate::FPS_60.tick_of(5);
    for (choice, ease) in [DynamicZoomEase::Linear, DynamicZoomEase::EaseIn, DynamicZoomEase::EaseOut, DynamicZoomEase::EaseInOut].into_iter().enumerate() {
        set(f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap(), "easing", ParamValue::Choice(choice as u32));
        let q = f.project.sequence(f.sequence).unwrap();
        let item = &q.video_tracks[1].items[0];
        let m = dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(1));
        let expected = 2.0_f64.powf(ease.sample(0.25));
        assert!((m.a - expected).abs() < 1e-12);
    }
}

#[test]
fn off_center_portrait_and_par_keep_focus() {
    for (size, par, anchor) in [((2752, 1536), (1, 1), Vec2::new(1430.4, 499.5)), ((1080, 1920), (4, 3), Vec2::new(360.0, 1100.0))] {
        let mut f = VideoFxFixture::gradient_and_black_motion();
        zoom(&mut f);
        let q = f.project.sequence_mut(f.sequence).unwrap();
        q.settings.width = 1920;
        q.settings.height = 1080;
        q.settings.par = (4, 3);
        let item = &mut q.video_tracks[1].items[0];
        set(item.effect_mut("motion").unwrap(), "anchor", ParamValue::Vec2(anchor));
        set(item.effect_mut("motion").unwrap(), "position", ParamValue::Vec2(Vec2::new(740.0, 620.0)));
        set(item.effect_mut("motion").unwrap(), "rotation", ParamValue::Float(17.0));
        set(item.effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap(), "end_center", ParamValue::Vec2(Vec2::new(740.0 / 1920.0, 620.0 / 1080.0)));
        let q = f.project.sequence(f.sequence).unwrap();
        let item = &q.video_tracks[1].items[0];
        let basic = filmcraft_render::motion_matrix(q, item, size, Some(par), item.source_time_at(Tick::ZERO));
        let combined = dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(239)).then_apply(&basic);
        assert!((combined.apply(anchor) - Vec2::new(960.0, 540.0)).length() < 1e-6);
        finite(combined);
    }
}

#[test]
fn trim_retimes_and_invalid_rect_stays_finite() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    let item = f.clip();
    item.start = FrameRate::FPS_60.tick_of(60);
    item.duration = FrameRate::FPS_60.tick_of(121);
    item.speed = 0.5;
    item.reverse = true;
    let q = f.project.sequence(f.sequence).unwrap();
    let item = &q.video_tracks[1].items[0];
    assert_eq!(dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(59)), Affine::IDENTITY);
    let end = dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(180));
    assert_eq!(end.a, 2.0);
    assert_eq!(end, dynamic_zoom::matrix(q, item, FrameRate::FPS_60.tick_of(200)));
    set(f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap(), "end_width", ParamValue::Float(f64::NAN));
    let q = f.project.sequence(f.sequence).unwrap();
    finite(dynamic_zoom::matrix(q, &q.video_tracks[1].items[0], FrameRate::FPS_60.tick_of(100)));
}

#[test]
fn image_zoom_matches_plan_at_all_preview_scales_and_bypass_restores_motion() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    let original = f.frame(239, 1.0);
    zoom(&mut f);
    let end = f.frame(239, 1.0);
    assert!(max_abs_difference(&original, &end) > 0.1);
    for scale in [1.0, 0.5, 0.25] {
        assert!(max_abs_difference(&f.frame(239, scale), &f.planned(239, scale)) < 1e-6);
    }
    f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap().enabled = false;
    assert!(max_abs_difference(&original, &f.frame(239, 1.0)) < 1e-6);
    f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap().enabled = true;
    f.track().video_effects_bypassed = true;
    assert!(max_abs_difference(&original, &f.frame(239, 1.0)) < 1e-6);
    valid_alpha(&end);
}

#[test]
fn reverse_uses_end_rectangle_on_the_first_frame() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    let e = f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap();
    set(e, "reverse", ParamValue::Bool(true));
    let settings = DynamicZoomSettings::from_effect(e).unwrap();
    assert!(settings.reverse);
    let q = f.project.sequence(f.sequence).unwrap();
    let m = dynamic_zoom::matrix(q, &q.video_tracks[1].items[0], Tick::ZERO);
    assert_eq!(m.a, 2.0);
}

#[test]
fn opacity_mask_after_track_uses_the_combined_geometry_once() {
    let mut f = VideoFxFixture::gradient_and_black_motion();
    zoom(&mut f);
    f.project.sequence_mut(f.sequence).unwrap().video_tracks[0].enabled = false;
    set(f.clip().effect_mut("motion").unwrap(), "position", ParamValue::Vec2(Vec2::new(140.0, 80.0)));
    set(f.clip().effects.iter_mut().find(|e| e.effect == "dynamic_zoom").unwrap(), "end_center", ParamValue::Vec2(Vec2::new(172.0 / 320.0, 71.0 / 180.0)));
    let mut mask = filmcraft_project::Mask::new("Focus", filmcraft_project::MaskPath::ellipse(Vec2::new(192.0, 81.0), Vec2::new(24.0, 12.0)));
    mask.feather.value = ParamValue::Float(0.0);
    let opacity = f.clip().effect_mut("opacity").unwrap();
    opacity.masks.push(mask);
    set(opacity, "opacity", ParamValue::Float(65.0));
    set(opacity, "blend", ParamValue::Choice(0));
    f.track().effects.push(blur(8.0));
    for scale in [1.0, 0.5, 0.25] {
        let image = f.frame(239, scale);
        let (cx, cy) = ((160.0 * scale) as usize, (90.0 * scale) as usize);
        assert!((image.get(cx, cy)[3] - 0.65).abs() < 1e-6);
        assert!(image.get(cx + (55.0 * scale) as usize, cy)[3] < 1e-6);
        valid_alpha(&image);
    }
}
