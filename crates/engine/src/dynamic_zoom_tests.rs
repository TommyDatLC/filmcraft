use super::*;
use filmcraft_project::{ParamValue, video_fx::DynamicZoomSettings};
use serde_json::json;

fn fixture() -> (Session, ClipId, usize) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let id = s.active_sequence().unwrap().video_tracks[0].items[0].id;
    s.execute("effects.apply", json!({"clips":[id.0],"effect":"dynamic_zoom"})).unwrap();
    let index = s.active_sequence().unwrap().find_item(id).unwrap().1.effects.iter().position(|e| e.effect == "dynamic_zoom").unwrap();
    (s, id, index)
}

#[test]
fn invalid_rect_commands_are_atomic() {
    let (mut s, id, index) = fixture();
    for (param, value) in [
        ("start_width", json!(0.0)),
        ("end_width", json!(1e99)),
        ("end_center", json!([3.1, 0.5])),
        ("start_center", json!(["NaN", 0.5])),
        ("easing", json!(99)),
        ("easing", json!(1_u64 << 32)),
    ] {
        let project = s.project.clone();
        let undo = s.history.undo.len();
        s.history.merge_key = Some("existing gesture".into());
        let state = serde_json::to_value(&s.state).unwrap();
        assert!(
            s.execute("effects.setParam", json!({"clip":id.0,"effect":index,"param":param,"value":value,"begin":true})).is_err(),
            "{param} must reject invalid data"
        );
        assert_eq!(*s.project, *project);
        assert_eq!(s.history.undo.len(), undo);
        assert_eq!(s.history.merge_key.as_deref(), Some("existing gesture"));
        assert_eq!(serde_json::to_value(&s.state).unwrap(), state);
    }
}

#[test]
fn automatic_geometry_does_not_accept_keyframes_or_masks() {
    let (mut s, id, index) = fixture();
    for (command, params) in [
        ("effects.toggleAnimation", json!({"clip":id.0,"effect":index,"param":"end_width"})),
        ("effects.addKeyframe", json!({"clip":id.0,"effect":index,"param":"end_width"})),
        ("masks.add", json!({"clip":id.0,"effect":index,"shape":"ellipse"})),
    ] {
        let project = s.project.clone();
        let undo = s.history.undo.len();
        assert!(s.execute(command, params).is_err(), "{command}");
        assert_eq!(*s.project, *project);
        assert_eq!(s.history.undo.len(), undo);
    }
}

#[test]
fn dynamic_zoom_keeps_existing_effect_indices_and_cannot_be_reordered() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let id = s.active_sequence().unwrap().video_tracks[0].items[0].id;
    let blur = s.execute("effects.apply", json!({"clips":[id.0],"effect":"gaussian_blur"})).unwrap();
    let blur_index = blur["applied"][0]["index"].as_u64().unwrap() as usize;
    s.execute("effects.apply", json!({"clips":[id.0],"effect":"dynamic_zoom"})).unwrap();
    let effects = &s.active_sequence().unwrap().find_item(id).unwrap().1.effects;
    assert_eq!(effects[blur_index].effect, "gaussian_blur");
    let index = effects.iter().position(|e| e.effect == "dynamic_zoom").unwrap();
    let before = s.project.clone();
    assert!(s.execute("effects.apply", json!({"clips":[id.0],"effect":"dynamic_zoom"})).is_err());
    assert_eq!(*s.project, *before);
    assert!(s.execute("effects.reorder", json!({"clip":id.0,"from":index,"to":0})).is_err());
    let sharpen = s.execute("effects.apply", json!({"clips":[id.0],"effect":"sharpen"})).unwrap();
    let sharpen_index = sharpen["applied"][0]["index"].as_u64().unwrap() as usize;
    s.execute("effects.reorder", json!({"clip":id.0,"from":blur_index,"to":sharpen_index})).unwrap();
    let effects = &s.active_sequence().unwrap().find_item(id).unwrap().1.effects;
    assert_eq!(effects[sharpen_index].effect, "gaussian_blur");
}

#[test]
fn reset_repairs_incompatible_imported_animation() {
    let (mut s, id, index) = fixture();
    s.edit_sequence("old data", |q, _, _| {
        let effect = &mut q.find_item_mut(id).unwrap().1.effects[index];
        effect.param_mut("end_width").unwrap().put_keyframe(Tick::ZERO, ParamValue::Float(0.0));
        Ok(())
    })
    .unwrap();
    s.execute("effects.reset", json!({"clip":id.0,"index":index})).unwrap();
    let effect = &s.active_sequence().unwrap().find_item(id).unwrap().1.effects[index];
    assert!(effect.params.values().all(|p| p.keyframes.is_empty()));
    assert!(DynamicZoomSettings::from_effect(effect).is_ok());
}

#[test]
fn valid_viewport_edits_undo_redo_without_retiming_or_motion_changes() {
    let (mut s, id, index) = fixture();
    let before = s.project.clone();
    let original = s.active_sequence().unwrap().find_item(id).unwrap().1.clone();
    s.execute("effects.setParam", json!({"clip":id.0,"effect":index,"param":"end_center","value":[0.6,0.4]})).unwrap();
    s.execute("effects.setParam", json!({"clip":id.0,"effect":index,"param":"easing","value":"Ease In-Out"})).unwrap();
    let item = s.active_sequence().unwrap().find_item(id).unwrap().1;
    assert_eq!(
        (item.start, item.duration, item.source_in, item.speed, item.reverse),
        (original.start, original.duration, original.source_in, original.speed, original.reverse)
    );
    assert_eq!(item.effect("motion"), original.effect("motion"));
    let after = s.project.clone();
    s.undo();
    s.undo();
    assert_eq!(*s.project, *before);
    s.redo();
    s.redo();
    assert_eq!(*s.project, *after);
}
