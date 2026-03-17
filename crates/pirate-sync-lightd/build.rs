use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=APP_VERSION");
    println!("cargo:rerun-if-changed=../../app/pubspec.yaml");

    let app_version = env::var("APP_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(resolve_app_version)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=APP_VERSION={app_version}");
}

fn resolve_app_version() -> Option<String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let pubspec = manifest_dir.join("../../app/pubspec.yaml");
    let contents = fs::read_to_string(pubspec).ok()?;
    let raw = contents.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("version:")
            .map(|value| value.trim().to_string())
    })?;
    let semver = raw.split('+').next()?.trim();
    if semver.is_empty() {
        None
    } else {
        Some(semver.to_string())
    }
}
