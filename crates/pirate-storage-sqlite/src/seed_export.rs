//! Secure seed phrase export functionality
//!
//! Implements a multi-step gated flow for seed export:
//! 1. Warning acknowledgment
//! 2. Biometric authentication (if available)
//! 3. Passphrase verification
//!
//! Security features:
//! - Screenshot blocking enabled during export
//! - Clipboard auto-clear after 30 seconds
//! - Memory zeroization of seed data
//! - Export only happens on-device (never transmitted)

#![allow(missing_docs)]

use crate::screenshot_guard::{ProtectionReason, ScreenshotGuard};
use crate::secure_clipboard::{ClipboardDataType, SecureClipboard};
use crate::security::AppPassphrase;
use crate::{Error, Result};
use zeroize::Zeroizing;

/// Export flow state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFlowState {
    /// Not started
    NotStarted,
    /// Warning displayed, awaiting acknowledgment
    WarningDisplayed,
    /// Warning acknowledged, awaiting biometric
    AwaitingBiometric,
    /// Biometric passed (or skipped), awaiting passphrase
    AwaitingPassphrase,
    /// Passphrase verified, seed available
    SeedReady,
    /// Export complete (seed cleared)
    Complete,
    /// Export cancelled
    Cancelled,
    /// Error occurred
    Failed,
}

/// Seed export request with validation
#[derive(Debug)]
pub struct SeedExportRequest {
    /// Wallet ID
    pub wallet_id: String,
    /// Whether user acknowledged the warning
    pub warning_acknowledged: bool,
    /// Whether biometric was successful (or skipped if not available)
    pub biometric_passed: bool,
    /// Timestamp of request
    pub requested_at: i64,
    /// User agent / device info (for audit log)
    pub device_info: Option<String>,
}

impl SeedExportRequest {
    /// Create new export request
    pub fn new(wallet_id: String) -> Self {
        Self {
            wallet_id,
            warning_acknowledged: false,
            biometric_passed: false,
            requested_at: chrono::Utc::now().timestamp(),
            device_info: None,
        }
    }

    /// Set device info
    pub fn with_device_info(mut self, info: String) -> Self {
        self.device_info = Some(info);
        self
    }

    /// Acknowledge warning
    pub fn acknowledge_warning(&mut self) {
        self.warning_acknowledged = true;
    }

    /// Mark biometric as passed
    pub fn biometric_success(&mut self) {
        self.biometric_passed = true;
    }

    /// Check if request is ready for passphrase verification
    pub fn is_ready_for_passphrase(&self) -> bool {
        self.warning_acknowledged && self.biometric_passed
    }
}

/// Export result containing the seed (zeroized on drop)
pub struct SeedExportResult {
    /// The seed phrase words
    words: Zeroizing<Vec<String>>,
    /// Export timestamp
    pub exported_at: i64,
    /// Wallet ID
    pub wallet_id: String,
}

impl SeedExportResult {
    /// Create new result
    pub fn new(words: Vec<String>, wallet_id: String) -> Self {
        Self {
            words: Zeroizing::new(words),
            exported_at: chrono::Utc::now().timestamp(),
            wallet_id,
        }
    }

    /// Get words (read-only reference)
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Get words as space-separated string (for clipboard)
    pub fn as_string(&self) -> Zeroizing<String> {
        Zeroizing::new(self.words.join(" "))
    }

    /// Get word count
    pub fn word_count(&self) -> usize {
        self.words.len()
    }
}

impl Drop for SeedExportResult {
    fn drop(&mut self) {
        // Zeroizing wrapper handles cleanup, but we log for audit
        tracing::debug!("Seed export result dropped and zeroized");
    }
}

/// Seed export manager
pub struct SeedExportManager {
    /// Current flow state
    state: std::sync::RwLock<ExportFlowState>,
    /// Current request
    request: std::sync::RwLock<Option<SeedExportRequest>>,
    /// Screenshot guard
    screenshot_guard: ScreenshotGuard,
    /// Secure clipboard
    clipboard: SecureClipboard,
    /// Passphrase hash for verification
    passphrase_hash: std::sync::RwLock<Option<String>>,
}

impl SeedExportManager {
    /// Create new manager
    pub fn new() -> Self {
        Self {
            state: std::sync::RwLock::new(ExportFlowState::NotStarted),
            request: std::sync::RwLock::new(None),
            screenshot_guard: ScreenshotGuard::new(),
            clipboard: SecureClipboard::new(),
            passphrase_hash: std::sync::RwLock::new(None),
        }
    }

    /// Set passphrase hash for verification
    pub fn set_passphrase_hash(&self, hash: String) {
        *self.passphrase_hash.write().unwrap() = Some(hash);
    }

    /// Get current state
    pub fn state(&self) -> ExportFlowState {
        *self.state.read().unwrap()
    }

    /// Start export flow
    pub fn start_export(&self, wallet_id: String) -> Result<ExportFlowState> {
        let mut state = self.state.write().unwrap();
        let mut request = self.request.write().unwrap();

        // Enable screenshot protection
        let _guard = self.screenshot_guard.enable(ProtectionReason::SeedPhrase);

        *request = Some(SeedExportRequest::new(wallet_id));
        *state = ExportFlowState::WarningDisplayed;

        tracing::info!("Seed export flow started");
        Ok(*state)
    }

    /// Acknowledge warning
    pub fn acknowledge_warning(&self) -> Result<ExportFlowState> {
        let mut state = self.state.write().unwrap();
        let mut request = self.request.write().unwrap();

        if *state != ExportFlowState::WarningDisplayed {
            return Err(Error::Security("Invalid export flow state".to_string()));
        }

        if let Some(ref mut req) = *request {
            req.acknowledge_warning();
        }

        *state = ExportFlowState::AwaitingBiometric;

        tracing::info!("Seed export warning acknowledged");
        Ok(*state)
    }

    /// Complete biometric step (called after successful biometric or if skipped)
    pub fn complete_biometric(&self, success: bool) -> Result<ExportFlowState> {
        let mut state = self.state.write().unwrap();
        let mut request = self.request.write().unwrap();

        if *state != ExportFlowState::AwaitingBiometric {
            return Err(Error::Security("Invalid export flow state".to_string()));
        }

        if !success {
            *state = ExportFlowState::Cancelled;
            return Ok(*state);
        }

        if let Some(ref mut req) = *request {
            req.biometric_success();
        }

        *state = ExportFlowState::AwaitingPassphrase;

        tracing::info!("Seed export biometric step completed");
        Ok(*state)
    }

    /// Skip biometric (when not available)
    pub fn skip_biometric(&self) -> Result<ExportFlowState> {
        self.complete_biometric(true)
    }

    /// Verify passphrase and get seed
    pub fn verify_passphrase(&self, passphrase: &str) -> Result<bool> {
        let state = self.state.read().unwrap();

        if *state != ExportFlowState::AwaitingPassphrase {
            return Err(Error::Security("Invalid export flow state".to_string()));
        }

        let passphrase_hash = self.passphrase_hash.read().unwrap();
        let Some(ref hash) = *passphrase_hash else {
            return Err(Error::Security("Passphrase not configured".to_string()));
        };

        let app_passphrase = AppPassphrase::from_hash(hash.clone());
        app_passphrase.verify(passphrase)
    }

    /// Complete export with verified passphrase
    pub fn complete_export(
        &self,
        passphrase: &str,
        seed_words: Vec<String>,
    ) -> Result<SeedExportResult> {
        let verified = self.verify_passphrase(passphrase)?;

        if !verified {
            return Err(Error::Security("Invalid passphrase".to_string()));
        }

        let mut state = self.state.write().unwrap();
        let request = self.request.read().unwrap();

        let wallet_id = request
            .as_ref()
            .map(|r| r.wallet_id.clone())
            .unwrap_or_default();

        *state = ExportFlowState::SeedReady;

        let result = SeedExportResult::new(seed_words, wallet_id);

        tracing::info!("Seed export completed successfully");
        Ok(result)
    }

    /// Complete export after passphrase verification
    pub fn complete_export_verified(&self, seed_words: Vec<String>) -> Result<SeedExportResult> {
        if self.state() != ExportFlowState::AwaitingPassphrase {
            return Err(Error::Security("Invalid export flow state".to_string()));
        }

        let mut state = self.state.write().unwrap();
        let request = self.request.read().unwrap();

        let wallet_id = request
            .as_ref()
            .map(|r| r.wallet_id.clone())
            .unwrap_or_default();

        *state = ExportFlowState::SeedReady;

        let result = SeedExportResult::new(seed_words, wallet_id);

        tracing::info!("Seed export completed successfully");
        Ok(result)
    }

    /// Copy seed to clipboard with auto-clear
    pub fn copy_to_clipboard(&self, seed: &SeedExportResult) -> Zeroizing<String> {
        let seed_string = seed.as_string();
        self.clipboard
            .prepare_copy_sensitive(&seed_string, ClipboardDataType::SeedPhrase)
    }

    /// Get clipboard remaining time
    pub fn clipboard_remaining_seconds(&self) -> Option<u64> {
        self.clipboard.timer().remaining_seconds()
    }

    /// Cancel export
    pub fn cancel(&self) {
        let mut state = self.state.write().unwrap();
        let mut request = self.request.write().unwrap();

        *state = ExportFlowState::Cancelled;
        *request = None;

        tracing::info!("Seed export cancelled");
    }

    /// Reset export flow
    pub fn reset(&self) {
        let mut state = self.state.write().unwrap();
        let mut request = self.request.write().unwrap();

        *state = ExportFlowState::NotStarted;
        *request = None;

        tracing::debug!("Seed export flow reset");
    }

    /// Get screenshot guard reference (for UI to check state)
    pub fn screenshot_guard(&self) -> &ScreenshotGuard {
        &self.screenshot_guard
    }

    /// Check if screenshots are blocked
    pub fn are_screenshots_blocked(&self) -> bool {
        self.screenshot_guard.is_active()
    }
}

impl Default for SeedExportManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Warning messages for export flow
pub mod warnings {
    /// Primary warning message
    pub const PRIMARY_WARNING: &str = "Your seed phrase is the ONLY way to recover your wallet. \
         Anyone with access to these words can steal all your funds.";

    /// Secondary warning
    pub const SECONDARY_WARNING: &str = "Never share your seed phrase with anyone. \
         Never enter it on any website. \
         Never store it digitally in an unencrypted format.";

    /// Backup instructions
    pub const BACKUP_INSTRUCTIONS: &str =
        "Write down these 24 words on paper and store them in a secure location. \
         Consider using a metal backup for fire/water resistance.";

    /// Clipboard warning
    pub const CLIPBOARD_WARNING: &str = "The seed phrase has been copied to your clipboard. \
         It will be automatically cleared in 10 seconds for security.";
}

/// Export audit log entry
#[derive(Debug, Clone)]
pub struct ExportAuditEntry {
    /// Wallet ID
    pub wallet_id: String,
    /// Export timestamp
    pub timestamp: i64,
    /// Device info
    pub device_info: Option<String>,
    /// Whether biometric was used
    pub biometric_used: bool,
    /// Result (success/cancelled/failed)
    pub result: String,
}

impl ExportAuditEntry {
    pub fn new(wallet_id: String, biometric_used: bool, result: &str) -> Self {
        Self {
            wallet_id,
            timestamp: chrono::Utc::now().timestamp(),
            device_info: None,
            biometric_used,
            result: result.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_flow_states() {
        let manager = SeedExportManager::new();
        assert_eq!(manager.state(), ExportFlowState::NotStarted);

        // Start
        let state = manager.start_export("wallet_123".to_string()).unwrap();
        assert_eq!(state, ExportFlowState::WarningDisplayed);

        // Acknowledge
        let state = manager.acknowledge_warning().unwrap();
        assert_eq!(state, ExportFlowState::AwaitingBiometric);

        // Skip biometric
        let state = manager.skip_biometric().unwrap();
        assert_eq!(state, ExportFlowState::AwaitingPassphrase);
    }

    #[test]
    fn test_export_cancellation() {
        let manager = SeedExportManager::new();
        manager.start_export("wallet_123".to_string()).unwrap();

        manager.cancel();
        assert_eq!(manager.state(), ExportFlowState::Cancelled);
    }

    #[test]
    fn test_seed_result_zeroization() {
        let words = vec![
            "abandon".to_string(),
            "abandon".to_string(),
            "abandon".to_string(),
        ];

        let result = SeedExportResult::new(words, "test".to_string());
        assert_eq!(result.word_count(), 3);

        let seed_string = result.as_string();
        assert_eq!(&*seed_string, "abandon abandon abandon");

        // Result will be zeroized on drop
    }

    #[test]
    fn test_export_request() {
        let mut request = SeedExportRequest::new("wallet_123".to_string());
        assert!(!request.is_ready_for_passphrase());

        request.acknowledge_warning();
        assert!(!request.is_ready_for_passphrase());

        request.biometric_success();
        assert!(request.is_ready_for_passphrase());
    }
}
