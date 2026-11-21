//! Video-track inspector. It edits the real stack using sequence ticks, without synthetic clips.
use egui::{Align2, Rect, Sense, Stroke, pos2, vec2};
use filmcraft_engine::EffectTarget;
use filmcraft_project::{EffectInstance, TrackId};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::effect_controls::value_editor;
use crate::{FilmcraftApp, theme::Tokens};

#[derive(Clone)]
struct KeyDrag {
    revision: u64,
    track: TrackId,
    sequence: filmcraft_project::ItemId,
    index: usize,
    effect: EffectInstance,
    param: String,
    from: Tick,
    to: Tick,
    origin: f32,
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, track: TrackId) {
    let project = app.session.project.clone();
    let Some(sequence_id) = app.session.state.active_sequence else { return };
    let Some(q) = project.sequence(sequence_id) else { return };
    let Some(tr) = q.track(track) else { return };
    let prefix = format!("effectControls.track.{}", track.0);
    app.auto.add(&prefix, rect, &format!("Track: {}", tr.name));
    let t = app.tokens;
    let time = app.session.playhead();
    let duration = q.duration().max(q.settings.frame_rate.frame_duration());
    let mut actions: Vec<(&str, Value)> = Vec::new();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(6.0)).id_salt(("track-effect-controls", track.0)));
    child.set_clip_rect(rect);
    child.label(egui::RichText::new(tlf!("Track: {name}", name = tr.name)).strong());
    if tr.video_effects_bypassed {
        child.colored_label(t.accent, tl!("FX bypassed · values remain editable"));
    }
    if tr.locked {
        child.colored_label(t.text_dim, tl!("Track locked · unlock to edit effects"));
    }
    let add = child.button(tl!("+ Add effect · Shift+Space"));
    app.auto.add_response(&format!("{prefix}.add"), &add, "Add effect");
    if add.clicked()
        && let Err(error) = super::effects_search::open(app)
    {
        app.ui.status = error;
    }
    child.label(egui::RichText::new(tl!("Keyframes · sequence time")).small().color(t.text_dim));
    let drag_id = egui::Id::unique("track-effect-key-drag");
    let mut drag = ui.data(|d| d.get_temp::<KeyDrag>(drag_id));
    if drag.is_some() {
        app.staged_pointer_drag(ui.ctx());
    }
    if drag
        .as_ref()
        .is_some_and(|d| d.revision != app.session.revision || d.track != track || d.sequence != sequence_id || tr.effects.get(d.index) != Some(&d.effect))
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        drag = None;
    }
    egui::ScrollArea::vertical().id_salt(("track-fx-scroll", track.0)).auto_shrink([false, false]).show(&mut child, |ui| {
        if tr.effects.is_empty() {
            ui.label(tl!("No track effects. Clip effects remain independent."));
        }
        for (index, effect) in tr.effects.iter().enumerate() {
            let base = format!("{prefix}.effect.{index}");
            let selected =
                app.session.state.selected_effect.as_ref().is_some_and(|s| s.target == EffectTarget::VideoTrack(track) && s.index(&tr.effects) == Some(index));
            let name = effect.def().map_or(effect.effect.as_str(), |d| d.name);
            let supported = effect.def().is_some_and(filmcraft_project::video_fx::supports_track);
            ui.horizontal(|ui| {
                let enabled = ui.button(if effect.enabled { "fx" } else { "fx̸" });
                app.auto.add_response(&format!("{base}.enabled"), &enabled, "Toggle Effect");
                if enabled.clicked() {
                    actions.push(("effects.toggleEnabled", json!({"track":track.0,"index":index})));
                }
                let label = ui.selectable_label(selected, crate::i18n::t(name));
                app.auto.add_response(&base, &label, name);
                if label.clicked() {
                    actions.push(("effects.select", json!({"track":track.0,"effect":index})));
                }
                for (suffix, text, command, params, enabled) in [
                    ("up", "↑", "effects.reorder", json!({"track":track.0,"from":index,"to":index.saturating_sub(1)}), index > 0),
                    ("down", "↓", "effects.reorder", json!({"track":track.0,"from":index,"to":index.saturating_add(1)}), index + 1 < tr.effects.len()),
                    ("reset", "↺", "effects.reset", json!({"track":track.0,"index":index}), true),
                    ("remove", "×", "effects.remove", json!({"track":track.0,"index":index}), true),
                ] {
                    let button = ui.add_enabled(enabled, egui::Button::new(text));
                    app.auto.add_response(&format!("{base}.{suffix}"), &button, suffix);
                    if button.clicked() {
                        actions.push((command, params));
                    }
                }
            });
            if !supported {
                ui.colored_label(t.text_dim, tl!("Unsupported track effect · not rendered"));
                continue;
            }
            let Some(def) = effect.def() else { continue };
            if effect.effect == "vignette_blur" && selected {
                super::vignette_handles::controls(app, ui, ui.available_width());
            }
            for pd in &def.params {
                let Some(param) = effect.params.get(pd.id) else { continue };
                let id = format!("{base}.param.{}", pd.id);
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                let lane_width = (row.width() * 0.22).clamp(36.0, 220.0);
                let lane = Rect::from_min_max(pos2(row.max.x - lane_width, row.min.y), row.max);
                let controls = Rect::from_min_max(row.min, pos2(lane.min.x - 4.0, row.max.y));
                let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(controls).id_salt(&id).layout(egui::Layout::left_to_right(egui::Align::Center)));
                rui.spacing_mut().item_spacing.x = 3.0;
                if pd.animatable {
                    let sw = crate::icons::button(&mut rui, crate::icons::Icon::Stopwatch, 18.0, param.is_animated(), &t, tl!("Toggle animation"));
                    app.auto.add_response(&format!("{id}.stopwatch"), &sw, "Toggle animation");
                    if sw.clicked() {
                        actions.push(("effects.toggleAnimation", json!({"track":track.0,"effect":index,"param":pd.id})));
                    }
                }
                let label_width = (controls.width() * 0.30).clamp(40.0, 108.0);
                let label = rui.add_sized(vec2(label_width, 18.0), egui::Label::new(crate::i18n::t(pd.label)).truncate());
                label.on_hover_text(pd.label);
                let reserved = if pd.animatable && param.is_animated() { 144.0 } else { 86.0 };
                let value_rect = Rect::from_min_size(rui.cursor().min, vec2((controls.width() - label_width - reserved).max(40.0), 20.0));
                let mut vui =
                    rui.new_child(egui::UiBuilder::new().max_rect(value_rect).id_salt("value").layout(egui::Layout::left_to_right(egui::Align::Center)));
                let edit = value_editor(app, &mut vui, egui::Id::unique((&id, "value")), pd, &param.value_at(time), false);
                app.auto.add(&format!("{id}.value"), vui.min_rect(), pd.label);
                rui.advance_cursor_after_rect(value_rect);
                if let Some(value) = edit.value {
                    let mut params = json!({"track":track.0,"effect":index,"param":pd.id,"value":value});
                    if edit.dragging {
                        params["merge"] = json!(true);
                        params["begin"] = json!(edit.begin);
                    }
                    actions.push(("effects.setParam", params));
                }
                let reset = rui.small_button("↺");
                app.auto.add_response(&format!("{id}.reset"), &reset, "Reset Parameter");
                if reset.clicked() {
                    actions.push(("effects.resetParam", json!({"track":track.0,"effect":index,"param":pd.id})));
                }
                ui.painter().line_segment([lane.left_center(), lane.right_center()], Stroke::new(1.0, t.separator));
                if pd.animatable && param.is_animated() {
                    for (suffix, text, next) in
                        [("previous", "‹", param.keyframes.iter().rev().find(|k| k.time < time)), ("next", "›", param.keyframes.iter().find(|k| k.time > time))]
                    {
                        let b = rui.add_enabled(next.is_some(), egui::Button::new(text).small());
                        app.auto.add_response(&format!("{id}.{suffix}"), &b, suffix);
                        if b.clicked()
                            && let Some(k) = next
                        {
                            actions.push(("playhead.set", json!({"time":k.time.0})));
                        }
                    }
                    let key = rui.small_button(if param.keyframes.iter().any(|k| k.time == time) { "◆" } else { "◇" });
                    app.auto.add_response(&format!("{id}.keyframe"), &key, "Add/Remove Keyframe");
                    if key.clicked() {
                        actions.push(("effects.addKeyframe", json!({"track":track.0,"effect":index,"param":pd.id})));
                    }
                    for keyframe in &param.keyframes {
                        let active = drag.as_ref().filter(|d| d.index == index && d.param == pd.id && d.from == keyframe.time);
                        let at = active.map_or(keyframe.time, |d| d.to);
                        let x = lane.min.x + (at.0 as f64 / duration.0 as f64).clamp(0.0, 1.0) as f32 * lane.width();
                        let diamond = Rect::from_center_size(pos2(x, lane.center().y), vec2(10.0, 14.0));
                        let response = ui.interact(diamond.expand(2.0), egui::Id::unique((&id, keyframe.time.0)), Sense::click_and_drag());
                        app.auto.add_response(&format!("{id}.key.{}", keyframe.time.0), &response, "Sequence keyframe");
                        ui.painter().text(diamond.center(), Align2::CENTER_CENTER, "◆", Tokens::ui(12.0), t.accent);
                        if response.drag_started() {
                            app.staged_pointer_drag(ui.ctx());
                            drag = Some(KeyDrag {
                                revision: app.session.revision,
                                track,
                                sequence: sequence_id,
                                index,
                                effect: effect.clone(),
                                param: pd.id.into(),
                                from: keyframe.time,
                                to: keyframe.time,
                                origin: ui.input(|i| i.pointer.press_origin()).map_or(x, |p| p.x),
                            });
                        }
                        if response.dragged()
                            && let Some(d) = drag.as_mut()
                            && let Some(pointer) = response.interact_pointer_pos()
                        {
                            let delta = (f64::from(pointer.x - d.origin) / f64::from(lane.width()) * duration.0 as f64).round();
                            let ticks = d.from.0.saturating_add(delta as i64).clamp(0, duration.0);
                            d.to = q.settings.frame_rate.tick_of(q.settings.frame_rate.frame_at(Tick(ticks)));
                        }
                        if response.drag_stopped()
                            && let Some(d) = drag.take()
                            && d.to != d.from
                        {
                            actions.push(("effects.moveKeyframe", json!({"track":track.0,"effect":index,"param":pd.id,"mediaTime":d.from.0,"to":d.to.0})));
                        }
                        if response.clicked() {
                            actions.push(("playhead.set", json!({"time":keyframe.time.0})));
                        }
                        response.context_menu(|ui| {
                            for (name, interp) in [("Linear", "linear"), ("Ease In", "easeIn"), ("Ease Out", "easeOut"), ("Hold", "hold")] {
                                if ui.button(crate::i18n::t(name)).clicked() {
                                    actions.push((
                                        "effects.setInterpolation",
                                        json!({"track":track.0,"effect":index,"param":pd.id,"mediaTime":keyframe.time.0,"interpolation":interp}),
                                    ));
                                    ui.close();
                                }
                            }
                            if ui.button(tl!("Delete Keyframe")).clicked() {
                                actions.push(("effects.deleteKeyframe", json!({"track":track.0,"effect":index,"param":pd.id,"mediaTime":keyframe.time.0})));
                                ui.close();
                            }
                        });
                    }
                }
                let px = lane.min.x + (time.0 as f64 / duration.0 as f64).clamp(0.0, 1.0) as f32 * lane.width();
                ui.painter().line_segment([pos2(px, row.min.y), pos2(px, row.max.y)], Stroke::new(1.0, t.playhead));
            }
            ui.separator();
        }
    });
    ui.data_mut(|d| {
        if let Some(drag) = drag {
            d.insert_temp(drag_id, drag);
        } else {
            d.remove::<KeyDrag>(drag_id);
        }
    });
    for (command, params) in actions {
        if let Err(error) = app.session.execute(command, params) {
            app.ui.status = error.to_string();
        }
    }
}
