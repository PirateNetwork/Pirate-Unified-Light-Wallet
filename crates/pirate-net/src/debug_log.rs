use std::time::{SystemTime, UNIX_EPOCH};

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
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("{:08x}", ts);
    let data = escape_log_value(data);

    // Serialize writes through the single process-wide debug log writer so
    // JSON lines can't interleave across crates/threads.
    pirate_core::debug_log::append_line(&format!(
        r#"{{"id":"net_{}","timestamp":{},"location":"{}","message":"{}","data":"{}"}}"#,
        id, ts, location, message, data
    ));
}
