//! Local, opt-in developer action log. No typed text, passwords, or query payloads.
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct DevLogger {
    pub control_origin: bool,
    file: Option<File>,
    path: Option<PathBuf>,
    events: u64,
}
impl DevLogger {
    pub fn enabled(&self) -> bool {
        self.file.is_some()
    }
    pub fn status(&self) -> Value {
        json!({"enabled":self.enabled(),"path":self.path,"events":self.events,"typed_text":false})
    }
    fn set(&mut self, enabled: bool, path: Option<PathBuf>) -> io::Result<()> {
        if let Some(path) = path {
            self.path = Some(path);
        }
        if !enabled {
            self.file = None;
            return Ok(());
        }
        let Some(path) = self.path.as_ref() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "choose a log path first"));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.file = Some(OpenOptions::new().create(true).append(true).open(path)?);
        Ok(())
    }
    pub(crate) fn record(&mut self, command: &str, label: &str, params: &Value, success: bool, window: &str) -> io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        // Bound total storage and stop cleanly instead of risking the user's disk/project.
        if file.metadata()?.len() > 128 * 1024 * 1024 {
            return Err(io::Error::other("Dev log reached 128 MB; choose a new log file"));
        }
        let record = json!({"timestamp_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "window":window,"action":label,"command":command,"params":redact(params,0),"success":success,
            "origin":if self.control_origin {"control/MCP"} else {"user interface"}});
        serde_json::to_writer(&mut *file, &record)?;
        file.write_all(b"\n")?;
        file.flush()?;
        self.events = self.events.saturating_add(1);
        Ok(())
    }
    pub(crate) fn disable(&mut self) {
        self.file = None;
    }
}
fn redact(value: &Value, depth: usize) -> Value {
    if depth > 8 {
        return Value::String("[depth limited]".into());
    }
    match value {
        Value::Object(map) => {
            let text_parameter = map.get("param").and_then(Value::as_str).is_some_and(|p| p.to_ascii_lowercase().contains("text"));
            Value::Object(
                map.iter()
                    .take(64)
                    .map(|(k, v)| {
                        let key = k.to_ascii_lowercase();
                        let sensitive = ["password", "token", "secret", "text", "caption", "script", "body"].iter().any(|s| key.contains(s))
                            || (text_parameter && key == "value");
                        (k.clone(), if sensitive { Value::String("[redacted]".into()) } else { redact(v, depth + 1) })
                    })
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().take(64).map(|v| redact(v, depth + 1)).collect()),
        Value::String(s) => Value::String(s.chars().take(512).collect()),
        _ => value.clone(),
    }
}
pub(crate) fn set_logging(s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).unwrap_or(!s.dev_log.enabled());
    let requested = p.get("path").and_then(Value::as_str).map(PathBuf::from);
    let path = requested.or_else(|| {
        if s.dev_log.path.is_none() {
            s.path
                .as_deref()
                .and_then(|p| Path::new(p).parent())
                .or_else(|| s.prefs_path.as_deref().and_then(Path::parent))
                .map(|base| base.join("EditLogs").join("filmcraft_actions.jsonl"))
        } else {
            None
        }
    });
    s.dev_log.set(enabled, path).map_err(|e| EngineError::Other(format!("Dev logging: {e}")))?;
    Ok(s.dev_log.status())
}
