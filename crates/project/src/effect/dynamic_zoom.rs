//! Two sequence-space viewports; geometry is evaluated after intrinsic Motion.
use super::{EffectDef, b, ch, f, pt, video};

pub(super) fn definition() -> EffectDef {
    let mut def = video(
        "dynamic_zoom",
        "Dynamic Zoom",
        &["Custom Effects", "Motion"],
        vec![
            pt("start_center", "Start Center", 0.5, 0.5),
            f("start_width", "Start Width", 1.0, 0.01, 4.0, "frame"),
            pt("end_center", "End Center", 0.5, 0.5),
            f("end_width", "End Width", 0.9, 0.01, 4.0, "frame"),
            b("reverse", "Reverse", false),
            ch("easing", "Easing", &["Linear", "Ease In", "Ease Out", "Ease In-Out"], 0),
        ],
    );
    for param in &mut def.params {
        param.animatable = false;
    }
    def.accelerated = false;
    def
}
