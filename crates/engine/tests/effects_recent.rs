use filmcraft_engine::{Session, autosave::Preferences};
use filmcraft_project::EffectKind;
use serde_json::{Value, json};

fn demo() -> (Session, u64) {
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let clip = session.active_sequence().unwrap().video_tracks[0].items[0].id.0;
    (session, clip)
}

fn recent(session: &mut Session) -> Value {
    session.execute("prefs.get", json!({"key": "recentEffects"})).unwrap()
}

#[test]
fn recent_effects_default_is_empty_for_existing_preferences() {
    let mut session = Session::default();
    assert_eq!(recent(&mut session), json!([]));
    let older = serde_json::from_value::<Preferences>(json!({"version": 2})).unwrap();
    assert_eq!(serde_json::to_value(older).unwrap()["recentEffects"], json!([]));
}

#[test]
fn recent_effects_records_successful_apply_by_canonical_id_and_survives_undo() {
    let (mut session, clip) = demo();
    for effect in ["Gaussian Blur", "invert", "gaussian_blur"] {
        session.execute("effects.apply", json!({"clips": [clip], "effect": effect})).unwrap();
    }
    assert_eq!(recent(&mut session), json!(["gaussian_blur", "invert"]));
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(recent(&mut session), json!(["gaussian_blur", "invert"]));
    session.execute("edit.redo", json!({})).unwrap();
    assert_eq!(recent(&mut session), json!(["gaussian_blur", "invert"]));
    assert!(session.execute("effects.apply", json!({"effect": "does_not_exist"})).is_err());
    assert!(session.execute("effects.apply", json!({"effect": "gaussian_blur"})).is_err());
    assert!(session.execute("effects.apply", json!({"clips": [999999], "effect": "crop"})).is_err());
    assert_eq!(recent(&mut session), json!(["gaussian_blur", "invert"]));
}

#[test]
fn recent_effects_caps_at_ten_and_round_trips_in_preferences() {
    let (mut session, clip) = demo();
    let effects: Vec<_> = filmcraft_project::effect_defs()
        .iter()
        .filter(|d| d.kind == EffectKind::Video && !d.intrinsic && !filmcraft_project::graphic::is_layer_id(d.id))
        .take(12)
        .map(|d| d.id)
        .collect();
    assert_eq!(effects.len(), 12);
    for effect in &effects {
        session.execute("effects.apply", json!({"clips": [clip], "effect": effect})).unwrap();
    }
    let expected: Vec<_> = effects.iter().rev().take(10).copied().collect();
    assert_eq!(recent(&mut session), json!(expected));
    let loaded: Preferences = serde_json::from_value(serde_json::to_value(&session.prefs).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(loaded).unwrap()["recentEffects"], json!(expected));
}

#[test]
fn malformed_recent_effects_does_not_reset_other_preferences() {
    for history in [json!("invalid"), json!(["gaussian_blur", 42, "invert", "gaussian_blur"])] {
        let prefs = Preferences::from_value(json!({"general": {"interfaceLanguage": "fr"}, "recentEffects": history}));
        assert_eq!(prefs.general.interface_language, "fr");
        let saved = serde_json::to_value(prefs).unwrap();
        assert!(saved["recentEffects"].as_array().unwrap().iter().all(|id| id.is_string()));
    }
}

#[test]
fn history_saves_to_disk_and_a_failed_write_does_not_reverse_the_effect() {
    let (mut session, clip) = demo();
    let dir = std::env::temp_dir().join(format!("fc-recent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("preferences.json");
    session.prefs_path = Some(path.clone());
    session.execute("effects.apply", json!({"clips": [clip], "effect": "gaussian_blur"})).unwrap();
    assert_eq!(Preferences::load(&path).recent_effects, vec!["gaussian_blur"]);
    let blocked = dir.join("directory-not-file");
    std::fs::create_dir_all(&blocked).unwrap();
    session.prefs_path = Some(blocked.clone());
    session.execute("effects.apply", json!({"clips": [clip], "effect": "invert"})).unwrap();
    assert_eq!(recent(&mut session), json!(["invert", "gaussian_blur"]));
    assert!(session.active_sequence().unwrap().find_item(filmcraft_project::ClipId(clip)).unwrap().1.effects.iter().any(|effect| effect.effect == "invert"));
    assert!(
        session
            .events
            .iter()
            .any(|event| matches!(event, filmcraft_engine::Event::Toast { message, error: true } if message.contains("recent effects could not be saved")))
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(blocked).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
