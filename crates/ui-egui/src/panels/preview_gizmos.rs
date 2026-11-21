//! Finite, invertible coordinate space shared by preview overlays.
use egui::{Pos2, pos2};
use filmcraft_geom::{Affine, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct GizmoSpace {
    forward: Affine,
    inverse: Affine,
}
fn finite(m: Affine) -> bool {
    [m.a, m.b, m.c, m.d, m.e, m.f, m.determinant()].iter().all(|v| v.is_finite())
}
impl GizmoSpace {
    pub fn new(to_screen: Affine) -> Option<Self> {
        if !finite(to_screen) {
            return None;
        }
        let inverse = to_screen.inverse()?;
        finite(inverse).then_some(Self { forward: to_screen, inverse })
    }
    pub fn to_screen(&self, p: Vec2) -> Pos2 {
        let q = self.forward.apply(p);
        let out = pos2(q.x as f32, q.y as f32);
        if out.x.is_finite() && out.y.is_finite() { out } else { Pos2::ZERO }
    }
    pub fn from_screen(&self, p: Pos2) -> Option<Vec2> {
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        let q = self.inverse.apply(Vec2::new(f64::from(p.x), f64::from(p.y)));
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    }
}
