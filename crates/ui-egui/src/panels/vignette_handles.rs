//! Source/sequence-space blur controls. A drag owns only its draft until release.
use super::preview_gizmos::GizmoSpace;
use crate::{FilmcraftApp, theme::Tokens};
use egui::{Align2, Color32, Rect, Sense, Stroke, vec2};
use filmcraft_engine::EffectTarget;
use filmcraft_geom::{Affine, Vec2};
use filmcraft_project::{EffectInstance, ItemId, ParamValue, Project, TrackKind};
use filmcraft_time::Tick;
use serde_json::{Value, json};
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    center: Vec2,
    width: f64,
    height: f64,
    angle: f64,
    softness: f64,
    roundness: f64,
    rectangle: bool,
    outside: bool,
}
impl Geometry {
    fn read(e: &EffectInstance, time: Tick, size: (u32, u32)) -> Self {
        let value = |id: &str| e.param(id).map(|p| p.value_at(time)).or_else(|| e.def()?.param(id).map(|p| p.default.clone()));
        let number = |id: &str, fallback: f64, lo: f64, hi: f64| value(id).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(fallback).clamp(lo, hi);
        let center = value("center").and_then(|v| v.as_vec2()).unwrap_or(Vec2::new(f64::NAN, f64::NAN));
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        Self {
            center: Vec2::new(
                if center.x.is_finite() { center.x.clamp(-4.0 * w, 4.0 * w) } else { w / 2.0 },
                if center.y.is_finite() { center.y.clamp(-4.0 * h, 4.0 * h) } else { h / 2.0 },
            ),
            width: number("width", 0.35, 0.0, 1.0),
            height: number("height", 0.35, 0.0, 1.0),
            angle: number("angle", 0.0, -36000.0, 36000.0),
            softness: number("softness", 0.05, 0.0, 0.25),
            roundness: number("roundness", 0.2, 0.0, 1.0),
            rectangle: number("shape", 0.0, 0.0, u32::MAX as f64) == 1.0,
            outside: value("outside").and_then(|v| v.as_bool()).unwrap_or(true),
        }
    }
    fn radii(self, size: (u32, u32)) -> (f64, f64) {
        (self.width * f64::from(size.0) / 2.0, self.height * f64::from(size.1) / 2.0)
    }
    fn world(self, p: Vec2) -> Vec2 {
        let (s, c) = self.angle.to_radians().sin_cos();
        self.center + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
    }
    fn local(self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        let (s, c) = self.angle.to_radians().sin_cos();
        Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c)
    }
    fn distance(self, p: Vec2, size: (u32, u32)) -> f64 {
        let (x, y) = self.radii(size);
        if x <= 1e-6 || y <= 1e-6 {
            return f64::from(size.0.max(size.1));
        }
        if self.rectangle {
            let r = self.roundness * x.min(y);
            let (a, b) = (p.x.abs() - x + r, p.y.abs() - y + r);
            a.max(0.0).hypot(b.max(0.0)) + a.max(b).min(0.0) - r
        } else {
            ((p.x / x).hypot(p.y / y) - 1.0) * x.min(y)
        }
    }
    fn softness_point(self, size: (u32, u32)) -> Option<Vec2> {
        let (x, y) = self.radii(size);
        let r = x.min(y);
        if r <= 1e-6 {
            return None;
        }
        let half = self.softness * f64::from(size.0.min(size.1)) / 2.0;
        let p = if self.rectangle {
            let corner = self.roundness * r;
            let d = (corner + half) / 2.0f64.sqrt();
            Vec2::new(x - corner + d, -y + corner - d)
        } else {
            let f = 1.0 + half / r;
            Vec2::new(x * f / 2.0f64.sqrt(), -y * f / 2.0f64.sqrt())
        };
        Some(self.world(p))
    }
    fn outline(self, size: (u32, u32), distance: f64) -> Vec<Vec2> {
        let (x, y) = self.radii(size);
        let mut points = Vec::with_capacity(65);
        if self.rectangle {
            let (x, y) = ((x + distance).max(0.0), (y + distance).max(0.0));
            let r = (self.roundness * self.radii(size).0.min(self.radii(size).1) + distance).clamp(0.0, x.min(y));
            for (cx, cy, start) in [(x - r, -y + r, -90.0), (x - r, y - r, 0.0), (-x + r, y - r, 90.0), (-x + r, -y + r, 180.0)] {
                for n in 0..=12 {
                    let angle = (start + n as f64 * 7.5).to_radians();
                    points.push(self.world(Vec2::new(cx + r * angle.cos(), cy + r * angle.sin())));
                }
            }
        } else {
            let r = x.min(y);
            let factor = if r > 1e-6 { (1.0 + distance / r).max(0.0) } else { 0.0 };
            for n in 0..=64 {
                let a = n as f64 * std::f64::consts::TAU / 64.0;
                points.push(self.world(Vec2::new(x * factor * a.cos(), y * factor * a.sin())));
            }
        }
        if let Some(p) = points.first().copied() {
            points.push(p);
        }
        points
    }
}

struct Info {
    sequence: ItemId,
    target: EffectTarget,
    index: usize,
    time: Tick,
    effect_time: Tick,
    size: (u32, u32),
    matrix: Affine,
    geometry: Geometry,
    dim: bool,
    locked: bool,
}
fn info(app: &FilmcraftApp, pic: Rect, frame: (u32, u32)) -> Option<Info> {
    if !app.ui.show_vignette_controls
        || app.ui.mask_pen.is_some()
        || app.session.state.selected_mask.is_some()
        || app.ui.eyedropper.is_some()
        || !matches!(app.ui.tool, crate::state::Tool::Selection | crate::state::Tool::Hand)
    {
        return None;
    }
    let selected = app.session.state.selected_effect.as_ref()?;
    let q = app.session.active_sequence()?;
    let sequence = app.session.state.active_sequence?;
    let time = app.session.playhead();
    let (effects, size, matrix, effect_time, bypassed, locked) = match selected.target {
        EffectTarget::Clip(clip) => {
            if !app.session.state.selection.contains(&clip) || matches!(app.session.state.effect_controls_target, Some(EffectTarget::VideoTrack(_))) {
                return None;
            }
            let (track, it) = q.find_item(clip)?;
            let tr = q.track(track)?;
            if !tr.enabled || !it.enabled || !it.range().contains(time) || tr.kind != TrackKind::Video {
                return None;
            }
            let size = filmcraft_render::source_size(&app.session.project, it.item)?;
            let m = filmcraft_render::dynamic_zoom::clip_matrix(&app.session.project, q, it, time, true)?;
            (&it.effects, size, super::graphics::frame_to_screen(pic, frame).then_apply(&m), it.effect_time_at(time), tr.video_effects_bypassed, tr.locked)
        }
        EffectTarget::VideoTrack(track) => {
            if app.session.state.effect_controls_target != Some(selected.target) {
                return None;
            }
            let tr = q.track(track)?;
            if !tr.enabled || tr.kind != TrackKind::Video || !tr.items.iter().any(|i| i.enabled && i.range().contains(time)) {
                return None;
            }
            (&tr.effects, (q.settings.width, q.settings.height), super::graphics::frame_to_screen(pic, frame), time, tr.video_effects_bypassed, tr.locked)
        }
    };
    if size.0 == 0 || size.1 == 0 {
        return None;
    }
    let index = selected.index(effects)?;
    let effect = effects.get(index)?;
    if effect.effect != "vignette_blur" {
        return None;
    }
    GizmoSpace::new(matrix)?;
    Some(Info {
        sequence,
        target: selected.target,
        index,
        time,
        effect_time,
        size,
        matrix,
        geometry: Geometry::read(effect, effect_time, size),
        dim: bypassed || !effect.enabled,
        locked,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Center,
    Width,
    Height,
    Rotation,
    Softness,
}
impl Grab {
    fn param(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Width => "width",
            Self::Height => "height",
            Self::Rotation => "angle",
            Self::Softness => "softness",
        }
    }
}
#[derive(Clone)]
struct Drag {
    sequence: ItemId,
    target: EffectTarget,
    index: usize,
    revision: u64,
    time: Tick,
    from: Geometry,
    draft: Geometry,
    size: (u32, u32),
    grab: Grab,
    origin: Vec2,
    offset: f64,
    last_angle: f64,
    turned: f64,
}
fn state_id() -> egui::Id {
    egui::Id::unique("vignette-gizmo-drag")
}
impl Drag {
    fn valid(&self, app: &FilmcraftApp, info: &Info) -> bool {
        self.sequence == info.sequence
            && self.target == info.target
            && self.index == info.index
            && self.time == info.time
            && self.revision == app.session.revision
            && app.ui.tool == crate::state::Tool::Selection
    }
    fn update(&mut self, p: Vec2) {
        let local = self.from.local(p);
        match self.grab {
            Grab::Center => {
                self.draft.center = self.from.center + (p - self.origin);
                self.draft.center.x = self.draft.center.x.clamp(-4.0 * f64::from(self.size.0), 4.0 * f64::from(self.size.0));
                self.draft.center.y = self.draft.center.y.clamp(-4.0 * f64::from(self.size.1), 4.0 * f64::from(self.size.1));
            }
            Grab::Width => self.draft.width = ((local.x - self.offset) * 2.0 / f64::from(self.size.0)).clamp(0.0, 1.0),
            Grab::Height => self.draft.height = ((local.y - self.offset) * 2.0 / f64::from(self.size.1)).clamp(0.0, 1.0),
            Grab::Softness => {
                self.draft.softness = (2.0 * self.from.distance(local, self.size) / f64::from(self.size.0.min(self.size.1)) + self.offset).clamp(0.0, 0.25)
            }
            Grab::Rotation => {
                let p = p - self.from.center;
                if p.length() > 1e-6 {
                    let angle = p.y.atan2(p.x);
                    let delta = (angle - self.last_angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
                    self.turned += delta.to_degrees();
                    self.last_angle = angle;
                    self.draft.angle = (self.from.angle + self.turned).clamp(-36000.0, 36000.0);
                }
            }
        }
    }
    fn value(&self) -> Value {
        match self.grab {
            Grab::Center => json!([self.draft.center.x, self.draft.center.y]),
            Grab::Width => json!(self.draft.width),
            Grab::Height => json!(self.draft.height),
            Grab::Rotation => json!(self.draft.angle),
            Grab::Softness => json!(self.draft.softness),
        }
    }
    fn param_value(&self) -> ParamValue {
        match self.grab {
            Grab::Center => ParamValue::Vec2(self.draft.center),
            Grab::Width => ParamValue::Float(self.draft.width),
            Grab::Height => ParamValue::Float(self.draft.height),
            Grab::Rotation => ParamValue::Float(self.draft.angle),
            Grab::Softness => ParamValue::Float(self.draft.softness),
        }
    }
    fn params(&self) -> Value {
        let mut p = json!({"effect":self.index,"param":self.grab.param(),"value":self.value()});
        match self.target {
            EffectTarget::Clip(c) => p["clip"] = json!(c.0),
            EffectTarget::VideoTrack(t) => p["track"] = json!(t.0),
        }
        p
    }
}

pub(crate) fn controls(app: &mut FilmcraftApp, ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 23.0), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 1.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let response = child.checkbox(&mut app.ui.show_vignette_controls, tl!("Show Controls"));
    app.auto.add_response("effectControls.vignetteBlur.showControls", &response, "Show Controls");
}

/// A separate cache target carries a private project snapshot, never the session's live document.
pub(crate) fn preview_project(app: &mut FilmcraftApp, ctx: &egui::Context, pic: Rect, frame: (u32, u32)) -> Option<(u64, Arc<Project>)> {
    let mut drag = ctx.data(|d| d.get_temp::<Drag>(state_id()))?;
    app.staged_pointer_drag(ctx);
    let current = info(app, pic, frame);
    let Some(current) = current.filter(|i| drag.valid(app, i)) else {
        ctx.data_mut(|d| d.remove::<Drag>(state_id()));
        return None;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape) || (!i.pointer.primary_down() && !i.pointer.primary_released())) {
        ctx.data_mut(|d| d.remove::<Drag>(state_id()));
        return None;
    }
    let space = GizmoSpace::new(current.matrix)?;
    if let Some(p) = ctx.input(|i| i.pointer.interact_pos()).and_then(|p| space.from_screen(p)) {
        drag.update(p);
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), drag.clone()));
    if drag.from == drag.draft {
        return None;
    }
    let mut project = (*app.session.project).clone();
    let q = project.sequence_mut(drag.sequence)?;
    let effects = match drag.target {
        EffectTarget::Clip(c) => &mut q.find_item_mut(c)?.1.effects,
        EffectTarget::VideoTrack(t) => &mut q.track_mut(t)?.effects,
    };
    let param = effects.get_mut(drag.index)?.params.get_mut(drag.grab.param())?;
    param.set_at(current.effect_time, drag.param_value());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    app.session.revision.hash(&mut hasher);
    drag.sequence.hash(&mut hasher);
    drag.target.hash(&mut hasher);
    drag.time.hash(&mut hasher);
    serde_json::to_string(&drag.params()).ok()?.hash(&mut hasher);
    Some((hasher.finish(), Arc::new(project)))
}

pub(crate) fn active(app: &FilmcraftApp, pic: Rect, frame: (u32, u32)) -> bool {
    info(app, pic, frame).is_some()
}
pub fn monitor_overlay(app: &mut FilmcraftApp, ui: &mut egui::Ui, pic: Rect, frame: (u32, u32)) {
    let mut drag = ui.data(|d| d.get_temp::<Drag>(state_id()));
    if drag.is_some() {
        app.staged_pointer_drag(ui.ctx());
    }
    let Some(current) = info(app, pic, frame) else {
        ui.data_mut(|d| d.remove::<Drag>(state_id()));
        return;
    };
    let Some(space) = GizmoSpace::new(current.matrix) else {
        return;
    };
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || drag.as_ref().is_some_and(|d| !d.valid(app, &current)) || ui.input(|i| !i.pointer.primary_down() && !i.pointer.primary_released()) {
        drag = None;
    }
    if let Some(d) = drag.as_mut()
        && let Some(p) = ui.input(|i| i.pointer.interact_pos()).and_then(|p| space.from_screen(p))
    {
        d.update(p);
    }
    let geometry = drag.as_ref().map_or(current.geometry, |d| d.draft);
    let color = if current.dim || current.locked { Color32::from_gray(115) } else { Color32::from_rgb(86, 210, 235) };
    let soft = geometry.softness * f64::from(current.size.0.min(current.size.1)) / 2.0;
    for (offset, stroke) in
        [(0.0, Stroke::new(1.5, color)), (-soft, Stroke::new(0.75, color.gamma_multiply(0.45))), (soft, Stroke::new(0.75, color.gamma_multiply(0.45)))]
    {
        let points = geometry.outline(current.size, offset).into_iter().map(|p| space.to_screen(p)).collect();
        ui.painter().add(egui::Shape::line(points, stroke));
    }
    let center = space.to_screen(geometry.center);
    let (x, y) = geometry.radii(current.size);
    let unit_x = space.to_screen(geometry.world(Vec2::new(1.0, 0.0))).distance(center).max(1e-5) as f64;
    let unit_y = space.to_screen(geometry.world(Vec2::new(0.0, 1.0))).distance(center).max(1e-5) as f64;
    let width_point = geometry.world(Vec2::new(x.max(12.0 / unit_x), 0.0));
    let height_point = geometry.world(Vec2::new(0.0, y.max(12.0 / unit_y)));
    let rotation_point = geometry.world(Vec2::new(0.0, -y - 26.0 / unit_y));
    ui.painter().line_segment([space.to_screen(geometry.world(Vec2::new(0.0, -y))), space.to_screen(rotation_point)], Stroke::new(1.0, color));
    let mut handles = vec![
        (Grab::Center, "center", geometry.center),
        (Grab::Width, "width", width_point),
        (Grab::Height, "height", height_point),
        (Grab::Rotation, "rotation", rotation_point),
    ];
    if let Some(point) = geometry.softness_point(current.size) {
        handles.push((Grab::Softness, "softness", point));
    }
    let interactive = app.ui.tool == crate::state::Tool::Selection && !current.locked && !escape;
    let mut commit = None;
    for (grab, name, point) in handles {
        let point = space.to_screen(point);
        let drawn = Rect::from_center_size(point, vec2(7.0, 7.0));
        if matches!(grab, Grab::Center | Grab::Rotation | Grab::Softness) {
            ui.painter().circle_stroke(point, 4.5, Stroke::new(1.5, color));
        } else {
            ui.painter().rect_filled(drawn, 0.0, color);
        }
        let tooltip = if current.dim {
            "FX bypassed/disabled · edit latent values"
        } else if current.locked {
            "Track locked"
        } else {
            grab.param()
        };
        let response = ui
            .interact(
                drawn.expand(5.0),
                egui::Id::unique(("vignette-handle", current.target, current.index, name)),
                if interactive { Sense::click_and_drag() } else { Sense::hover() },
            )
            .on_hover_text(tooltip);
        app.auto.add_response(&format!("program.vignette.{name}"), &response, grab.param());
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin()).and_then(|p| space.from_screen(p))
        {
            app.stop();
            app.staged_pointer_drag(ui.ctx());
            let local = geometry.local(origin);
            let offset = match grab {
                Grab::Width => local.x - x,
                Grab::Height => local.y - y,
                Grab::Softness => geometry.softness - 2.0 * geometry.distance(local, current.size) / f64::from(current.size.0.min(current.size.1)),
                _ => 0.0,
            };
            let relative = origin - geometry.center;
            let mut d = Drag {
                sequence: current.sequence,
                target: current.target,
                index: current.index,
                revision: app.session.revision,
                time: current.time,
                from: geometry,
                draft: geometry,
                size: current.size,
                grab,
                origin,
                offset,
                last_angle: relative.y.atan2(relative.x),
                turned: 0.0,
            };
            if let Some(p) = response.interact_pointer_pos().and_then(|p| space.from_screen(p)) {
                d.update(p);
            }
            drag = Some(d);
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) && !escape {
            commit = drag.take();
        }
    }
    ui.painter().text(
        center + vec2(8.0, 8.0),
        Align2::LEFT_TOP,
        crate::i18n::t(if geometry.outside { "Blur Outside Mask" } else { "Vignette Blur (Masked Gaussian Blur)" }),
        Tokens::ui(10.0),
        color,
    );
    ui.data_mut(|d| {
        if let Some(drag) = drag {
            d.insert_temp(state_id(), drag);
        } else {
            d.remove::<Drag>(state_id());
        }
    });
    if let Some(d) = commit
        && d.from != d.draft
        && let Err(error) = app.session.edit_group("Vignette Blur", |s| {
            s.execute("effects.setParam", d.params())?;
            Ok(())
        })
    {
        app.ui.status = error.to_string();
    }
}
