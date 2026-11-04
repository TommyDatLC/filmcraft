use filmcraft_engine::Session;
use serde_json::json;

#[test]
fn remove_unused_keeps_the_external_alpha_media_of_delta_key() {
    use filmcraft_project::{ItemKind, MediaRef};
    let mut session = Session::default();
    session.execute("file.openDemoProject", json!({})).unwrap();
    let matte = session.project.items.values().find(|p| p.name == "Bars and Tone").unwrap().id;
    let project = std::sync::Arc::make_mut(&mut session.project);
    if let ItemKind::Media(media) = &mut project.items.get_mut(&matte).unwrap().kind {
        media.media = MediaRef::File { path: "E:/local_alpha.mp4".into() };
    } else {
        panic!("demo media missing");
    }
    let clip = session.active_sequence().unwrap().video_tracks[0].items[0].id.0;
    session.execute("effects.apply", json!({"clips":[clip],"effect":"delta_key"})).unwrap();
    session.execute("effects.setParam", json!({"clip":clip,"effect":"delta_key","param":"alpha_source","value":"E:/local_alpha.mp4"})).unwrap();
    session.execute("edit.removeUnused", json!({})).unwrap();
    assert!(session.project.item(matte).is_some(), "alpha dependencies must survive Remove Unused");
}

#[test]
fn dev_logging_toggle_records_commands_and_redacts_text() {
    let root = std::env::temp_dir().join(format!("filmcraft-devlog-test-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("events.jsonl");
    let mut session = Session::default();
    let status = session.execute("dev.logging.status", json!({})).unwrap();
    assert_eq!(status["enabled"], false);
    session.execute("dev.logging.set", json!({"enabled":true,"path":path})).unwrap();
    session.execute("file.newProject", json!({"name":"Test"})).unwrap();
    // A harmless ignored extra field proves logging never stores typed text.
    session.execute("file.newProject", json!({"name":"Test 2","text":"private narration"})).unwrap();
    session.execute("dev.logging.toggle", json!({})).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(before.contains("file.newProject"));
    assert!(!before.contains("private narration"));
    session.execute("file.newProject", json!({"name":"Test 3"})).unwrap();
    assert_eq!(before, std::fs::read_to_string(path).unwrap());
}
