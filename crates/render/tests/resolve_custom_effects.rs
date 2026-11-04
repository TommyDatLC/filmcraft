use filmcraft_project::{EffectInstance, ParamValue, find_effect};
use filmcraft_render::{
    Image,
    effects::{FxCtx, apply},
};
use filmcraft_time::Tick;

fn context(seconds: f64) -> FxCtx<'static> {
    FxCtx {
        t: Tick::from_seconds_f64(seconds),
        px_scale: 1.0,
        seconds,
        timecode: "",
        clip_name: "test",
        project: None,
        env: None,
        frame_rate: 24.0,
        working: filmcraft_color::WorkingSpace::Rec709,
    }
}
fn effect(id: &str) -> EffectInstance {
    find_effect(id).unwrap_or_else(|| panic!("missing custom effect {id}")).instance()
}
fn set(e: &mut EffectInstance, id: &str, v: ParamValue) {
    e.params.get_mut(id).unwrap().value = v;
}
fn check(img: &Image) {
    assert!(img.px.iter().all(|v| v.is_finite()));
    for p in img.px.chunks_exact(4) {
        assert!((0.0..=1.0001).contains(&p[3]));
    }
}

#[test]
fn resolve_custom_catalog_has_user_controls() {
    for (id, param, default) in [
        ("three_layer_bloom", "layer2_threshold", 0.3),
        ("exponential_bloom", "falloff", 0.8822),
        ("vignette_blur", "blur", 15.0),
        ("screen_effect", "scanline_count", 10000.0),
    ] {
        let e = effect(id);
        assert_eq!(e.f64_at(param, Tick::ZERO), default);
    }
}

#[test]
fn resolve_exponential_zoom_reaches_exact_end_and_preserves_alpha() {
    let mut source = Image::new(32, 24);
    for y in 6..18 {
        for x in 8..24 {
            let i = (y * 32 + x) * 4;
            source.px[i..i + 4].copy_from_slice(&[0.7, 0.3, 0.2, 1.0]);
        }
    }
    let mut e = effect("exponential_zoom");
    set(&mut e, "motion_blur", ParamValue::Bool(false));
    let mut start = source.clone();
    let mut end = source.clone();
    apply(&mut start, &e, &context(0.0));
    apply(&mut end, &e, &context(0.6));
    assert_ne!(start, source);
    assert_eq!(end, source);
    check(&start);
    check(&end);
}

struct ZoomPlacement {
    transform: filmcraft_geom::Affine,
    output_scale: f32,
}
impl filmcraft_render::vfx::FxEnv for ZoomPlacement {
    fn frame(&self, _: f64, _: &EffectInstance) -> Option<Image> {
        None
    }
    fn source_frame(&self, _: f64, _: f32) -> Option<Image> {
        None
    }
    fn track(&self, _: usize) -> Option<Image> {
        None
    }
    fn layer_to_output(&self) -> filmcraft_geom::Affine {
        self.transform
    }
    fn output_scale(&self) -> f32 {
        self.output_scale
    }
    fn clip_seconds(&self) -> f64 {
        5.0
    }
    fn clip_offset(&self) -> f64 {
        0.0
    }
    fn frame_rate(&self) -> f64 {
        60.0
    }
    fn media_timecode(&self) -> String {
        String::new()
    }
    fn file_name(&self) -> String {
        String::new()
    }
    fn sequence_name(&self) -> String {
        String::new()
    }
    fn sequence_size(&self) -> (u32, u32) {
        (24, 18)
    }
    fn source_size(&self) -> (u32, u32) {
        (32, 24)
    }
    fn clip_key(&self) -> u64 {
        0
    }
}

#[test]
fn exponential_zoom_pivot_stays_at_sequence_center_after_motion() {
    use filmcraft_geom::{Affine, Vec2};
    let mut source = Image::new(32, 24);
    for y in 0..24 {
        for x in 0..32 {
            let i = (y * 32 + x) * 4;
            source.px[i..i + 4].copy_from_slice(&[x as f32 / 32.0, y as f32 / 24.0, 0.3, 1.0]);
        }
    }
    let mut e = effect("exponential_zoom");
    set(&mut e, "motion_blur", ParamValue::Bool(false));
    set(&mut e, "start", ParamValue::Float(150.0));
    set(&mut e, "pivot", ParamValue::Vec2(Vec2::new(12.0, 9.0)));
    let env = ZoomPlacement { transform: Affine::translate(-4.0, -3.0), output_scale: 1.0 };
    let mut actual = source.clone();
    apply(&mut actual, &e, &FxCtx { env: Some(&env), ..context(0.0) });
    // Sequence centre maps to the source centre through the clip's Motion placement.
    set(&mut e, "pivot", ParamValue::Vec2(Vec2::new(16.0, 12.0)));
    let mut expected = source;
    apply(&mut expected, &e, &context(0.0));
    assert!(actual == expected, "zoom must not drift when source size differs from the sequence");
}

#[test]
fn exponential_zoom_sequence_pivot_accounts_for_preview_scale_and_custom_focus() {
    use filmcraft_geom::{Affine, Vec2};
    let mut e = effect("exponential_zoom");
    set(&mut e, "motion_blur", ParamValue::Bool(false));
    set(&mut e, "start", ParamValue::Float(150.0));
    // Half-size output with a quarter-size decoded source and a translated crop.
    let env = ZoomPlacement { transform: Affine::translate(-2.0, -1.5).then_apply(&Affine::scale(2.0, 2.0)), output_scale: 0.5 };
    for focus in [Vec2::new(12.0, 9.0), Vec2::new(8.0, 6.0)] {
        let mut source = Image::new(8, 6);
        for y in 0..6 {
            for x in 0..8 {
                let i = (y * 8 + x) * 4;
                source.px[i..i + 4].copy_from_slice(&[x as f32 / 8.0, y as f32 / 6.0, 0.3, 1.0]);
            }
        }
        set(&mut e, "pivot", ParamValue::Vec2(focus));
        let mut actual = source.clone();
        apply(&mut actual, &e, &FxCtx { env: Some(&env), px_scale: 0.25, ..context(0.0) });
        let layer_focus = env.transform.inverse().unwrap().apply(Vec2::new(focus.x * 0.5, focus.y * 0.5));
        set(&mut e, "pivot", ParamValue::Vec2(layer_focus));
        apply(&mut source, &e, &context(0.0));
        assert!(actual == source, "screen focus must stay fixed at any preview resolution");
    }
}

#[test]
fn delta_key_removes_green_but_preserves_red_and_transparency() {
    let e = effect("delta_key");
    let mut image = Image::new(3, 1);
    image.px.copy_from_slice(&[0.0, 1.0, 0.0, 1.0, 0.8, 0.05, 0.1, 1.0, 0.0, 0.0, 0.0, 0.0]);
    apply(&mut image, &e, &context(0.0));
    check(&image);
    assert_eq!(image.get(0, 0)[3], 0.0);
    assert!(image.get(1, 0)[3] > 0.99);
    assert_eq!(image.get(2, 0), [0.0; 4]);
}

#[test]
fn resolve_bloom_spreads_light_without_opaque_background() {
    for id in ["three_layer_bloom", "exponential_bloom"] {
        let mut image = Image::new(64, 32);
        for y in 12..20 {
            for x in 28..36 {
                let i = (y * 64 + x) * 4;
                image.px[i..i + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
            }
        }
        apply(&mut image, &effect(id), &context(0.0));
        check(&image);
        assert!(image.get(25, 16)[3] > 0.0, "{id} must spread glow alpha");
        assert!(image.get(0, 0)[3] < 0.5, "{id} must not add an opaque plane");
        assert!(image.get(25, 16)[0] > 0.0);
    }
}

#[test]
fn resolve_exponent_bloom_zero_blend_is_identity() {
    let mut image = Image::filled(16, 12, [0.5, 0.2, 0.1, 0.7]);
    let before = image.clone();
    let mut e = effect("exponential_bloom");
    set(&mut e, "blend", ParamValue::Float(0.0));
    apply(&mut image, &e, &context(0.0));
    assert_eq!(image, before);
}

#[test]
fn resolve_screen_texture_preserves_alpha_and_can_be_disabled() {
    let mut e = effect("screen_effect");
    set(&mut e, "auto_move", ParamValue::Bool(false));
    let source = Image::filled(16, 12, [0.4, 0.2, 0.1, 0.6]);
    let mut image = source.clone();
    apply(&mut image, &e, &context(0.0));
    check(&image);
    assert_ne!(image, source);
    for (a, b) in image.px.chunks_exact(4).zip(source.px.chunks_exact(4)) {
        assert_eq!(a[3], b[3]);
        assert!(a[0] <= b[0] + 1e-6);
    }
    set(&mut e, "texture_on", ParamValue::Bool(false));
    let mut image = source.clone();
    apply(&mut image, &e, &context(0.0));
    assert_eq!(image, source);
}

#[test]
fn resolve_screen_camera_is_deterministic_and_animated() {
    let mut source = Image::new(32, 24);
    for y in 6..18 {
        for x in 8..24 {
            let i = (y * 32 + x) * 4;
            source.px[i..i + 4].copy_from_slice(&[0.7, 0.3, 0.2, 1.0]);
        }
    }
    let e = effect("screen_effect");
    let mut a = source.clone();
    let mut b = source.clone();
    let mut later = source;
    apply(&mut a, &e, &context(0.0));
    apply(&mut b, &e, &context(0.0));
    apply(&mut later, &e, &context(5.0));
    assert_eq!(a, b);
    assert_ne!(a, later);
    check(&a);
    check(&later);
}

#[test]
fn resolve_vignette_blur_keeps_center_sharp() {
    let mut source = Image::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let i = (y * 64 + x) * 4;
            let v = ((x + y) % 2) as f32;
            source.px[i..i + 4].copy_from_slice(&[v, v, v, 1.0]);
        }
    }
    let mut e = effect("vignette_blur");
    set(&mut e, "outside", ParamValue::Bool(true));
    let mut image = source.clone();
    apply(&mut image, &e, &context(0.0));
    assert_eq!(image.get(32, 32), source.get(32, 32));
    assert_ne!(image.get(3, 3), source.get(3, 3));
    check(&image);
}

#[test]
fn resolve_custom_effects_keep_transparent_frames_clear_and_handle_hostile_values() {
    for id in ["three_layer_bloom", "exponential_bloom", "vignette_blur", "screen_effect", "exponential_zoom", "delta_key"] {
        let mut e = effect(id);
        let mut clear = Image::new(16, 12);
        apply(&mut clear, &e, &context(0.0));
        assert!(clear.px.iter().all(|v| v.abs() < 1e-6));
        for p in e.params.values_mut() {
            if matches!(p.value, ParamValue::Float(_)) {
                p.value = ParamValue::Float(f64::NAN);
            }
        }
        let mut image = Image::filled(16, 12, [0.5, 0.2, 0.1, 0.7]);
        apply(&mut image, &e, &context(f64::NAN));
        check(&image);
    }
}
