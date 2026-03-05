//! Simple process-wide debug log writer.
//!
//! We emit JSONL to a single `debug.log` file for field diagnostics. Writes are
//! synchronized and each log entry is written as a single append to prevent
//! interleaving/corruption under concurrency.

use directories::ProjectDirs;
use once_cell::sync::Lazy;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const DEFAULT_DEBUG_LOG_MAX_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_DEBUG_LOG_BACKUP_COUNT: usize = 2;
const MAX_DEBUG_LOG_BACKUP_COUNT: usize = 10;

static DEBUG_LOG_PATH: Lazy<PathBuf> = Lazy::new(resolve_debug_log_path);
static DEBUG_LOG_FILE: Lazy<Mutex<Option<File>>> = Lazy::new(|| Mutex::new(None));
static DEBUG_LOG_MAX_BYTES: Lazy<u64> = Lazy::new(resolve_max_debug_log_bytes);
static DEBUG_LOG_BACKUP_COUNT: Lazy<usize> = Lazy::new(resolve_debug_log_backup_count);

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

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
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
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*DEBUG_LOG_PATH)
        .ok()
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

/// Appends a single line to `debug.log`.
///
/// The caller should pass a complete line without a trailing newline. This
/// function will add one.
pub fn append_line(line: &str) {
    let mut guard = DEBUG_LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

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
    F: FnOnce(&mut File),
{
    let mut guard = DEBUG_LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if !ensure_capacity_for_write(&mut guard, 0) {
        return;
    }

    if let Some(file) = guard.as_mut() {
        f(file);
    }

    enforce_post_write_cap(&mut guard);
}
