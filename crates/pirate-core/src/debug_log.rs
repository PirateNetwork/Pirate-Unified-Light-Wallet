//! Simple process-wide debug log writer.
//!
//! We emit JSONL to a single `debug.log` file for field diagnostics. Writes are
//! synchronized and each log entry is written as a single append to prevent
//! interleaving/corruption under concurrency.

use directories::ProjectDirs;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const DEFAULT_DEBUG_LOG_MAX_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_DEBUG_LOG_BACKUP_COUNT: usize = 2;
const MAX_DEBUG_LOG_BACKUP_COUNT: usize = 10;

static DEBUG_LOG_PATH: Lazy<PathBuf> = Lazy::new(resolve_debug_log_path);
static DEBUG_LOG_FILE: Lazy<Mutex<Option<File>>> = Lazy::new(|| Mutex::new(None));
static DEBUG_LOG_MAX_BYTES: Lazy<u64> = Lazy::new(resolve_max_debug_log_bytes);
static DEBUG_LOG_BACKUP_COUNT: Lazy<usize> = Lazy::new(resolve_debug_log_backup_count);
static DEBUG_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

static PRIVATE_JSON_FIELD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)("(?:mnemonic|seed|passphrase|password|pin|panic_pin|duress_passphrase|spending_key|sapling_key|orchard_key|sapling_viewing_key|orchard_viewing_key|viewing_key|extsk|ovk|ivk|fvk|private_key|secret|panic|panic_location|backtrace|stack)"\s*:\s*)("[^"\\]*(?:\\.[^"\\]*)*"|[^,}\n]+)"#,
    )
    .expect("valid private-field redaction regex")
});

static CORRELATING_JSON_FIELD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)("(?:wallet_id|account_id|key_id|address|addresses|z_addresses|address_id|txid|txids|spent_txid|pending_txid|recent_txids|last_seen_txids|txid_prefix|nullifier|nullifiers|nf|cmu|cmx|cmx_prefix|commitment|memo|memo_hex|path|cwd|db_path|endpoint|url|host|server|server_name|tls_server_name|tls_pin)"\s*:\s*)("[^"\\]*(?:\\.[^"\\]*)*"|[^,}\n]+)"#,
    )
    .expect("valid correlating-field redaction regex")
});

static RAW_SECRET_ASSIGNMENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)\b(mnemonic|seed|passphrase|password|pin|spending[_ -]?key|viewing[_ -]?key|private[_ -]?key|sapling[_ -]?key|orchard[_ -]?key)\b\s*[:=]\s*("[^"]*"|'[^']*'|\S+)"#,
    )
    .expect("valid secret assignment redaction regex")
});

static PIRATE_ADDRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\b(?:zs1|ztestsapling1|zregtestsapling1|pirate1|pirate-test1|pirate-regtest1)[a-z0-9_-]{20,}\b"#)
        .expect("valid address redaction regex")
});

static LONG_HEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\b(?:0x)?[0-9a-f]{64,}\b"#).expect("valid hex redaction regex"));

static WINDOWS_PATH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"[A-Za-z]:\\[^\s",}]+"#).expect("valid windows path redaction regex")
});

static UNIX_USER_PATH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"/(?:Users|home|var|private|tmp|data|storage)/[^\s",}]+"#)
        .expect("valid unix path redaction regex")
});

fn resolve_debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        ProjectDirs::from("com", "Pirate", "PirateWallet")
            .map(|dirs| dirs.data_local_dir().join("logs").join("debug.log"))
            .unwrap_or_else(|| {
                env::current_dir()
                    .map(|dir| dir.join(".cursor").join("debug.log"))
                    .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
            })
    };

    path
}

fn resolve_max_debug_log_bytes() -> u64 {
    env::var("PIRATE_DEBUG_LOG_MAX_BYTES")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|bytes| *bytes > 0)
        .unwrap_or(DEFAULT_DEBUG_LOG_MAX_BYTES)
}

fn resolve_debug_log_backup_count() -> usize {
    env::var("PIRATE_DEBUG_LOG_BACKUPS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .map(|count| count.min(MAX_DEBUG_LOG_BACKUP_COUNT))
        .unwrap_or(DEFAULT_DEBUG_LOG_BACKUP_COUNT)
}

fn backup_log_path(path: &Path, index: usize) -> PathBuf {
    let mut raw = OsString::from(path.as_os_str());
    raw.push(format!(".{index}"));
    PathBuf::from(raw)
}

fn open_debug_log_file() -> Option<File> {
    if let Some(parent) = DEBUG_LOG_PATH.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*DEBUG_LOG_PATH)
        .ok()
}

fn redact_json_field(text: String, regex: &Regex, replacement: &str) -> String {
    regex
        .replace_all(&text, |caps: &Captures<'_>| {
            format!(r#"{}"{}""#, &caps[1], replacement)
        })
        .into_owned()
}

fn redact_secret_assignment(text: String) -> String {
    RAW_SECRET_ASSIGNMENT
        .replace_all(&text, |caps: &Captures<'_>| {
            format!("{}=[REDACTED_SECRET]", &caps[1])
        })
        .into_owned()
}

fn redact_log_text(text: &str) -> String {
    let text = redact_json_field(text.to_string(), &PRIVATE_JSON_FIELD, "[REDACTED_SECRET]");
    let text = redact_json_field(text, &CORRELATING_JSON_FIELD, "[REDACTED]");
    let text = redact_secret_assignment(text);
    let text = PIRATE_ADDRESS
        .replace_all(&text, "[REDACTED_ADDRESS]")
        .into_owned();
    let text = LONG_HEX.replace_all(&text, "[REDACTED_HEX]").into_owned();
    let text = WINDOWS_PATH
        .replace_all(&text, "[REDACTED_PATH]")
        .into_owned();
    UNIX_USER_PATH
        .replace_all(&text, "[REDACTED_PATH]")
        .into_owned()
}

fn remove_log_files(path: &Path) {
    let _ = fs::remove_file(path);
    for index in 1..=MAX_DEBUG_LOG_BACKUP_COUNT {
        let _ = fs::remove_file(backup_log_path(path, index));
    }
}

fn current_log_len(guard: &Option<File>) -> u64 {
    guard
        .as_ref()
        .and_then(|file| file.metadata().ok().map(|meta| meta.len()))
        .or_else(|| fs::metadata(&*DEBUG_LOG_PATH).ok().map(|meta| meta.len()))
        .unwrap_or(0)
}

fn rotate_locked(guard: &mut Option<File>) {
    *guard = None;

    // Drop oversized logs instead of archiving giant files.
    if fs::metadata(&*DEBUG_LOG_PATH)
        .ok()
        .map(|meta| meta.len() > *DEBUG_LOG_MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(&*DEBUG_LOG_PATH);
        return;
    }

    let backups = *DEBUG_LOG_BACKUP_COUNT;
    if backups == 0 {
        let _ = fs::remove_file(&*DEBUG_LOG_PATH);
        return;
    }

    for index in (1..=backups).rev() {
        let src = if index == 1 {
            (*DEBUG_LOG_PATH).clone()
        } else {
            backup_log_path(&DEBUG_LOG_PATH, index - 1)
        };
        let dst = backup_log_path(&DEBUG_LOG_PATH, index);
        let _ = fs::remove_file(&dst);
        let _ = fs::rename(&src, &dst);
    }
}

fn ensure_file_open(guard: &mut Option<File>) -> bool {
    if guard.is_none() {
        *guard = open_debug_log_file();
    }
    guard.is_some()
}

fn ensure_capacity_for_write(guard: &mut Option<File>, upcoming_bytes: usize) -> bool {
    if !ensure_file_open(guard) {
        return false;
    }

    let current = current_log_len(guard);
    if current.saturating_add(upcoming_bytes as u64) > *DEBUG_LOG_MAX_BYTES {
        rotate_locked(guard);
        if !ensure_file_open(guard) {
            return false;
        }
    }

    true
}

fn enforce_post_write_cap(guard: &mut Option<File>) {
    if current_log_len(guard) > *DEBUG_LOG_MAX_BYTES {
        rotate_locked(guard);
        let _ = ensure_file_open(guard);
    }
}

/// Returns the resolved debug log path.
pub fn debug_log_path() -> PathBuf {
    (*DEBUG_LOG_PATH).clone()
}

/// Enables or disables field diagnostics logging for the current process.
///
/// Logging is disabled by default. Disabling closes the active file handle and
/// removes any existing `debug.log` files so users do not retain stale
/// diagnostics unless they explicitly opt in.
pub fn set_enabled(enabled: bool) {
    DEBUG_LOG_ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        clear_logs();
    }
}

/// Returns whether debug logging is enabled for the current process.
pub fn is_enabled() -> bool {
    DEBUG_LOG_ENABLED.load(Ordering::Relaxed)
}

/// Closes the active debug log handle and removes the active log plus backups.
pub fn clear_logs() {
    let mut guard = DEBUG_LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
    remove_log_files(&DEBUG_LOG_PATH);
}

/// Appends a single line to `debug.log`.
///
/// The caller should pass a complete line without a trailing newline. This
/// function will add one.
pub fn append_line(line: &str) {
    if !is_enabled() {
        return;
    }

    let mut guard = DEBUG_LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let line = redact_log_text(line);

    // Write as a single buffer so entries can't interleave.
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');

    if !ensure_capacity_for_write(&mut guard, buf.len()) {
        return;
    }

    let failed = guard
        .as_mut()
        .and_then(|file| file.write_all(&buf).err())
        .is_some();

    // If the write failed (file moved/locked/etc), drop the handle so we try to
    // reopen on the next log event.
    if failed {
        *guard = None;
        return;
    }

    enforce_post_write_cap(&mut guard);
}

/// Convenience wrapper that formats the line before appending.
pub fn append_line_fmt(args: std::fmt::Arguments<'_>) {
    append_line(&args.to_string());
}

/// Runs `f` with the debug log file locked for the full duration of the call.
///
/// This is mainly useful when you need multiple writes to be serialized as one
/// critical section (for example `writeln!` that may call `write()` multiple
/// times internally).
pub fn with_locked_file<F>(f: F)
where
    F: FnOnce(&mut dyn Write),
{
    if !is_enabled() {
        return;
    }

    let mut guard = DEBUG_LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut pending = Vec::new();
    f(&mut pending);
    if pending.is_empty() {
        return;
    }

    let pending = String::from_utf8_lossy(&pending);
    let sanitized = redact_log_text(&pending);
    let buf = sanitized.as_bytes();

    if !ensure_capacity_for_write(&mut guard, buf.len()) {
        return;
    }

    let failed = guard
        .as_mut()
        .and_then(|file| file.write_all(buf).err())
        .is_some();

    if failed {
        *guard = None;
        return;
    }

    enforce_post_write_cap(&mut guard);
}

#[cfg(test)]
mod tests {
    use super::redact_log_text;

    #[test]
    fn redacts_private_and_correlating_fields() {
        let raw = r#"{"mnemonic":"abandon abandon","wallet_id":"wallet-1","key_id":42,"address":"zs1qqqqqqqqqqqqqqqqqqqqqqqqqqqq","txid":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","panic":"seed leaked","path":"C:\\Users\\alice\\wallet.db"}"#;
        let redacted = redact_log_text(raw);

        assert!(!redacted.contains("abandon abandon"));
        assert!(!redacted.contains("wallet-1"));
        assert!(!redacted.contains("\"key_id\":42"));
        assert!(!redacted.contains("zs1qqqq"));
        assert!(!redacted.contains("0123456789abcdef"));
        assert!(!redacted.contains("seed leaked"));
        assert!(!redacted.contains("alice"));
        assert!(redacted.contains("[REDACTED_SECRET]"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
