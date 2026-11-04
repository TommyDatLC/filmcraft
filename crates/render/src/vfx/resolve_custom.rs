//! Contributor-authored Fusion macro graphs, evaluated as native FilmCraft effects.
//! No Resolve runtime is required. Premultiplied alpha survives every branch and merge.
use super::{bv, chv, cv, fv, pv};
use crate::effects::{dec, enc};

fn exponential_zoom(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let start = param(e, "start", cx, 110.0, 1.0, 1000.0) / 100.0;
    let end = param(e, "end", cx, 100.0, 1.0, 1000.0) / 100.0;
    let duration = param(e, "duration", cx, 0.6, 0.0, 60.0);
    let duration = if duration > 0.0001 { duration } else { cx.env.map(|env| finite(env.clip_seconds() as f32, 1.0, 0.0001, 1_000_000.0)).unwrap_or(1.0) };
    let steepness = param(e, "steepness", cx, 10.0, 0.1, 50.0);
    let time = if cx.seconds.is_finite() { cx.seconds as f32 } else { 0.0 };
    // This control is authored in sequence pixels, as resolved by effects.apply.
    // The effect runs before intrinsic Motion, so map the screen pivot back into
    // the working source layer. Source-sized coordinates drift on scaled PNGs
    // and on cropped/positioned footage, especially at reduced preview resolution.
    let center = if let Some(env) = cx.env {
        let (sw, sh) = env.sequence_size();
        let v = e.param("pivot").map(|p| p.vec2_at(cx.t));
        let x = v.map(|v| v.x).filter(|x| x.is_finite()).unwrap_or(sw as f64 / 2.0);
        let y = v.map(|v| v.y).filter(|y| y.is_finite()).unwrap_or(sh as f64 / 2.0);
        let k = finite(env.output_scale(), 1.0, 0.001, 16.0) as f64;
        env.layer_to_output()
            .inverse()
            .map(|m| m.apply(filmcraft_geom::Vec2::new(x * k, y * k)))
            .unwrap_or(filmcraft_geom::Vec2::new(image.w as f64 / 2.0, image.h as f64 / 2.0))
    } else {
        pv(e, "pivot", cx, image)
    };
    let (w, h) = (image.w as f32, image.h as f32);
    let pivot = (finite(center.x as f32, w / 2.0, -w * 4.0, w * 4.0), finite(center.y as f32, h / 2.0, -h * 4.0, h * 4.0));
    let shutter = param(e, "shutter", cx, 330.0, 0.0, 360.0) / 360.0;
    let samples = if bv(e, "motion_blur") && shutter > 0.0 { param(e, "quality", cx, 9.0, 2.0, 10.0).round() as usize } else { 1 };
    let fps = cx.env.map(|env| finite(env.frame_rate() as f32, 60.0, 1.0, 1000.0)).unwrap_or(60.0);
    let zooms: Vec<f32> = (0..samples)
        .map(|i| {
            let delta = if samples > 1 { (i as f32 / (samples - 1) as f32 - 0.5) * shutter / fps } else { 0.0 };
            let progress = ((time + delta) / duration).clamp(0.0, 1.0);
            let curve = (1.0 - 2.0f32.powf(-steepness * progress)) / (1.0 - 2.0f32.powf(-steepness));
            start + (end - start) * curve
        })
        .collect();
    if zooms.iter().all(|z| (*z - 1.0).abs() < 1e-7) {
        return;
    }
    let source = image.clone();
    let edges = chv(e, "edges");
    image.px.par_chunks_exact_mut(image.w * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let mut total = [0.0; 4];
            for zoom in &zooms {
                let mut sx = (x as f32 + 0.5 - pivot.0) / zoom + pivot.0;
                let mut sy = (y as f32 + 0.5 - pivot.1) / zoom + pivot.1;
                if edges == 2 {
                    sx = w - (sx.rem_euclid(2.0 * w) - w).abs();
                    sy = h - (sy.rem_euclid(2.0 * h) - h).abs();
                }
                if edges == 3 {
                    sx = sx.rem_euclid(w);
                    sy = sy.rem_euclid(h);
                }
                let sample = if edges == 1 { source.sample_bilinear_clamped(sx, sy) } else { source.sample_bilinear(sx, sy) };
                for (a, b) in total.iter_mut().zip(sample) {
                    *a += b / samples as f32;
                }
            }
            p.copy_from_slice(&total);
        }
    });
}

fn delta_key(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let alpha_path = super::tv(e, "alpha_source");
    if !alpha_path.is_empty() {
        if let (Some(project), Some(env)) = (cx.project, cx.env) {
            let item = project.items.values().find(|p| match &p.kind {
                filmcraft_project::ItemKind::Media(media) => match &media.media {
                    filmcraft_project::MediaRef::File { path, .. } => path.replace('\\', "/").eq_ignore_ascii_case(&alpha_path.replace('\\', "/")),
                    _ => false,
                },
                _ => false,
            });
            if let Some(item) = item
                && let Some(matte) = env.auxiliary_frame(item.id)
            {
                let recover = bv(e, "unmatte_black");
                let (w, h) = (image.w, image.h);
                image.px.par_chunks_exact_mut(w * 4).enumerate().for_each(|(y, row)| {
                    for (x, p) in row.chunks_exact_mut(4).enumerate() {
                        let m = matte.sample_bilinear((x as f32 + 0.5) * matte.w as f32 / w as f32, (y as f32 + 0.5) * matte.h as f32 / h as f32);
                        let alpha = filmcraft_color::linear_to_srgb((0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2]).max(0.0)).clamp(0.0, 1.0);
                        if alpha < 1.0 / 510.0 || p[3] <= 0.0 {
                            p.fill(0.0);
                            continue;
                        }
                        let straight = enc([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
                        let recovered = dec(straight.map(|v| if recover { (v / alpha).clamp(0.0, 1.0) } else { v }));
                        let a = alpha * p[3];
                        p.copy_from_slice(&[recovered[0] * a, recovered[1] * a, recovered[2] * a, a]);
                    }
                });
            }
        }
        return;
    }
    let key = cv(e, "key_color", cx);
    let dom = if key[1] >= key[0] && key[1] >= key[2] {
        1
    } else if key[2] >= key[0] {
        2
    } else {
        0
    };
    let other = [(dom + 1) % 3, (dom + 2) % 3];
    let key_difference = (key[dom] - key[other[0]].max(key[other[1]])).max(0.05);
    let clean_bg = param(e, "clean_bg", cx, 0.03, 0.0, 0.49);
    let clean_fg = param(e, "clean_fg", cx, 0.05, 0.0, 0.49);
    let spill = param(e, "spill", cx, 1.0, 0.0, 1.0);
    image.px.par_chunks_exact_mut(4).for_each(|p| {
        if p[3] <= 0.0 {
            p.fill(0.0);
            return;
        }
        let mut color = enc([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        let difference = (color[dom] - color[other[0]].max(color[other[1]])).max(0.0);
        let raw_alpha = 1.0 - (difference / key_difference).clamp(0.0, 1.0);
        let alpha = ((raw_alpha - clean_bg) / (1.0 - clean_bg - clean_fg)).clamp(0.0, 1.0) * p[3];
        color[dom] = (color[dom] - difference * spill).max(0.0);
        let linear = dec(color);
        p.copy_from_slice(&[linear[0] * alpha, linear[1] * alpha, linear[2] * alpha, alpha]);
    });
}
use crate::{
    Blend, Image,
    blend::composite,
    effects::{FxCtx, gaussian},
};
use filmcraft_project::EffectInstance;
use rayon::prelude::*;

fn finite(v: f32, fallback: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { fallback }
}
fn param(e: &EffectInstance, id: &str, cx: &FxCtx, fallback: f32, lo: f32, hi: f32) -> f32 {
    finite(fv(e, id, cx), fallback, lo, hi)
}
fn scale(cx: &FxCtx) -> f32 {
    finite(cx.px_scale, 1.0, 0.001, 16.0)
}
fn blur(source: &Image, radius: f32) -> Image {
    let mut image = source.clone();
    gaussian(&mut image, radius, radius, false);
    image
}
fn threshold(source: &Image, value: f32) -> Image {
    let mut image = source.clone();
    image.px.par_chunks_exact_mut(4).for_each(|p| {
        let a = p[3];
        let l = if a > 1e-8 { (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / a } else { 0.0 };
        if a <= 1e-8 || l < value {
            p.fill(0.0);
        }
    });
    image
}
fn three_layer(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let source = image.clone();
    for (th, rd, td, dd) in [("layer2_threshold", "layer2_blur", 0.3, 0.5), ("layer3_threshold", "layer3_blur", 0.85, 30.0)] {
        let mask = threshold(&source, param(e, th, cx, td, 0.0, 1.0));
        let layer = blur(&mask, param(e, rd, cx, dd, 0.0, 100.0) * scale(cx));
        composite(image, &layer, 1.0, Blend::Lighten);
    }
}
fn exponential(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let mix = param(e, "blend", cx, 0.89, 0.0, 1.0);
    if mix <= 0.0 {
        return;
    }
    let threshold = param(e, "threshold", cx, 0.808, 0.0, 1.0);
    let knee = param(e, "knee", cx, 0.128, 0.0, 1.0);
    let gain = param(e, "gain", cx, 0.674, 0.0, 10.0);
    if gain <= 0.0 {
        return;
    }
    let radius = param(e, "radius", cx, 5.4, 0.0, 100.0) * scale(cx);
    let falloff = param(e, "falloff", cx, 0.8822, 0.0, 1.0);
    let raw = cv(e, "color", cx);
    let color = [finite(raw[0], 1.0, 0.0, 1.0), finite(raw[1], 1.0, 0.0, 1.0), finite(raw[2], 1.0, 0.0, 1.0)];
    let colorize = ((1.0 - color[0].min(color[1]).min(color[2])) * 2.0).clamp(0.0, 1.0) * param(e, "colorize", cx, 1.0, 0.0, 1.0);
    let mut highlights = image.clone();
    highlights.px.par_chunks_exact_mut(4).for_each(|p| {
        let a = p[3];
        if a <= 1e-8 {
            p.fill(0.0);
            return;
        }
        let l = (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / a;
        let factor = if l >= threshold { if knee > 0.0001 { ((l - threshold) / knee).min(1.0) * gain } else { gain } } else { 0.0 };
        let alpha = (a * factor).clamp(0.0, 1.0);
        for (c, v) in color.iter().enumerate() {
            p[c] = ((1.0 - colorize) * p[c] + colorize * l * a) * v * factor;
        }
        p[3] = alpha;
    });
    let mut accumulated = blur(&highlights, radius);
    for index in 1..6 {
        let layer = blur(&highlights, radius * (1u32 << index) as f32);
        composite(&mut accumulated, &layer, falloff.powi(index), Blend::Screen);
    }
    let mode = match chv(e, "mode") {
        1 => Blend::Screen,
        2 => Blend::LinearDodge,
        3 => Blend::Normal,
        _ => Blend::Lighten,
    };
    composite(image, &accumulated, mix, mode);
}
fn vignette(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let mix = param(e, "blend", cx, 1.0, 0.0, 1.0);
    let radius = param(e, "blur", cx, 15.0, 0.0, 100.0) * scale(cx);
    if mix <= 0.0 || radius <= 0.0 {
        return;
    }
    let out = blur(image, radius);
    let center = pv(e, "center", cx, image);
    let (w, h) = (image.w as f32, image.h as f32);
    let center_x = finite(center.x as f32, w / 2.0, -w * 4.0, w * 4.0);
    let center_y = finite(center.y as f32, h / 2.0, -h * 4.0, h * 4.0);
    let width = param(e, "width", cx, 0.35, 0.0, 1.0) * w / 2.0;
    let height = param(e, "height", cx, 0.35, 0.0, 1.0) * h / 2.0;
    let softness = param(e, "softness", cx, 0.05, 0.0, 0.25) * w.min(h);
    let roundness = param(e, "roundness", cx, 0.2, 0.0, 1.0) * width.min(height);
    let angle = param(e, "angle", cx, 0.0, -36000.0, 36000.0).to_radians();
    let (sn, cs) = angle.sin_cos();
    let outside = bv(e, "outside");
    let rectangle = chv(e, "shape") == 1;
    image.px.par_chunks_exact_mut(image.w * 4).zip(out.px.par_chunks_exact(out.w * 4)).enumerate().for_each(|(y, (row, other))| {
        for (x, (p, q)) in row.chunks_exact_mut(4).zip(other.chunks_exact(4)).enumerate() {
            let (dx, dy) = (x as f32 + 0.5 - center_x, y as f32 + 0.5 - center_y);
            let (xx, yy) = (dx * cs + dy * sn, -dx * sn + dy * cs);
            let distance = if width <= 1e-6 || height <= 1e-6 {
                w.max(h)
            } else if rectangle {
                let a = xx.abs() - width + roundness;
                let b = yy.abs() - height + roundness;
                a.max(0.0).hypot(b.max(0.0)) + a.max(b).min(0.0) - roundness
            } else {
                (xx / width).hypot(yy / height).mul_add(width.min(height), -width.min(height))
            };
            let mask = if softness > 1e-6 {
                (0.5 - distance / softness).clamp(0.0, 1.0)
            } else if distance <= 0.0 {
                1.0
            } else {
                0.0
            };
            let mask = mask * mask * (3.0 - 2.0 * mask);
            let amount = (if outside { 1.0 - mask } else { mask }) * mix;
            for (v, b) in p.iter_mut().zip(q) {
                *v += (*b - *v) * amount;
            }
        }
    });
}
fn screen(image: &mut Image, e: &EffectInstance, cx: &FxCtx) {
    let texture = bv(e, "texture_on");
    let camera = bv(e, "auto_move");
    if !texture && !camera {
        return;
    }
    let (w, h) = (image.w as f32, image.h as f32);
    if texture {
        let strength = param(e, "scanline_strength", cx, 0.15, 0.0, 1.0);
        let lines = param(e, "scanline_count", cx, 10000.0, 10.0, 10000.0);
        let softness = param(e, "softness", cx, 0.5, 0.02, 1.0);
        let grid = param(e, "grid_strength", cx, 0.05, 0.0, 1.0);
        let columns = param(e, "grid_columns", cx, 480.0, 10.0, 1920.0);
        image.px.par_chunks_exact_mut(image.w * 4).enumerate().for_each(|(y, row)| {
            let u = 1.0 - (y as f32 + 0.5) / h;
            let scan = (0.5 + 0.5 * (u * lines * std::f32::consts::TAU).sin() / softness).clamp(0.0, 1.0);
            for (x, p) in row.chunks_exact_mut(4).enumerate() {
                let u = (x as f32 + 0.5) / w;
                let pixel = (0.5 + 0.5 * (u * columns * std::f32::consts::TAU).sin() / softness).clamp(0.0, 1.0);
                let factor = (1.0 - strength * scan) * (1.0 - grid * pixel);
                for v in &mut p[..3] {
                    *v *= factor;
                }
            }
        });
    }
    if !camera {
        return;
    }
    let seconds = if cx.seconds.is_finite() { cx.seconds.rem_euclid(1_000_000.0) as f32 } else { 0.0 };
    let time = seconds * param(e, "speed", cx, 1.0, 0.0, 3.0);
    let seed = param(e, "seed", cx, 1.0, 0.0, 100.0);
    let yaw = param(e, "yaw", cx, 1.5, 0.0, 5.0)
        * (0.58 * (time * 0.43 + seed * 1.73).sin() + 0.28 * (time * 0.79 + seed * 2.41 + 0.9).sin() + 0.14 * (time * 1.31 + seed * 0.91 + 2.1).sin());
    let pitch = param(e, "pitch", cx, 1.0, 0.0, 5.0)
        * (0.58 * (time * 0.37 + seed * 2.19 + 1.4).sin() + 0.28 * (time * 0.67 + seed * 1.31 + 2.8).sin() + 0.14 * (time * 1.13 + seed * 2.83 + 0.6).sin());
    let zoom = param(e, "zoom", cx, 1.06, 1.0, 1.3);
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let source = image.clone();
    // Inverse plane homography for small-angle DVE pitch/yaw; transparent outside the plane.
    image.px.par_chunks_exact_mut(image.w * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let u = (x as f32 + 0.5 - w / 2.0) / h / zoom;
            let v = (y as f32 + 0.5 - h / 2.0) / h / zoom;
            let a = cy - u * sy / 2.0;
            let b = sy * sp + u * cy * sp / 2.0;
            let c = -v * sy / 2.0;
            let d = cp + v * cy * sp / 2.0;
            let determinant = a * d - b * c;
            if determinant.abs() < 1e-6 {
                p.fill(0.0);
                continue;
            }
            let sx = (u * d - b * v) / determinant * h + w / 2.0;
            let sy = (a * v - c * u) / determinant * h + h / 2.0;
            p.copy_from_slice(&source.sample_bilinear(sx, sy));
        }
    });
}

pub(super) fn apply(image: &mut Image, e: &EffectInstance, cx: &FxCtx) -> bool {
    if !matches!(e.effect.as_str(), "three_layer_bloom" | "exponential_bloom" | "vignette_blur" | "screen_effect" | "exponential_zoom" | "delta_key") {
        return false;
    }
    let valid = image.w.checked_mul(image.h).and_then(|n| n.checked_mul(4)).is_some_and(|n| n == image.px.len() && n <= 64_000_000);
    if !valid || image.w == 0 || image.h == 0 {
        return true;
    }
    match e.effect.as_str() {
        "exponential_zoom" => exponential_zoom(image, e, cx),
        "delta_key" => delta_key(image, e, cx),
        "three_layer_bloom" => three_layer(image, e, cx),
        "exponential_bloom" => exponential(image, e, cx),
        "vignette_blur" => vignette(image, e, cx),
        "screen_effect" => screen(image, e, cx),
        _ => {}
    }
    true
}
