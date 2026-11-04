//! Undoable video-track inserts. All entry points are shared by UI and MCP.
use crate::commands::{CommandSpec, bad, bool_p, has_seq, str_p, time_p, track_p};
use crate::{EffectSel, EffectTarget, EngineError, Result, Session, effect_target};
use filmcraft_project::{EffectInstance, ParamDef, ParamKind, ParamValue, TrackKind};
use serde_json::{Value, json};

const CMD: &str = "video effects";

pub(crate) fn target(s: &Session, p: &Value, writing: bool) -> Result<EffectTarget> {
    if p.get("track").is_some() && (p.get("clip").is_some() || p.get("clips").is_some()) {
        return Err(bad(CMD, "choose either a video track or clips"));
    }
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let id = track_p(s, p, "track", CMD)?.ok_or_else(|| bad(CMD, "need `track`"))?;
    let tr = q.track(id).ok_or_else(|| bad(CMD, "track does not exist"))?;
    if tr.kind != TrackKind::Video {
        return Err(bad(CMD, "select a video track"));
    }
    if writing && tr.locked {
        return Err(bad(CMD, "video track is locked"));
    }
    Ok(EffectTarget::VideoTrack(id))
}

fn stack_index(effects: &[EffectInstance], p: &Value, field: &str) -> Result<usize> {
    let index = match p.get(field) {
        Some(Value::String(id)) => effects.iter().position(|e| &e.effect == id),
        Some(value) => value.as_u64().and_then(|n| usize::try_from(n).ok()).filter(|n| *n < effects.len()),
        None => None,
    };
    index.ok_or_else(|| bad(CMD, format!("no such effect (`{field}`)")))
}

/// Strict schema validation before an edit, including static parameters and choice names.
pub(crate) fn value_for(def: &ParamDef, value: &Value) -> Result<ParamValue> {
    let value = if let (ParamKind::Choice(options), Value::String(name)) = (&def.kind, value) {
        json!(options.iter().position(|o| o.eq_ignore_ascii_case(name)).ok_or_else(|| bad(CMD, "unknown choice"))?)
    } else {
        value.clone()
    };
    let v = crate::commands::json_to_param(&def.default, &value).ok_or_else(|| bad(CMD, "parameter value has the wrong type"))?;
    let valid = match (&def.kind, &v) {
        (ParamKind::Float { min, max, .. }, ParamValue::Float(x)) => x.is_finite() && x >= min && x <= max,
        (ParamKind::Angle, ParamValue::Float(x)) => x.is_finite() && x.abs() <= 1e9,
        (ParamKind::Point | ParamKind::Wheel, ParamValue::Vec2(p)) => p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e7 && p.y.abs() <= 1e7,
        (ParamKind::Color, ParamValue::Color(c)) => c.iter().all(|x| x.is_finite() && x.abs() <= 65504.0),
        (ParamKind::Choice(options), ParamValue::Choice(x)) => value.as_u64().is_some_and(|n| n < options.len() as u64) && (*x as usize) < options.len(),
        (ParamKind::Text, ParamValue::Text(t)) => t.len() <= 16384,
        (ParamKind::Curve { .. }, ParamValue::Curve(points)) => points.len() <= 4096 && points.iter().flatten().all(|x| x.is_finite()),
        (ParamKind::Path, _) => false,
        (ParamKind::Bool, ParamValue::Bool(_)) => true,
        _ => false,
    };
    if !valid {
        return Err(bad(CMD, "parameter value is outside supported bounds"));
    }
    Ok(v)
}

fn validate_aux(e: &EffectInstance, pid: &str, value: &ParamValue) -> Result<()> {
    if e.effect == "delta_key" && pid == "alpha_source" && matches!(value, ParamValue::Text(t) if !t.trim().is_empty()) {
        return Err(bad(CMD, "auxiliary alpha sources are available on clips only"));
    }
    Ok(())
}

pub(crate) fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let target = target(s, p, true)?;
    let name = str_p(p, "effect").ok_or_else(|| bad(CMD, "need effect"))?;
    let def = crate::commands::resolve_effect_name(s, p, name).ok_or_else(|| bad(CMD, "unknown effect"))?;
    if !filmcraft_project::video_fx::supports_track(def) {
        return Err(bad(CMD, "this effect requires a clip or another source"));
    }
    s.edit_sequence("Apply Track Effect", |q, _, st| {
        let frame = (q.settings.width, q.settings.height);
        let stack = effect_target::effects_mut(q, target)?;
        if stack.len() >= 128 {
            return Err(bad(CMD, "track effect limit reached (128)"));
        }
        let mut effect = def.instance();
        filmcraft_project::resolve_auto_points(&mut effect, frame, frame);
        stack.push(effect);
        st.selected_effect = EffectSel::of_target(target, stack, stack.len() - 1);
        st.effect_controls_target = Some(target);
        Ok(())
    })?;
    s.note_recent_effect(def.id);
    Ok(Value::Null)
}

fn merge_key(p: &Value, target: EffectTarget, operation: &str) -> Option<String> {
    match p.get("merge") {
        Some(Value::String(key)) => Some(format!("trackFx:{target:?}:{key}")),
        Some(Value::Bool(true)) => Some(format!("trackFx:{target:?}:{operation}:{}:{}", p["effect"], p["param"])),
        _ => None,
    }
}

pub(crate) fn set_param(s: &mut Session, p: &Value) -> Result<Value> {
    let target = target(s, p, true)?;
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let effects = effect_target::effects(q, target)?;
    let index = stack_index(effects, p, "effect")?;
    let e = effects.get(index).ok_or_else(|| bad(CMD, "no effect"))?;
    let pid = str_p(p, "param").ok_or_else(|| bad(CMD, "need param"))?;
    if p.get("mask").is_some_and(|v| !v.is_null()) {
        return Err(bad(CMD, "track effect masks are not supported"));
    }
    let def = e.def().and_then(|d| d.param(pid)).ok_or_else(|| bad(CMD, "unknown parameter"))?;
    let value = value_for(def, p.get("value").ok_or_else(|| bad(CMD, "need value"))?)?;
    validate_aux(e, pid, &value)?;
    let keyframe = bool_p(p, "keyframe").unwrap_or(false);
    if keyframe && !def.animatable {
        return Err(bad(CMD, "parameter cannot be animated"));
    }
    let at = effect_target::effect_time(q, target, time_p(s, p, "").unwrap_or(s.playhead()))?;
    let merge = merge_key(p, target, "setParam");
    if bool_p(p, "begin").unwrap_or(false) {
        s.history.merge_key = None;
    }
    let keys = s.edit_sequence_as("Change Track Effect Parameter", merge.as_deref(), |q, _, _| {
        let e = effect_target::effects_mut(q, target)?.get_mut(index).ok_or_else(|| bad(CMD, "effect no longer exists"))?;
        let prm = e.params.entry(pid.into()).or_insert_with(|| filmcraft_project::Param::new(def.default.clone()));
        if keyframe {
            prm.put_keyframe(at, value);
        } else {
            prm.set_at(at, value);
        }
        Ok(prm.keyframes.len())
    })?;
    Ok(json!({"keyframes":keys}))
}

fn edit_instance(s: &mut Session, p: &Value, op: &str) -> Result<Value> {
    let target = target(s, p, true)?;
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let stack = effect_target::effects(q, target)?;
    let index = stack_index(stack, p, if matches!(op, "resetParam" | "toggleAnimation") { "effect" } else { "index" })?;
    let selected = s.state.selected_effect.as_ref().filter(|e| e.target == target).and_then(|e| e.index(stack));
    let at = effect_target::effect_time(q, target, s.playhead())?;
    s.edit_sequence("Edit Track Effect", |q, _, st| {
        let frame = (q.settings.width, q.settings.height);
        let stack = effect_target::effects_mut(q, target)?;
        let e = stack.get_mut(index).ok_or_else(|| bad(CMD, "effect no longer exists"))?;
        match op {
            "toggleEnabled" => e.enabled = !e.enabled,
            "reset" => {
                crate::commands::reset_params(e, None, frame, frame, at);
            }
            "resetParam" => {
                let pid = str_p(p, "param").ok_or_else(|| bad(CMD, "need param"))?;
                if !crate::commands::reset_params(e, Some(pid), frame, frame, at) {
                    return Err(bad(CMD, "unknown param"));
                }
            }
            "toggleAnimation" => {
                let pid = str_p(p, "param").ok_or_else(|| bad(CMD, "need param"))?;
                if !e.def().and_then(|d| d.param(pid)).is_some_and(|d| d.animatable) {
                    return Err(bad(CMD, "parameter cannot be animated"));
                }
                e.param_mut(pid).ok_or_else(|| bad(CMD, "unknown param"))?.toggle_animation(at);
            }
            "remove" => {
                stack.remove(index);
                if let Some(i) = selected {
                    st.selected_effect = if i == index { None } else { EffectSel::of_target(target, stack, if i > index { i - 1 } else { i }) };
                }
            }
            _ => return Err(bad(CMD, "unsupported operation")),
        }
        Ok(())
    })?;
    Ok(Value::Null)
}
pub(crate) fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    edit_instance(s, p, "remove")
}
pub(crate) fn toggle_enabled(s: &mut Session, p: &Value) -> Result<Value> {
    edit_instance(s, p, "toggleEnabled")
}
pub(crate) fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    edit_instance(s, p, "reset")
}
pub(crate) fn reset_param(s: &mut Session, p: &Value) -> Result<Value> {
    edit_instance(s, p, "resetParam")
}
pub(crate) fn toggle_animation(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("mask").is_some_and(|v| !v.is_null()) {
        return Err(bad(CMD, "track effect masks are not supported"));
    }
    edit_instance(s, p, "toggleAnimation")
}

pub(crate) fn select_effect(s: &mut Session, p: &Value) -> Result<Value> {
    let target = target(s, p, false)?;
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let stack = effect_target::effects(q, target)?;
    let index = stack_index(stack, p, "effect")?;
    s.state.selected_effect = EffectSel::of_target(target, stack, index);
    s.state.selected_mask = None;
    s.state.effect_controls_target = Some(target);
    s.state.selection.clear();
    s.state.transition_selection.clear();
    Ok(Value::Null)
}

pub(crate) fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    let target = if p.get("track").is_some() {
        target(s, p, true)?
    } else {
        EffectTarget::Clip(crate::commands::clip_p(p, "clip").ok_or_else(|| bad(CMD, "need clip or track"))?)
    };
    let stack = effect_target::effects(s.active_sequence().ok_or(EngineError::NoSequence)?, target)?;
    let from = stack_index(stack, p, "from")?;
    let to = stack_index(stack, p, "to")?;
    if stack.get(from).and_then(|e| e.def()).is_some_and(|d| d.intrinsic) || stack.get(to).and_then(|e| e.def()).is_some_and(|d| d.intrinsic) {
        return Err(bad(CMD, "intrinsic effects cannot be reordered"));
    }
    let moves_zoom = [from, to].into_iter().any(|index| stack.get(index).is_some_and(|e| e.effect == "dynamic_zoom"));
    let crosses_intrinsic = stack.get(from.min(to)..=from.max(to)).is_some_and(|effects| effects.iter().any(|e| e.def().is_some_and(|d| d.intrinsic)));
    if moves_zoom || crosses_intrinsic {
        return Err(bad(CMD, "fixed geometry stages cannot be reordered"));
    }
    let selected = s.state.selected_effect.as_ref().filter(|e| e.target == target).and_then(|e| e.index(stack));
    s.edit_sequence("Reorder Effects", |q, _, st| {
        let stack = effect_target::effects_mut(q, target)?;
        let effect = stack.remove(from);
        stack.insert(to, effect);
        if let Some(i) = selected {
            let moved = if i == from {
                to
            } else if from < i && i <= to {
                i - 1
            } else if to <= i && i < from {
                i + 1
            } else {
                i
            };
            st.selected_effect = EffectSel::of_target(target, stack, moved);
        }
        Ok(())
    })?;
    Ok(Value::Null)
}

fn select_track(s: &mut Session, p: &Value) -> Result<Value> {
    let target = target(s, p, false)?;
    s.state.effect_controls_target = Some(target);
    s.state.selection.clear();
    s.state.transition_selection.clear();
    s.state.selected_mask = None;
    s.state.selected_effect = None;
    s.state.gap_selection = None;
    s.state.edit_points.clear();
    s.state.caption_selection.clear();
    Ok(Value::Null)
}
fn bypass(s: &mut Session, p: &Value) -> Result<Value> {
    let target = target(s, p, true)?;
    let EffectTarget::VideoTrack(id) = target else { return Err(bad(CMD, "need video track")) };
    let bypass = bool_p(p, "bypass").ok_or_else(|| bad(CMD, "need boolean bypass"))?;
    s.edit_sequence("Bypass Video Track Effects", |q, _, _| {
        q.track_mut(id).ok_or_else(|| bad(CMD, "track no longer exists"))?.video_effects_bypassed = bypass;
        Ok(())
    })?;
    Ok(Value::Null)
}
pub(crate) fn commands() -> Vec<CommandSpec> {
    [
        ("timeline.selectVideoTrack", "Select Video Track", r#"{"track":"V1"|id}"#, select_track as fn(&mut Session, &Value) -> Result<Value>, false),
        ("timeline.setVideoFxBypass", "Bypass Video Effects", r#"{"track":"V1"|id,"bypass":bool}"#, bypass, true),
        ("effects.reorder", "Reorder Effects", r#"{"clip":id|"track":id,"from":index,"to":index}"#, reorder, true),
    ]
    .into_iter()
    .map(|(id, label, params, run, journal)| CommandSpec { id, label, params, run, journal, menu: &[], shortcut: None, enabled: has_seq })
    .collect()
}
