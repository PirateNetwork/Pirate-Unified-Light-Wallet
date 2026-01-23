//! Sync progress tracking with ETA calculation and performance counters

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sync stage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStage {
    /// Fetching headers
    Headers,
    /// Scanning notes
    Notes,
    /// Building witness tree
    Witness,
    /// Verifying chain
    Verify,
    /// Complete
    Complete,
}

impl SyncStage {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Headers => "Fetching Headers",
            Self::Notes => "Scanning Notes",
            Self::Witness => "Building Witnesses",
            Self::Verify => "Synching Chain",
            Self::Complete => "Synced",
        }
    }
}

/// Performance counters snapshot
#[derive(Debug, Clone, Default)]
pub struct PerfCountersSnapshot {
    /// Total notes decrypted (matched wallet)
    pub notes_decrypted: u64,
    /// Last batch processing time in milliseconds
    pub last_batch_ms: u64,
    /// Average batch processing time in milliseconds
    pub avg_batch_ms: u64,
    /// Total commitments applied to frontier
    pub commitments_applied: u64,
}

/// Sync progress
#[derive(Debug, Clone)]
pub struct SyncProgress {
    inner: Arc<RwLock<ProgressInner>>,
}

#[derive(Debug, Clone)]
struct ProgressInner {
    current_height: u64,
    target_height: u64,
    start_height: u64,
    stage: SyncStage,
    start_time: Option<Instant>,
    last_update: Option<Instant>,
    eta_seconds: Option<u64>,
    blocks_per_second: f64,
    last_checkpoint: Option<u64>,
    // Performance counters
    notes_decrypted: u64,
    last_batch_ms: u64,
    total_batch_ms: u64,
    batch_count: u64,
    commitments_applied: u64,
}

impl SyncProgress {
    /// Create new progress tracker
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ProgressInner {
                current_height: 0,
                target_height: 0,
                start_height: 0,
                stage: SyncStage::Headers,
                start_time: None,
                last_update: None,
                eta_seconds: None,
                blocks_per_second: 0.0,
                last_checkpoint: None,
                notes_decrypted: 0,
                last_batch_ms: 0,
                total_batch_ms: 0,
                batch_count: 0,
                commitments_applied: 0,
            })),
        }
    }

    /// Start tracking
    pub fn start(&self) {
        let mut inner = self.inner.write();
        inner.start_time = Some(Instant::now());
        inner.last_update = Some(Instant::now());
        inner.start_height = inner.current_height;
        // Reset perf counters
        inner.notes_decrypted = 0;
        inner.last_batch_ms = 0;
        inner.total_batch_ms = 0;
        inner.batch_count = 0;
        inner.commitments_applied = 0;
    }

    /// Update current height
    pub fn set_current(&self, height: u64) {
        self.inner.write().current_height = height;
    }

    /// Set target height
    pub fn set_target(&self, height: u64) {
        self.inner.write().target_height = height;
    }

    /// Set stage
    pub fn set_stage(&self, stage: SyncStage) {
        self.inner.write().stage = stage;
    }

    /// Set last checkpoint
    pub fn set_checkpoint(&self, height: u64) {
        self.inner.write().last_checkpoint = Some(height);
    }

    /// Update ETA calculation
    pub fn update_eta(&self) {
        let mut inner = self.inner.write();

        if let (Some(start_time), Some(_last_update)) = (inner.start_time, inner.last_update) {
            let elapsed = start_time.elapsed().as_secs_f64();
            let blocks_synced = inner.current_height.saturating_sub(inner.start_height);
            let blocks_remaining = inner.target_height.saturating_sub(inner.current_height);

            if blocks_synced > 0 && elapsed > 0.0 {
                inner.blocks_per_second = blocks_synced as f64 / elapsed;

                if inner.blocks_per_second > 0.0 {
                    inner.eta_seconds =
                        Some((blocks_remaining as f64 / inner.blocks_per_second) as u64);
                }
            }

            inner.last_update = Some(Instant::now());
        }
    }

    /// Get progress percentage
    pub fn percentage(&self) -> f64 {
        let inner = self.inner.read();
        if inner.target_height == 0 {
            return 0.0;
        }
        if inner.current_height >= inner.target_height {
            return 100.0;
        }
        if inner.target_height <= inner.start_height {
            return 0.0;
        }

        let total = inner.target_height - inner.start_height;
        let done = inner.current_height.saturating_sub(inner.start_height);

        (done as f64 / total as f64) * 100.0
    }

    /// Get current height
    pub fn current_height(&self) -> u64 {
        self.inner.read().current_height
    }

    /// Get target height
    pub fn target_height(&self) -> u64 {
        self.inner.read().target_height
    }

    /// Get current stage
    pub fn stage(&self) -> SyncStage {
        self.inner.read().stage
    }

    /// Get ETA in seconds
    pub fn eta_seconds(&self) -> Option<u64> {
        self.inner.read().eta_seconds
    }

    /// Get blocks per second
    pub fn blocks_per_second(&self) -> f64 {
        self.inner.read().blocks_per_second
    }

    /// Get last checkpoint height
    pub fn last_checkpoint(&self) -> Option<u64> {
        self.inner.read().last_checkpoint
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Option<Duration> {
        self.inner.read().start_time.map(|start| start.elapsed())
    }

    // ========================================================================
    // Performance Counters
    // ========================================================================

    /// Record batch completion with perf metrics
    pub fn record_batch(&self, notes: u64, commitments: u64, duration_ms: u64) {
        let mut inner = self.inner.write();
        inner.notes_decrypted += notes;
        inner.commitments_applied += commitments;
        inner.last_batch_ms = duration_ms;
        inner.total_batch_ms += duration_ms;
        inner.batch_count += 1;
    }

    /// Get notes decrypted count
    pub fn notes_decrypted(&self) -> u64 {
        self.inner.read().notes_decrypted
    }

    /// Get last batch processing time in ms
    pub fn last_batch_ms(&self) -> u64 {
        self.inner.read().last_batch_ms
    }

    /// Get average batch processing time in ms
    pub fn avg_batch_ms(&self) -> u64 {
        let inner = self.inner.read();
        if inner.batch_count == 0 {
            return 0;
        }
        inner.total_batch_ms / inner.batch_count
    }

    /// Get total commitments applied
    pub fn commitments_applied(&self) -> u64 {
        self.inner.read().commitments_applied
    }

    /// Get perf counters snapshot
    pub fn perf_snapshot(&self) -> PerfCountersSnapshot {
        let inner = self.inner.read();
        PerfCountersSnapshot {
            notes_decrypted: inner.notes_decrypted,
            last_batch_ms: inner.last_batch_ms,
            avg_batch_ms: if inner.batch_count > 0 {
                inner.total_batch_ms / inner.batch_count
            } else {
                0
            },
            commitments_applied: inner.commitments_applied,
        }
    }

    /// Mark as complete
    pub fn complete(&self) {
        let mut inner = self.inner.write();
        inner.stage = SyncStage::Complete;
        inner.current_height = inner.target_height;
        inner.eta_seconds = Some(0);
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        let inner = self.inner.read();
        inner.stage == SyncStage::Complete || inner.current_height >= inner.target_height
    }

    /// Get summary string
    pub fn summary(&self) -> String {
        let inner = self.inner.read();

        let progress_pct = if inner.target_height > 0 {
            (inner.current_height as f64 / inner.target_height as f64) * 100.0
        } else {
            0.0
        };

        let eta_str = match inner.eta_seconds {
            Some(secs) if secs > 0 => format!("ETA: {}m {}s", secs / 60, secs % 60),
            _ => "ETA: calculating...".to_string(),
        };

        format!(
            "{} | {}/{} ({:.1}%) | {:.1} blocks/s | {}",
            inner.stage.name(),
            inner.current_height,
            inner.target_height,
            progress_pct,
            inner.blocks_per_second,
            eta_str
        )
    }
}

impl Default for SyncProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_creation() {
        let progress = SyncProgress::new();
        assert_eq!(progress.current_height(), 0);
        assert_eq!(progress.target_height(), 0);
    }

    #[test]
    fn test_progress_percentage() {
        let progress = SyncProgress::new();
        progress.set_target(100);
        progress.set_current(50);

        // Need to set start height for accurate percentage
        progress.start();

        assert!(progress.percentage() >= 0.0 && progress.percentage() <= 100.0);
    }

    #[test]
    fn test_stage_names() {
        assert_eq!(SyncStage::Headers.name(), "Fetching Headers");
        assert_eq!(SyncStage::Notes.name(), "Scanning Notes");
        assert_eq!(SyncStage::Witness.name(), "Building Witnesses");
        assert_eq!(SyncStage::Verify.name(), "Synching Chain");
        assert_eq!(SyncStage::Complete.name(), "Synced");
    }

    #[test]
    fn test_completion() {
        let progress = SyncProgress::new();
        progress.set_target(100);
        progress.set_current(0);

        assert!(!progress.is_complete());

        progress.complete();
        assert!(progress.is_complete());
        assert_eq!(progress.stage(), SyncStage::Complete);
    }

    #[test]
    fn test_summary_string() {
        let progress = SyncProgress::new();
        progress.set_target(1000);
        progress.set_current(500);
        progress.start();

        let summary = progress.summary();
        assert!(summary.contains("500"));
        assert!(summary.contains("1000"));
    }
}
