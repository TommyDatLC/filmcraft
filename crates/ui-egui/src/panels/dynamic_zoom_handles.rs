//! Two sequence-space viewports, edited as a draft and committed atomically on release.
use super::preview_gizmos::GizmoSpace;
use crate::{FilmcraftApp, state::DynamicZoomRectSelection as Role, theme::Tokens};
use egui::{Align2, Color32, Rect, Sense, Stroke, vec2};
use filmcraft_geom::Vec2;
use filmcraft_project::{
    ClipId, EffectInstance, ItemId,
    video_fx::{DynamicZoomRect, DynamicZoomSettings},
};
use serde_json::{Value, json};

#[derive(Clone)]
struct Drag {
    clip: ClipId,
    sequence: ItemId,
    index: usize,
    revision: u64,
    role: Role,
    from: DynamicZoomRect,
    draft: DynamicZoomRect,
    origin: Vec2,
    corner: Option<usize>,
    reverse: bool,
}
fn drag_id() -> egui::Id {
    egui::Id::unique("dynamic-zoom-drag")
}

/// The selected geometry instance, only for a directly selected clip under the playhead.
fn target(app: &FilmcraftApp) -> Option<(ItemId, ClipId, usize, DynamicZoomSettings)> {
    let selected = app.session.state.selected_effect.as_ref()?;
    let clip = selected.clip()?;
    if !app.session.state.selection.contains(&clip) {
        return None;
    }
    let sequence = app.session.state.active_sequence?;
    let q = app.session.active_sequence()?;
    let (track, item) = q.find_item(clip)?;
    if !q.track(track)?.enabled || !item.enabled || !item.range().contains(app.session.playhead()) {
        return None;
    }
    let index = selected.index(&item.effects)?;
    let effect = item.effects.get(index)?;
    if effect.effect != "dynamic_zoom" {
        return None;
    }
    Some((sequence, clip, index, DynamicZoomSettings::from_effect(effect).ok()?))
}
pub(crate) fn preview_target(app: &mut FilmcraftApp) -> Option<(ItemId, ClipId)> {
    let selected = target(app);
    if selected.as_ref().map(|t| t.1) != app.ui.dynamic_zoom_edit {
        app.ui.dynamic_zoom_edit = None;
    }
    if app.playback.playing {
        return None;
    }
    let (sequence, clip, _, _) = selected?;
    (app.ui.dynamic_zoom_edit == Some(clip)).then_some((sequence, clip))
}

pub(crate) fn controls(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    body: Rect,
    clip: ClipId,
    index: usize,
    effect: &EffectInstance,
    actions: &mut Vec<(String, Value)>,
) {
    let t = app.tokens;
    for row in 0..2 {
        let (rect, _) = ui.allocate_exact_size(vec2(body.width(), 24.0), Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(6.0, 1.0)))
                .id_salt(("dynamic-zoom-controls", clip.0, row))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 4.0;
        if row == 0 {
            let mut editing = app.ui.dynamic_zoom_edit == Some(clip);
            let edit = child.checkbox(&mut editing, tl!("Edit Start/End"));
            app.auto.add_response("effectControls.dynamicZoom.edit", &edit, "Edit Start/End");
            if edit.changed() {
                app.ui.dynamic_zoom_edit = editing.then_some(clip);
                actions.push(("effects.select".into(), json!({"clip":clip.0,"effect":index})));
            }
            for (role, label, id) in [(Role::Start, tl!("Start"), "start"), (Role::End, tl!("End"), "end")] {
                let button = child.selectable_label(app.ui.dynamic_zoom_active == role, label);
                app.auto.add_response(&format!("effectControls.dynamicZoom.active.{id}"), &button, label);
                if button.clicked() {
                    app.ui.dynamic_zoom_active = role;
                }
            }
        } else {
            let mut reverse = effect.param("reverse").and_then(|p| p.value.as_bool()).unwrap_or(false);
            let button = child.checkbox(&mut reverse, tl!("Reverse"));
            app.auto.add_response("effectControls.dynamicZoom.reverse", &button, "Reverse");
            if button.changed() {
                actions.push(("effects.setParam".into(), json!({"clip":clip.0,"effect":index,"param":"reverse","value":reverse})));
            }
            let mut ease = match effect.param("easing").map(|p| &p.value) {
                Some(filmcraft_project::ParamValue::Choice(value)) => *value,
                _ => 0,
            };
            let labels = [tl!("Linear"), tl!("Ease In"), tl!("Ease Out"), tl!("Ease In-Out")];
            let combo = craft_ui::ComboBox::from_id_salt(("dynamic-zoom-ease", clip.0))
                .width(112.0)
                .selected_text(labels.get(ease as usize).copied().unwrap_or(labels[0]))
                .show_ui(&mut child, |ui| {
                    for (i, label) in labels.iter().enumerate() {
                        let r = ui.selectable_value(&mut ease, i as u32, *label);
                        app.auto.add_response(&format!("effectControls.dynamicZoom.easing.{i}"), &r, label);
                        if r.changed() {
                            actions.push(("effects.setParam".into(), json!({"clip":clip.0,"effect":index,"param":"easing","value":i})));
                        }
                    }
                });
            app.auto.add_response("effectControls.dynamicZoom.easing", &combo.response, "Easing");
        }
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.separator));
    }
}

fn box_rect(space: &GizmoSpace, rect: DynamicZoomRect, frame: (u32, u32)) -> Rect {
    let (w, h) = (f64::from(frame.0), f64::from(frame.1));
    Rect::from_two_pos(
        space.to_screen(Vec2::new((rect.center.x - rect.width / 2.0) * w, (rect.center.y - rect.width / 2.0) * h)),
        space.to_screen(Vec2::new((rect.center.x + rect.width / 2.0) * w, (rect.center.y + rect.width / 2.0) * h)),
    )
}
fn updated(drag: &Drag, pointer: Vec2, frame: (u32, u32)) -> DynamicZoomRect {
    let normalized = Vec2::new(pointer.x / f64::from(frame.0), pointer.y / f64::from(frame.1));
    let origin = Vec2::new(drag.origin.x / f64::from(frame.0), drag.origin.y / f64::from(frame.1));
    let mut next = drag.from;
    if let Some(corner) = drag.corner {
        let (sx, sy) = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)][corner];
        let opposite = Vec2::new(drag.from.center.x - sx * drag.from.width / 2.0, drag.from.center.y - sy * drag.from.width / 2.0);
        next.width = ((normalized.x - opposite.x) * sx).max((normalized.y - opposite.y) * sy).clamp(0.01, 4.0);
        next.center = Vec2::new(opposite.x + sx * next.width / 2.0, opposite.y + sy * next.width / 2.0);
    } else {
        next.center = drag.from.center + (normalized - origin);
    }
    next.center.x = next.center.x.clamp(-2.0, 3.0);
    next.center.y = next.center.y.clamp(-2.0, 3.0);
    if next.is_valid() { next } else { drag.from }
}

pub fn monitor_overlay(app: &mut FilmcraftApp, ui: &mut egui::Ui, pic: Rect, frame: (u32, u32)) {
    let Some((sequence, clip, index, settings)) = target(app) else {
        ui.data_mut(|d| d.remove::<Drag>(drag_id()));
        return;
    };
    if preview_target(app).is_none() || frame.0 == 0 || frame.1 == 0 {
        ui.data_mut(|d| d.remove::<Drag>(drag_id()));
        return;
    }
    let Some(space) = GizmoSpace::new(super::graphics::frame_to_screen(pic, frame)) else {
        return;
    };
    let mut drag = ui.data(|d| d.get_temp::<Drag>(drag_id()));
    if drag.is_some() {
        app.staged_pointer_drag(ui.ctx());
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape
        || drag.as_ref().is_some_and(|d| {
            d.clip != clip || d.sequence != sequence || d.index != index || d.revision != app.session.revision || d.role != app.ui.dynamic_zoom_active
        })
        || ui.input(|i| !i.pointer.primary_down() && !i.pointer.primary_released())
    {
        drag = None;
    }
    if let Some(d) = drag.as_mut()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos()).and_then(|p| space.from_screen(p))
    {
        d.draft = updated(d, pointer, frame);
    }
    let interactive = app.ui.tool == crate::state::Tool::Selection && app.ui.eyedropper.is_none() && app.ui.mask_pen.is_none() && !escape;
    let mut commit = None;
    let active = app.ui.dynamic_zoom_active;
    let roles = if active == Role::Start { [Role::End, Role::Start] } else { [Role::Start, Role::End] };
    for role in roles {
        let initial = if (role == Role::Start) != settings.reverse { settings.start } else { settings.end };
        let rect = drag.as_ref().filter(|d| d.role == role).map_or(initial, |d| d.draft);
        let screen = box_rect(&space, rect, frame);
        let (name, label, color) =
            if role == Role::Start { ("start", tl!("Start"), Color32::from_rgb(70, 220, 140)) } else { ("end", tl!("End"), Color32::from_rgb(255, 150, 65)) };
        let color = if app
            .session
            .active_sequence()
            .and_then(|q| q.find_item(clip))
            .and_then(|(track, _)| app.session.active_sequence()?.track(track))
            .is_some_and(|t| t.video_effects_bypassed)
        {
            color.gamma_multiply(0.45)
        } else {
            color
        };
        ui.painter().rect_stroke(screen, 0.0, Stroke::new(if active == role { 2.0 } else { 1.0 }, color), egui::StrokeKind::Inside);
        let label_at = if role == Role::Start { screen.left_top() + vec2(8.0, 10.0) } else { screen.right_bottom() - vec2(8.0, 10.0) };
        ui.painter().text(label_at, if role == Role::Start { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER }, label, Tokens::semibold(12.0), color);
        app.auto.add(&format!("program.dynamicZoom.{name}.box"), screen, label);
        let label_rect = Rect::from_center_size(label_at + if role == Role::Start { vec2(15.0, 0.0) } else { vec2(-15.0, 0.0) }, vec2(44.0, 18.0));
        let label_response = ui.interact(label_rect, egui::Id::unique(("dynamic-zoom-label", name)), Sense::click());
        if label_response.clicked() {
            app.ui.dynamic_zoom_active = role;
        }
        if role != active || !interactive {
            continue;
        }
        let whole = ui.interact(screen, egui::Id::unique(("dynamic-zoom-box", clip.0, name)), Sense::click_and_drag());
        let corners = [screen.left_top(), screen.right_top(), screen.right_bottom(), screen.left_bottom()];
        let mut responses = vec![(None, whole)];
        for (n, p) in corners.into_iter().enumerate() {
            let drawn = Rect::from_center_size(p, vec2(6.0, 6.0));
            ui.painter().rect_filled(drawn, 0.0, color);
            let r = ui.interact(drawn.expand(5.0), egui::Id::unique(("dynamic-zoom-corner", clip.0, name, n)), Sense::click_and_drag());
            app.auto.add_response(&format!("program.dynamicZoom.{name}.handle.{n}"), &r, "Resize viewport");
            responses.push((Some(n), r));
        }
        for (corner, response) in responses {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(origin) = ui.input(|i| i.pointer.press_origin()).and_then(|p| space.from_screen(p))
            {
                app.staged_pointer_drag(ui.ctx());
                let mut d = Drag {
                    clip,
                    sequence,
                    index,
                    revision: app.session.revision,
                    role,
                    from: initial,
                    draft: initial,
                    origin,
                    corner,
                    reverse: settings.reverse,
                };
                if let Some(pointer) = response.interact_pointer_pos().and_then(|p| space.from_screen(p)) {
                    d.draft = updated(&d, pointer, frame);
                }
                drag = Some(d);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) && !escape {
                commit = drag.take();
            }
        }
    }
    ui.painter().text(pic.left_top() + vec2(6.0, 26.0), Align2::LEFT_TOP, tl!("Edit Dynamic Zoom"), Tokens::ui(11.0), app.tokens.text);
    ui.data_mut(|d| {
        if let Some(drag) = drag {
            d.insert_temp(drag_id(), drag);
        } else {
            d.remove::<Drag>(drag_id());
        }
    });
    if let Some(d) = commit
        && d.draft != d.from
        && d.draft.is_valid()
    {
        let prefix = if (d.role == Role::Start) != d.reverse { "start" } else { "end" };
        if let Err(error) = app.session.edit_group("Dynamic Zoom", |s| {
            s.execute(
                "effects.setParam",
                json!({"clip":clip.0,"effect":index,"param":format!("{prefix}_center"),"value":[d.draft.center.x,d.draft.center.y]}),
            )?;
            s.execute("effects.setParam", json!({"clip":clip.0,"effect":index,"param":format!("{prefix}_width"),"value":d.draft.width}))?;
            Ok(())
        }) {
            app.ui.status = error.to_string();
        }
    }
}
