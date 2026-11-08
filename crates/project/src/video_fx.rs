//! Capabilities shared by the track inspector, command validation and renderer.
use crate::{EffectDef, EffectKind};
use filmcraft_geom::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicZoomRect {
    pub center: Vec2,
    pub width: f64,
}
impl DynamicZoomRect {
    pub const FULL_FRAME: Self = Self { center: Vec2 { x: 0.5, y: 0.5 }, width: 1.0 };
    pub fn is_valid(self) -> bool {
        self.width.is_finite()
            && (0.01..=4.0).contains(&self.width)
            && self.center.x.is_finite()
            && self.center.y.is_finite()
            && (-2.0..=3.0).contains(&self.center.x)
            && (-2.0..=3.0).contains(&self.center.y)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DynamicZoomEase {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
impl DynamicZoomEase {
    pub fn sample(self, progress: f64) -> f64 {
        let p = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Self::Linear => p,
            Self::EaseIn => p * p,
            Self::EaseOut => 1.0 - (1.0 - p) * (1.0 - p),
            Self::EaseInOut => p * p * (3.0 - 2.0 * p),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicZoomSettings {
    pub start: DynamicZoomRect,
    pub end: DynamicZoomRect,
    pub reverse: bool,
    pub easing: DynamicZoomEase,
}
impl DynamicZoomSettings {
    pub fn from_effect(effect: &crate::EffectInstance) -> Result<Self, String> {
        use crate::ParamValue;
        if effect.effect != "dynamic_zoom" {
            return Err("expected Dynamic Zoom".into());
        }
        if !effect.masks.is_empty() {
            return Err("Dynamic Zoom viewports do not support masks".into());
        }

        let value = |id: &str| -> Result<ParamValue, String> {
            if let Some(param) = effect.param(id) {
                if !param.keyframes.is_empty() {
                    return Err(format!("Dynamic Zoom `{id}` cannot be animated"));
                }
                Ok(param.value.clone())
            } else {
                effect.def().and_then(|d| d.param(id)).map(|p| p.default.clone()).ok_or_else(|| format!("missing Dynamic Zoom `{id}`"))
            }
        };
        let point = |id: &str| match value(id)? {
            ParamValue::Vec2(v) => Ok(v),
            _ => Err(format!("Dynamic Zoom `{id}` must be a point")),
        };
        let width = |id: &str| match value(id)? {
            ParamValue::Float(v) => Ok(v),
            _ => Err(format!("Dynamic Zoom `{id}` must be a width")),
        };
        let start = DynamicZoomRect { center: point("start_center")?, width: width("start_width")? };
        let end = DynamicZoomRect { center: point("end_center")?, width: width("end_width")? };
        if !start.is_valid() || !end.is_valid() {
            return Err("Dynamic Zoom needs finite centers in -2..3 and widths in 0.01..4".into());
        }
        let reverse = match value("reverse")? {
            ParamValue::Bool(v) => v,
            _ => return Err("Dynamic Zoom reverse must be boolean".into()),
        };
        let easing = match value("easing")? {
            ParamValue::Choice(0) => DynamicZoomEase::Linear,
            ParamValue::Choice(1) => DynamicZoomEase::EaseIn,
            ParamValue::Choice(2) => DynamicZoomEase::EaseOut,
            ParamValue::Choice(3) => DynamicZoomEase::EaseInOut,
            _ => return Err("Dynamic Zoom easing must be one of its four options".into()),
        };
        Ok(Self { start, end, reverse, easing })
    }
    /// Interpolate only validated viewports; malformed caller data falls back to a full frame.
    pub fn rect_at(self, progress: f64) -> DynamicZoomRect {
        if !self.start.is_valid() || !self.end.is_valid() {
            return DynamicZoomRect::FULL_FRAME;
        }
        let (start, end) = if self.reverse { (self.end, self.start) } else { (self.start, self.end) };
        let u = self.easing.sample(progress);
        if u <= 0.0 {
            return start;
        }
        if u >= 1.0 {
            return end;
        }
        DynamicZoomRect { center: start.center + (end.center - start.center) * u, width: (start.width.ln() + (end.width.ln() - start.width.ln()) * u).exp() }
    }
}

/// Track inserts receive one isolated, sequence-sized RGBA layer. They cannot read
/// source history, other tracks or content-layer metadata.
pub fn supports_track(def: &EffectDef) -> bool {
    def.kind == EffectKind::Video
        && !def.intrinsic
        && !crate::graphic::is_layer_id(def.id)
        && !matches!(
            def.id,
            "dynamic_zoom"
                | "echo"
                | "posterize_time"
                | "warp_stabilizer"
                | "auto_reframe"
                | "track_matte"
                | "compound_blur"
                | "gradient_wipe"
                | "gradient_wipe_legacy"
                | "timecode"
                | "simple_text"
                | "clip_name"
                | "metadata_burnin"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParamValue, find_effect};
    use filmcraft_geom::Vec2;

    #[test]
    fn source_metadata_effects_require_a_clip() {
        for id in ["timecode", "clip_name", "metadata_burnin"] {
            assert!(!supports_track(find_effect(id).unwrap()), "{id} requires clip metadata");
        }
    }
    #[test]
    fn context_dependent_track_filters_are_rejected() {
        for id in ["track_matte", "compound_blur", "gradient_wipe", "gradient_wipe_legacy"] {
            assert!(!supports_track(find_effect(id).unwrap()), "{id} needs another track");
        }
    }

    #[test]
    fn catalog_defaults() {
        let def = find_effect("dynamic_zoom").unwrap();
        assert!(!supports_track(def));
        assert!(def.params.iter().all(|p| !p.animatable));
        let settings = DynamicZoomSettings::from_effect(&def.instance()).unwrap();
        assert_eq!(settings.start, DynamicZoomRect { center: Vec2::new(0.5, 0.5), width: 1.0 });
        assert_eq!(settings.end, DynamicZoomRect { center: Vec2::new(0.5, 0.5), width: 0.9 });
    }
    #[test]
    fn easing_exact_values() {
        use DynamicZoomEase::*;
        assert_eq!([Linear, EaseIn, EaseOut, EaseInOut].map(|e| e.sample(0.25)), [0.25, 0.0625, 0.4375, 0.15625]);
    }
    #[test]
    fn reverse_swaps_rects_and_width_is_logarithmic() {
        let mut effect = find_effect("dynamic_zoom").unwrap().instance();
        effect.param_mut("end_width").unwrap().value = ParamValue::Float(0.25);
        effect.param_mut("end_center").unwrap().value = ParamValue::Vec2(Vec2::new(0.75, 0.25));
        let settings = DynamicZoomSettings::from_effect(&effect).unwrap();
        let middle = settings.rect_at(0.5);
        assert!((middle.width - 0.5).abs() < 1e-12);
        assert_eq!(middle.center, Vec2::new(0.625, 0.375));
        effect.param_mut("reverse").unwrap().value = ParamValue::Bool(true);
        let reversed = DynamicZoomSettings::from_effect(&effect).unwrap();
        assert_eq!(reversed.rect_at(0.0), settings.end);
        assert_eq!(reversed.rect_at(1.0), settings.start);
    }
    #[test]
    fn malformed_rects_are_rejected() {
        for (param, value) in [
            ("start_width", ParamValue::Float(0.0)),
            ("end_width", ParamValue::Float(f64::NAN)),
            ("start_center", ParamValue::Vec2(Vec2::new(f64::INFINITY, 0.5))),
            ("end_center", ParamValue::Vec2(Vec2::new(3.1, 0.5))),
            ("easing", ParamValue::Choice(4)),
        ] {
            let mut effect = find_effect("dynamic_zoom").unwrap().instance();
            effect.param_mut(param).unwrap().value = value;
            assert!(DynamicZoomSettings::from_effect(&effect).is_err(), "{param}");
        }
    }
}
