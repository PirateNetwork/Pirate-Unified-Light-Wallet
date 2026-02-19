use chrono::{TimeZone, Utc};
use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=BUILD_DATE");
    println!("cargo:rerun-if-env-changed=RUSTC_VERSION");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let git_commit = env::var("GIT_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(resolve_git_commit)
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_COMMIT={git_commit}");

    let build_date = env::var("BUILD_DATE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(resolve_build_date)
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_DATE={build_date}");

    let rustc_version = env::var("RUSTC_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(resolve_rustc_version)
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=RUSTC_VERSION={rustc_version}");

    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_TARGET={target}");
}

fn resolve_git_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_build_date() -> Option<String> {
    if let Ok(source_date_epoch) = env::var("SOURCE_DATE_EPOCH") {
        if let Some(formatted) = format_epoch_rfc3339(&source_date_epoch) {
            return Some(formatted);
        }
    }

    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let epoch = String::from_utf8(output.stdout).ok()?;
    format_epoch_rfc3339(epoch.trim())
}

fn resolve_rustc_version() -> Option<String> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(rustc).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn format_epoch_rfc3339(epoch: &str) -> Option<String> {
    let epoch = epoch.parse::<i64>().ok()?;
    let timestamp = Utc.timestamp_opt(epoch, 0).single()?;
    Some(timestamp.to_rfc3339())
}
