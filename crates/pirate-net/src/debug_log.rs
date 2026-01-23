use once_cell::sync::Lazy;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        env::current_dir()
            .map(|dir| dir.join(".cursor").join("debug.log"))
            .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

fn escape_log_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn log_debug_event(location: &str, message: &str, data: &str) {
    let _guard = LOG_LOCK.lock().ok();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = format!("{:08x}", ts);
        let data = escape_log_value(data);
        let _ = writeln!(
            file,
            r#"{{"id":"net_{}","timestamp":{},"location":"{}","message":"{}","data":"{}"}}"#,
            id, ts, location, message, data
        );
    }
}
