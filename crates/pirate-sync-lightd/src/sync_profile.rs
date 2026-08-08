//! Local-only sync profile selection for coarse device classes.

use crate::sync::SyncConfig;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};
use tokio::sync::RwLock as TokioRwLock;

use crate::progress::{SyncProgress, SyncStage};

const MB: u64 = 1_000_000;
const PROFILE_CALIBRATION_BUDGET: Duration = Duration::from_millis(180);
const MAX_CRASH_DOWNGRADE_STEPS: u8 = 2;
const SUCCESSFUL_SYNCS_TO_RECOVER: u8 = 2;
const SYNC_PROFILE_STATE_VERSION: u8 = 2;
const SYNC_PROFILE_STATE_FILE: &str = "sync_profile_state.json";
const SYNC_PROFILE_STATE_PATH_ENV: &str = "PIRATE_SYNC_PROFILE_STATE_PATH";

static ACTIVE_SYNC_PROFILE_SESSIONS: AtomicUsize = AtomicUsize::new(0);

/// Coarse device class used to choose generic sync performance settings.
///
/// These names are intentionally broad. The wallet never sends the selected
/// profile or the underlying device measurements to lightwalletd.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncDeviceClass {
    /// Low-resource Android/iOS device.
    MobileLow,
    /// Typical modern Android/iOS device.
    MobileBalanced,
    /// High-resource Android/iOS device.
    MobileHigh,
    /// Low-resource laptop or desktop.
    DesktopLow,
    /// Typical laptop or desktop.
    DesktopBalanced,
    /// High-resource desktop or workstation.
    DesktopHigh,
}

/// Sync workload shape used when converting a profile into [`SyncConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncWorkload {
    /// Foreground compact sync.
    Compact,
    /// More expensive deep scan.
    Deep,
    /// Explicit rescan from a selected height.
    Rescan,
}

/// Local device snapshot used only for profile selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncDeviceSnapshot {
    /// Whether the binary was built for Android or iOS.
    pub is_mobile: bool,
    /// Logical CPU count reported by the OS.
    pub logical_cpus: usize,
    /// Physical CPU count when available.
    pub physical_cpus: Option<usize>,
    /// Total RAM in decimal megabytes.
    pub total_memory_mb: u64,
    /// Currently available RAM in decimal megabytes.
    pub available_memory_mb: u64,
    /// Available storage near the app/current directory in decimal megabytes.
    pub available_storage_mb: Option<u64>,
    /// Tiny local CPU calibration score. Higher is faster.
    pub cpu_score: u64,
}

/// Selected profile and config for a sync session.
#[derive(Clone, Debug)]
pub struct SyncProfileSelection {
    /// Effective profile after applying any local crash downgrade.
    pub profile: SyncDeviceClass,
    /// Sync configuration derived from the effective profile.
    pub config: SyncConfig,
    /// Whether this selection downgraded because the last process died mid-sync.
    pub crash_downgraded: bool,
    /// Number of downgrade steps currently applied by the local crash guard.
    pub downgrade_steps: u8,
}

#[derive(Clone, Copy, Debug)]
struct SyncProfileSpec {
    max_parallel_decrypt: usize,
    max_batch_memory_bytes: Option<u64>,
    target_batch_bytes: u64,
    min_batch_bytes: u64,
    max_batch_bytes: u64,
    prefetch_queue_depth: usize,
    prefetch_queue_max_bytes: u64,
    min_batch_size: u64,
    max_batch_size: u64,
    compact_batch_size: u64,
    deep_batch_size: u64,
    rescan_batch_size: u64,
    sync_state_flush_every_batches: u32,
    sync_state_flush_interval_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SyncProfileState {
    #[serde(default)]
    version: u8,
    in_progress: bool,
    downgrade_steps: u8,
    successful_syncs_after_downgrade: u8,
}

impl Default for SyncProfileState {
    fn default() -> Self {
        Self {
            version: SYNC_PROFILE_STATE_VERSION,
            in_progress: false,
            downgrade_steps: 0,
            successful_syncs_after_downgrade: 0,
        }
    }
}

/// Idempotent handle for one crash-guarded sync profile session.
///
/// Clones refer to the same session, allowing a tip monitor and the owning task
/// to race safely when reporting completion.
#[derive(Clone, Debug)]
pub struct SyncProfileSession {
    finished: Arc<AtomicBool>,
}

impl SyncProfileSession {
    fn new() -> Self {
        Self {
            finished: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Records successful initial catch-up exactly once.
    pub fn record_success(&self) {
        self.finish(true);
    }

    /// Records a clean cancellation or failure exactly once.
    pub fn record_failure(&self) {
        self.finish(false);
    }

    /// Returns whether this session has already been recorded.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    fn finish(&self, success: bool) {
        if self
            .finished
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            finish_sync_profile_session(success);
        }
    }
}

impl SyncDeviceClass {
    /// Stable human-readable profile identifier for local logs and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MobileLow => "mobile-low",
            Self::MobileBalanced => "mobile-balanced",
            Self::MobileHigh => "mobile-high",
            Self::DesktopLow => "desktop-low",
            Self::DesktopBalanced => "desktop-balanced",
            Self::DesktopHigh => "desktop-high",
        }
    }

    fn spec(self) -> SyncProfileSpec {
        match self {
            Self::MobileLow => SyncProfileSpec {
                max_parallel_decrypt: 2,
                max_batch_memory_bytes: Some(48 * MB),
                target_batch_bytes: 4 * MB,
                min_batch_bytes: MB,
                max_batch_bytes: 8 * MB,
                prefetch_queue_depth: 1,
                prefetch_queue_max_bytes: 8 * MB,
                min_batch_size: 10,
                max_batch_size: 1_000,
                compact_batch_size: 1_000,
                deep_batch_size: 500,
                rescan_batch_size: 1_000,
                sync_state_flush_every_batches: 2,
                sync_state_flush_interval_ms: 1_000,
            },
            Self::MobileBalanced => SyncProfileSpec {
                max_parallel_decrypt: 4,
                max_batch_memory_bytes: Some(96 * MB),
                target_batch_bytes: 8 * MB,
                min_batch_bytes: MB,
                max_batch_bytes: 16 * MB,
                prefetch_queue_depth: 1,
                prefetch_queue_max_bytes: 16 * MB,
                min_batch_size: 25,
                max_batch_size: 2_000,
                compact_batch_size: 2_000,
                deep_batch_size: 1_000,
                rescan_batch_size: 2_000,
                sync_state_flush_every_batches: 3,
                sync_state_flush_interval_ms: 1_500,
            },
            Self::MobileHigh => SyncProfileSpec {
                max_parallel_decrypt: 6,
                max_batch_memory_bytes: Some(160 * MB),
                target_batch_bytes: 16 * MB,
                min_batch_bytes: 2 * MB,
                max_batch_bytes: 32 * MB,
                prefetch_queue_depth: 1,
                prefetch_queue_max_bytes: 32 * MB,
                min_batch_size: 50,
                max_batch_size: 4_000,
                compact_batch_size: 4_000,
                deep_batch_size: 2_000,
                rescan_batch_size: 4_000,
                sync_state_flush_every_batches: 3,
                sync_state_flush_interval_ms: 1_500,
            },
            Self::DesktopLow => SyncProfileSpec {
                max_parallel_decrypt: 8,
                max_batch_memory_bytes: Some(256 * MB),
                target_batch_bytes: 32 * MB,
                min_batch_bytes: 4 * MB,
                max_batch_bytes: 64 * MB,
                prefetch_queue_depth: 1,
                prefetch_queue_max_bytes: 64 * MB,
                min_batch_size: 50,
                max_batch_size: 4_000,
                compact_batch_size: 4_000,
                deep_batch_size: 2_000,
                rescan_batch_size: 4_000,
                sync_state_flush_every_batches: 2,
                sync_state_flush_interval_ms: 1_500,
            },
            Self::DesktopBalanced => SyncProfileSpec {
                max_parallel_decrypt: 16,
                max_batch_memory_bytes: Some(500 * MB),
                target_batch_bytes: 96 * MB,
                min_batch_bytes: 8 * MB,
                max_batch_bytes: 128 * MB,
                prefetch_queue_depth: 2,
                prefetch_queue_max_bytes: 256 * MB,
                min_batch_size: 100,
                max_batch_size: 6_000,
                compact_batch_size: 6_000,
                deep_batch_size: 3_000,
                rescan_batch_size: 6_000,
                sync_state_flush_every_batches: 4,
                sync_state_flush_interval_ms: 3_000,
            },
            Self::DesktopHigh => SyncProfileSpec {
                max_parallel_decrypt: 24,
                max_batch_memory_bytes: Some(1_000 * MB),
                target_batch_bytes: 192 * MB,
                min_batch_bytes: 16 * MB,
                max_batch_bytes: 256 * MB,
                prefetch_queue_depth: 3,
                prefetch_queue_max_bytes: 512 * MB,
                min_batch_size: 100,
                max_batch_size: 16_000,
                compact_batch_size: 16_000,
                deep_batch_size: 8_000,
                rescan_batch_size: 16_000,
                sync_state_flush_every_batches: 6,
                sync_state_flush_interval_ms: 5_000,
            },
        }
    }

    fn downgrade(self, steps: u8) -> Self {
        let mut profile = self;
        for _ in 0..steps.min(MAX_CRASH_DOWNGRADE_STEPS) {
            profile = match profile {
                Self::MobileHigh => Self::MobileBalanced,
                Self::MobileBalanced | Self::MobileLow => Self::MobileLow,
                Self::DesktopHigh => Self::DesktopBalanced,
                Self::DesktopBalanced | Self::DesktopLow => Self::DesktopLow,
            };
        }
        profile
    }
}

impl SyncDeviceSnapshot {
    /// Select a coarse profile from this snapshot.
    pub fn profile(self) -> SyncDeviceClass {
        if self.is_mobile {
            classify_mobile(self)
        } else {
            classify_desktop(self)
        }
    }
}

/// Builds a sync config for the locally detected device profile.
pub fn sync_config_for_detected_device(workload: SyncWorkload) -> SyncConfig {
    let profile = detect_sync_profile();
    sync_config_for_profile(profile, workload)
}

/// Starts a local sync profile session and returns its effective config.
///
/// This records a tiny local crash guard marker before sync starts. If the app
/// process dies before the marker is cleared, the next session downgrades one
/// coarse profile step. No device measurements or profile data are sent to the
/// lightwalletd server.
pub fn begin_sync_profile_session(workload: SyncWorkload) -> SyncProfileSelection {
    let base_profile = detect_sync_profile();
    let state_path = sync_profile_state_path();
    let mut state = load_sync_profile_state(&state_path);
    let active_sessions = ACTIVE_SYNC_PROFILE_SESSIONS.load(Ordering::SeqCst);
    let crash_downgraded = state.in_progress && active_sessions == 0;

    if crash_downgraded {
        state.downgrade_steps = state
            .downgrade_steps
            .saturating_add(1)
            .min(MAX_CRASH_DOWNGRADE_STEPS);
        state.successful_syncs_after_downgrade = 0;
    }

    let profile = base_profile.downgrade(state.downgrade_steps);
    state.in_progress = true;
    state.downgrade_steps = state.downgrade_steps.min(MAX_CRASH_DOWNGRADE_STEPS);
    state.successful_syncs_after_downgrade = state
        .successful_syncs_after_downgrade
        .min(SUCCESSFUL_SYNCS_TO_RECOVER);
    ACTIVE_SYNC_PROFILE_SESSIONS.fetch_add(1, Ordering::SeqCst);
    save_sync_profile_state(&state_path, &state);

    if crash_downgraded {
        tracing::warn!(
            "sync profile crash guard downgraded {} to {} after an unfinished sync session",
            base_profile.as_str(),
            profile.as_str()
        );
    } else if state.downgrade_steps > 0 {
        tracing::info!(
            "sync profile crash guard is keeping {} at {} until recovery succeeds",
            base_profile.as_str(),
            profile.as_str()
        );
    }

    SyncProfileSelection {
        profile,
        config: sync_config_for_profile(profile, workload),
        crash_downgraded,
        downgrade_steps: state.downgrade_steps,
    }
}

/// Starts a sync profile session with an idempotent completion handle.
pub fn begin_guarded_sync_profile_session(
    workload: SyncWorkload,
) -> (SyncProfileSelection, SyncProfileSession) {
    (
        begin_sync_profile_session(workload),
        SyncProfileSession::new(),
    )
}

/// Clears a guarded session once its initial catch-up is fully verified.
///
/// Follow-tip synchronization can remain alive indefinitely after this point;
/// it must not leave a crash marker that penalizes the next app launch.
pub async fn monitor_sync_profile_initial_tip(
    session: SyncProfileSession,
    progress: Arc<TokioRwLock<SyncProgress>>,
) {
    const POLL_INTERVAL: Duration = Duration::from_millis(200);

    loop {
        if session.is_finished() {
            return;
        }

        let reached_verified_tip = {
            let progress = progress.read().await;
            progress.target_height() > 0
                && progress.current_height() >= progress.target_height()
                && matches!(progress.stage(), SyncStage::Verify | SyncStage::Complete)
        };
        if reached_verified_tip {
            session.record_success();
            return;
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Marks the current sync profile session as successfully completed.
///
/// After a small streak of successful syncs, the local crash guard relaxes one
/// downgrade step so the device can return to its faster detected profile.
pub fn record_sync_profile_success() {
    finish_sync_profile_session(true);
}

/// Marks the current sync profile session as cleanly stopped or failed.
///
/// Graceful failures clear the in-progress marker but do not trigger a crash
/// downgrade. This avoids punishing normal cancellations, network failures, or
/// server errors.
pub fn record_sync_profile_failure() {
    finish_sync_profile_session(false);
}

/// Builds a sync config for a specific coarse profile.
pub fn sync_config_for_profile(profile: SyncDeviceClass, workload: SyncWorkload) -> SyncConfig {
    let spec = profile.spec();
    SyncConfig {
        checkpoint_interval: 10_000,
        batch_size: match workload {
            SyncWorkload::Compact => spec.compact_batch_size,
            SyncWorkload::Deep => spec.deep_batch_size,
            SyncWorkload::Rescan => spec.rescan_batch_size,
        },
        min_batch_size: spec.min_batch_size,
        max_batch_size: spec.max_batch_size,
        use_server_batch_recommendations: true,
        mini_checkpoint_every: 5,
        mini_checkpoint_max_block_gap: 20_000,
        max_parallel_decrypt: spec.max_parallel_decrypt,
        lazy_memo_decode: true,
        defer_full_tx_fetch: true,
        target_batch_bytes: spec.target_batch_bytes,
        min_batch_bytes: spec.min_batch_bytes,
        max_batch_bytes: spec.max_batch_bytes,
        heavy_block_threshold_bytes: 500_000,
        max_batch_memory_bytes: spec.max_batch_memory_bytes,
        sync_state_flush_every_batches: spec.sync_state_flush_every_batches,
        sync_state_flush_interval_ms: spec.sync_state_flush_interval_ms,
        prefetch_queue_depth: spec.prefetch_queue_depth,
        prefetch_queue_max_bytes: spec.prefetch_queue_max_bytes,
        one_batch_ahead_decryption: true,
        stage_aware_cpu_scheduling: false,
    }
}

/// Detects the local coarse sync profile.
pub fn detect_sync_profile() -> SyncDeviceClass {
    if let Some(profile) = override_profile_from_env() {
        return profile;
    }
    detect_device_snapshot().profile()
}

/// Captures local-only device data for profile selection.
pub fn detect_device_snapshot() -> SyncDeviceSnapshot {
    let mut system = System::new();
    system.refresh_memory();

    let total_memory_mb = system.total_memory() / MB;
    let available_memory_mb = system.available_memory() / MB;
    let logical_cpus = num_cpus::get().max(1);
    let physical_cpus = system.physical_core_count();
    let available_storage_mb = available_storage_near_app().map(|bytes| bytes / MB);
    let cpu_score = quick_cpu_score(PROFILE_CALIBRATION_BUDGET);

    SyncDeviceSnapshot {
        is_mobile: cfg!(target_os = "android") || cfg!(target_os = "ios"),
        logical_cpus,
        physical_cpus,
        total_memory_mb,
        available_memory_mb,
        available_storage_mb,
        cpu_score,
    }
}

fn classify_mobile(snapshot: SyncDeviceSnapshot) -> SyncDeviceClass {
    let storage_mb = snapshot.available_storage_mb.unwrap_or(u64::MAX);
    let cores = snapshot.logical_cpus;

    if cores <= 2
        || snapshot.total_memory_mb < 3_000
        || snapshot.available_memory_mb < 750
        || storage_mb < 1_500
        || snapshot.cpu_score < 45
    {
        return SyncDeviceClass::MobileLow;
    }

    if cores >= 8
        && snapshot.total_memory_mb >= 6_000
        && snapshot.available_memory_mb >= 2_000
        && storage_mb >= 8_000
        && snapshot.cpu_score >= 120
    {
        return SyncDeviceClass::MobileHigh;
    }

    SyncDeviceClass::MobileBalanced
}

fn classify_desktop(snapshot: SyncDeviceSnapshot) -> SyncDeviceClass {
    let storage_mb = snapshot.available_storage_mb.unwrap_or(u64::MAX);
    let cores = snapshot.logical_cpus;

    if cores <= 4
        || snapshot.total_memory_mb < 8_000
        || snapshot.available_memory_mb < 2_000
        || storage_mb < 4_000
        || snapshot.cpu_score < 90
    {
        return SyncDeviceClass::DesktopLow;
    }

    if cores >= 12
        && snapshot.total_memory_mb >= 16_000
        && snapshot.available_memory_mb >= 8_000
        && storage_mb >= 20_000
        && snapshot.cpu_score >= 220
    {
        return SyncDeviceClass::DesktopHigh;
    }

    SyncDeviceClass::DesktopBalanced
}

fn override_profile_from_env() -> Option<SyncDeviceClass> {
    let value = std::env::var("PIRATE_SYNC_PROFILE").ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "mobile-low" | "mobile_low" => Some(SyncDeviceClass::MobileLow),
        "mobile-balanced" | "mobile_balanced" => Some(SyncDeviceClass::MobileBalanced),
        "mobile-high" | "mobile_high" => Some(SyncDeviceClass::MobileHigh),
        "desktop-low" | "desktop_low" => Some(SyncDeviceClass::DesktopLow),
        "desktop-balanced" | "desktop_balanced" => Some(SyncDeviceClass::DesktopBalanced),
        "desktop-high" | "desktop_high" => Some(SyncDeviceClass::DesktopHigh),
        _ => None,
    }
}

fn finish_sync_profile_session(success: bool) {
    let state_path = sync_profile_state_path();
    let mut state = load_sync_profile_state(&state_path);
    let remaining_sessions = ACTIVE_SYNC_PROFILE_SESSIONS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |active| {
            Some(active.saturating_sub(1))
        })
        .map(|previous| previous.saturating_sub(1))
        .unwrap_or(0);

    state.in_progress = remaining_sessions > 0;
    state.downgrade_steps = state.downgrade_steps.min(MAX_CRASH_DOWNGRADE_STEPS);

    if success && state.downgrade_steps > 0 {
        state.successful_syncs_after_downgrade =
            state.successful_syncs_after_downgrade.saturating_add(1);
        if state.successful_syncs_after_downgrade >= SUCCESSFUL_SYNCS_TO_RECOVER {
            state.downgrade_steps = state.downgrade_steps.saturating_sub(1);
            state.successful_syncs_after_downgrade = 0;
        }
    } else if !success {
        state.successful_syncs_after_downgrade = 0;
    }

    save_sync_profile_state(&state_path, &state);
}

fn sync_profile_state_path() -> PathBuf {
    if let Ok(path) = std::env::var(SYNC_PROFILE_STATE_PATH_ENV) {
        return PathBuf::from(path);
    }

    ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join(SYNC_PROFILE_STATE_FILE))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|dir| dir.join(SYNC_PROFILE_STATE_FILE))
        })
        .unwrap_or_else(|| PathBuf::from(SYNC_PROFILE_STATE_FILE))
}

fn load_sync_profile_state(path: &Path) -> SyncProfileState {
    let Ok(bytes) = fs::read(path) else {
        return SyncProfileState::default();
    };
    serde_json::from_slice::<SyncProfileState>(&bytes)
        .unwrap_or_default()
        .normalized()
}

fn save_sync_profile_state(path: &Path, state: &SyncProfileState) {
    let state = state.clone().normalized();
    let Ok(bytes) = serde_json::to_vec_pretty(&state) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp_path = path.with_extension("tmp");
    if fs::write(&tmp_path, bytes).is_ok() && fs::rename(&tmp_path, path).is_err() {
        let _ = fs::remove_file(path);
        let _ = fs::rename(tmp_path, path);
    }
}

impl SyncProfileState {
    fn normalized(mut self) -> Self {
        // Version 1 could leave `in_progress` set forever after the wallet had
        // reached tip because follow-tip sync intentionally does not return.
        // Reset that stale penalty once when upgrading to the guarded lifecycle.
        if self.version < SYNC_PROFILE_STATE_VERSION {
            return Self::default();
        }
        self.downgrade_steps = self.downgrade_steps.min(MAX_CRASH_DOWNGRADE_STEPS);
        self.successful_syncs_after_downgrade = self
            .successful_syncs_after_downgrade
            .min(SUCCESSFUL_SYNCS_TO_RECOVER);
        self
    }
}

fn available_storage_near_app() -> Option<u64> {
    let anchor = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())?;
    available_storage_for_path(&anchor)
}

fn available_storage_for_path(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    let canonical_path = path.canonicalize().unwrap_or_else(|_| PathBuf::from(path));
    disks
        .list()
        .iter()
        .filter(|disk| canonical_path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
        .or_else(|| disks.list().iter().map(|disk| disk.available_space()).max())
}

fn quick_cpu_score(budget: Duration) -> u64 {
    let deadline = Instant::now() + budget;
    let mut rounds = 0u64;
    let mut state = 0x9e37_79b9_7f4a_7c15u64;

    while Instant::now() < deadline {
        for _ in 0..10_000 {
            state ^= state.rotate_left(13);
            state = state.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            state ^= state >> 29;
            std::hint::black_box(state);
        }
        rounds = rounds.saturating_add(1);
    }

    rounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvGuard {
        profile: Option<OsString>,
        state_path: Option<OsString>,
    }

    impl EnvGuard {
        fn set(profile: &str, state_path: &Path) -> Self {
            let guard = Self {
                profile: std::env::var_os("PIRATE_SYNC_PROFILE"),
                state_path: std::env::var_os(SYNC_PROFILE_STATE_PATH_ENV),
            };
            std::env::set_var("PIRATE_SYNC_PROFILE", profile);
            std::env::set_var(SYNC_PROFILE_STATE_PATH_ENV, state_path);
            ACTIVE_SYNC_PROFILE_SESSIONS.store(0, Ordering::SeqCst);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.profile {
                std::env::set_var("PIRATE_SYNC_PROFILE", value);
            } else {
                std::env::remove_var("PIRATE_SYNC_PROFILE");
            }
            if let Some(value) = &self.state_path {
                std::env::set_var(SYNC_PROFILE_STATE_PATH_ENV, value);
            } else {
                std::env::remove_var(SYNC_PROFILE_STATE_PATH_ENV);
            }
            ACTIVE_SYNC_PROFILE_SESSIONS.store(0, Ordering::SeqCst);
        }
    }

    #[test]
    fn classifies_low_mobile_conservatively() {
        let snapshot = SyncDeviceSnapshot {
            is_mobile: true,
            logical_cpus: 2,
            physical_cpus: Some(2),
            total_memory_mb: 2_500,
            available_memory_mb: 700,
            available_storage_mb: Some(1_000),
            cpu_score: 20,
        };

        assert_eq!(snapshot.profile(), SyncDeviceClass::MobileLow);
    }

    #[test]
    fn classifies_high_mobile_when_resources_are_comfortable() {
        let snapshot = SyncDeviceSnapshot {
            is_mobile: true,
            logical_cpus: 8,
            physical_cpus: Some(4),
            total_memory_mb: 8_000,
            available_memory_mb: 3_000,
            available_storage_mb: Some(16_000),
            cpu_score: 160,
        };

        assert_eq!(snapshot.profile(), SyncDeviceClass::MobileHigh);
    }

    #[test]
    fn profile_config_is_bucketed_and_bounded() {
        let config =
            sync_config_for_profile(SyncDeviceClass::MobileBalanced, SyncWorkload::Compact);

        assert_eq!(config.batch_size, 2_000);
        assert_eq!(config.max_batch_size, 2_000);
        assert_eq!(config.target_batch_bytes, 8 * MB);
        assert_eq!(config.prefetch_queue_depth, 1);
    }

    #[test]
    fn high_desktop_gets_larger_coarse_bucket() {
        let config = sync_config_for_profile(SyncDeviceClass::DesktopHigh, SyncWorkload::Compact);

        assert_eq!(config.batch_size, 16_000);
        assert_eq!(config.max_batch_size, 16_000);
        assert_eq!(config.target_batch_bytes, 192 * MB);
        assert_eq!(config.prefetch_queue_depth, 3);
    }

    #[tokio::test]
    async fn crash_guard_downgrades_then_recovers_after_verified_tips() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("sync_profile_state.json");
        let _guard = EnvGuard::set("mobile-high", &state_path);

        let first = begin_sync_profile_session(SyncWorkload::Compact);
        assert_eq!(first.profile, SyncDeviceClass::MobileHigh);
        assert!(!first.crash_downgraded);
        assert_eq!(first.downgrade_steps, 0);

        ACTIVE_SYNC_PROFILE_SESSIONS.store(0, Ordering::SeqCst);
        let (second, second_session) = begin_guarded_sync_profile_session(SyncWorkload::Compact);
        assert_eq!(second.profile, SyncDeviceClass::MobileBalanced);
        assert!(second.crash_downgraded);
        assert_eq!(second.downgrade_steps, 1);
        second_session.record_success();
        second_session.record_success();
        assert!(second_session.is_finished());
        let state = load_sync_profile_state(&state_path);
        assert!(!state.in_progress);
        assert_eq!(state.successful_syncs_after_downgrade, 1);

        let (third, third_session) = begin_guarded_sync_profile_session(SyncWorkload::Compact);
        assert_eq!(third.profile, SyncDeviceClass::MobileBalanced);
        assert!(!third.crash_downgraded);
        assert_eq!(third.downgrade_steps, 1);
        let progress = Arc::new(TokioRwLock::new(SyncProgress::new()));
        {
            let progress = progress.read().await;
            progress.set_target(1_000);
            progress.set_current(1_000);
            progress.set_stage(SyncStage::Verify);
        }
        monitor_sync_profile_initial_tip(third_session.clone(), progress).await;
        assert!(third_session.is_finished());

        let recovered = begin_sync_profile_session(SyncWorkload::Compact);
        assert_eq!(recovered.profile, SyncDeviceClass::MobileHigh);
        assert!(!recovered.crash_downgraded);
        assert_eq!(recovered.downgrade_steps, 0);
        record_sync_profile_failure();
    }

    #[test]
    fn legacy_follow_tip_state_does_not_preserve_a_false_downgrade() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("sync_profile_state.json");
        fs::write(
            &state_path,
            br#"{
                "in_progress": true,
                "downgrade_steps": 2,
                "successful_syncs_after_downgrade": 1
            }"#,
        )
        .unwrap();

        let state = load_sync_profile_state(&state_path);
        assert_eq!(state.version, SYNC_PROFILE_STATE_VERSION);
        assert!(!state.in_progress);
        assert_eq!(state.downgrade_steps, 0);
        assert_eq!(state.successful_syncs_after_downgrade, 0);
    }
}
