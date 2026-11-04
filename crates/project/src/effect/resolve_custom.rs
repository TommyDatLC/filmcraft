//! Native controls for the contributor's local Fusion macro graphs.
//! The macro files remain external; these are original Rust implementations.
use super::{EffectDef, ang, b, ch, col, f, pt, txt, video};

const FOLDER: &[&str] = &["Custom Effects", "Evernerd"];

pub(super) fn defs() -> Vec<EffectDef> {
    let mut effects = vec![
        video(
            "exponential_zoom",
            "Exponential Zoom (Ease Expo Out)",
            FOLDER,
            vec![
                f("start", "Start Zoom", 110.0, 1.0, 1000.0, "%"),
                f("end", "End Zoom", 100.0, 1.0, 1000.0, "%"),
                f("duration", "Zoom Duration", 0.6, 0.0, 60.0, "s"),
                f("steepness", "Expo Steepness", 10.0, 0.1, 50.0, ""),
                pt("pivot", "Zoom Pivot (Sequence)", f64::NAN, f64::NAN),
                ch("edges", "Edges Mode", &["Transparent", "Duplicate", "Mirror", "Wrap"], 0),
                b("motion_blur", "Motion Blur", true),
                f("quality", "Motion Blur Quality", 9.0, 2.0, 10.0, ""),
                f("shutter", "Shutter Angle", 330.0, 0.0, 360.0, "deg"),
            ],
        ),
        video(
            "delta_key",
            "Delta Key",
            FOLDER,
            vec![
                col("key_color", "Key Color", [0.0, 1.0, 0.0, 1.0]),
                f("clean_bg", "Clean Background", 0.03, 0.0, 0.5, ""),
                f("clean_fg", "Clean Foreground", 0.05, 0.0, 0.5, ""),
                f("spill", "Spill Suppression", 1.0, 0.0, 1.0, ""),
                txt("alpha_source", "Alpha Source (optional)"),
                b("unmatte_black", "Recover Color From Black Matte", true),
            ],
        ),
        video(
            "three_layer_bloom",
            "Three Layer Bloom",
            FOLDER,
            vec![
                f("layer2_threshold", "Layer 2 - Threshold", 0.3, 0.0, 1.0, ""),
                f("layer2_blur", "Layer 2 - Gaussian Blur", 0.5, 0.0, 100.0, "px"),
                f("layer3_threshold", "Layer 3 - Threshold", 0.85, 0.0, 1.0, ""),
                f("layer3_blur", "Layer 3 - Gaussian Blur", 30.0, 0.0, 100.0, "px"),
            ],
        ),
        video(
            "exponential_bloom",
            "Exponential Bloom (ExponentBloom)",
            FOLDER,
            vec![
                f("threshold", "Threshold", 0.808, 0.0, 1.0, ""),
                f("knee", "Soft Knee", 0.128, 0.0, 1.0, ""),
                f("radius", "Base Radius (Spread)", 5.4, 0.0, 100.0, "px"),
                f("falloff", "Exponent Falloff", 0.8822, 0.0, 1.0, ""),
                f("gain", "Bloom Gain", 0.674, 0.0, 10.0, ""),
                col("color", "Bloom Color", [1.0; 4]),
                f("colorize", "Colorize Strength", 1.0, 0.0, 1.0, ""),
                ch("mode", "Apply Mode", &["Lighten", "Screen", "Add", "Normal"], 0),
                f("blend", "Effect Blend", 0.89, 0.0, 1.0, ""),
            ],
        ),
        video(
            "vignette_blur",
            "Vignette Blur (Masked Gaussian Blur)",
            FOLDER,
            vec![
                ch("shape", "Mask Shape", &["Oval", "Rounded Rectangle"], 0),
                f("roundness", "Rectangle Corner Roundness", 0.2, 0.0, 1.0, ""),
                f("blur", "Blur Strength", 15.0, 0.0, 100.0, "px"),
                pt("center", "Mask Center", f64::NAN, f64::NAN),
                f("width", "Mask Width", 0.35, 0.0, 1.0, ""),
                f("height", "Mask Height", 0.35, 0.0, 1.0, ""),
                ang("angle", "Mask Rotation", 0.0),
                f("softness", "Mask Softness", 0.05, 0.0, 0.25, ""),
                b("outside", "Blur Outside Mask", true),
                f("blend", "Effect Mix", 1.0, 0.0, 1.0, ""),
            ],
        ),
        video(
            "screen_effect",
            "Screen Effect (Custom Fusion)",
            FOLDER,
            vec![
                b("texture_on", "Screen Texture", true),
                f("scanline_strength", "Scanline Strength", 0.15, 0.0, 1.0, ""),
                f("scanline_count", "Scanline Count", 10000.0, 10.0, 10000.0, ""),
                f("softness", "Texture Softness", 0.5, 0.02, 1.0, ""),
                f("grid_strength", "Pixel Grid Strength", 0.05, 0.0, 1.0, ""),
                f("grid_columns", "Pixel Grid Columns", 480.0, 10.0, 1920.0, ""),
                b("auto_move", "Automatic Camera Movement", true),
                f("speed", "Movement Speed", 1.0, 0.0, 3.0, ""),
                f("yaw", "Yaw Amount", 1.5, 0.0, 5.0, "deg"),
                f("pitch", "Pitch Amount", 1.0, 0.0, 5.0, "deg"),
                f("zoom", "Zoom / Edge Coverage", 1.06, 1.0, 1.3, ""),
                f("seed", "Motion Variation", 1.0, 0.0, 100.0, ""),
            ],
        ),
    ];
    // These multi-pass reference effects run on the CPU, including the desktop/export paths.
    for effect in &mut effects {
        effect.accelerated = false;
    }
    effects
}
