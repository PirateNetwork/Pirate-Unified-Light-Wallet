//! Watch-only wallet management via viewing keys
//!
//! Watch-only wallets can:
//! - View incoming transactions
//! - See balance (incoming only)
//! - Generate receive addresses
//!
//! Watch-only wallets CANNOT:
//! - Spend funds
//! - See outgoing transactions (unless they were to self)
//! - Export seed phrase (doesn't have one)

use crate::{Error, Result};
use crate::secure_clipboard::{SecureClipboard, ClipboardDataType};
use crate::screenshot_guard::{ScreenshotGuard, ProtectionReason};
use zeroize::Zeroizing;

/// Watch-only wallet capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOnlyCapabilities {
    /// Can view incoming transactions
    pub can_view_incoming: bool,
    /// Can view outgoing transactions (only to self)
    pub can_view_outgoing: bool,
    /// Can spend funds
    pub can_spend: bool,
    /// Can export seed
    pub can_export_seed: bool,
    /// Can generate addresses
    pub can_generate_addresses: bool,
}

impl WatchOnlyCapabilities {
    /// Get capabilities for watch-only wallet
    pub fn watch_only() -> Self {
        Self {
            can_view_incoming: true,
            can_view_outgoing: false, // Can only see outgoing to own addresses
            can_spend: false,
            can_export_seed: false,
            can_generate_addresses: true,
        }
    }

    /// Get capabilities for full wallet
    pub fn full_wallet() -> Self {
        Self {
            can_view_incoming: true,
            can_view_outgoing: true,
            can_spend: true,
            can_export_seed: true,
            can_generate_addresses: true,
        }
    }
}

/// Viewing key export result
pub struct IvkExportResult {
    /// The viewing key string
    ivk: Zeroizing<String>,
    /// Wallet ID source
    pub wallet_id: String,
    /// Export timestamp
    pub exported_at: i64,
}

impl IvkExportResult {
    /// Create new result
    pub fn new(ivk: String, wallet_id: String) -> Self {
        Self {
            ivk: Zeroizing::new(ivk),
            wallet_id,
            exported_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Get viewing key string
    pub fn ivk(&self) -> &str {
        &self.ivk
    }

    /// Get viewing key for clipboard (zeroized copy)
    pub fn as_clipboard_string(&self) -> Zeroizing<String> {
        Zeroizing::new((*self.ivk).clone())
    }
}

/// Viewing key import request
#[derive(Debug, Clone)]
pub struct IvkImportRequest {
    /// Wallet name
    pub name: String,
    /// Viewing key string
    pub ivk: String,
    /// Birthday height
    pub birthday_height: u32,
}

impl IvkImportRequest {
    /// Create new import request
    pub fn new(name: String, ivk: String, birthday_height: u32) -> Self {
        Self {
            name,
            ivk,
            birthday_height,
        }
    }

    /// Validate viewing key format
    pub fn validate(&self) -> Result<()> {
        if self.ivk.trim().is_empty() {
            return Err(Error::Validation("Viewing key cannot be empty".to_string()));
        }

        let key = self.ivk.trim();
        let is_sapling = key.starts_with("zxviews");
        let is_orchard = key.starts_with("pirate-extended-viewing-key");
        if !(is_sapling || is_orchard) {
            return Err(Error::Validation("Invalid viewing key format".to_string()));
        }

        // Validate name
        if self.name.is_empty() {
            return Err(Error::Validation("Wallet name cannot be empty".to_string()));
        }

        if self.name.len() > 50 {
            return Err(Error::Validation("Wallet name too long (max 50 chars)".to_string()));
        }

        // Validate birthday height
        if self.birthday_height == 0 {
            return Err(Error::Validation("Birthday height must be greater than 0".to_string()));
        }

        Ok(())
    }
}

/// Watch-only wallet metadata
#[derive(Debug, Clone)]
pub struct WatchOnlyWalletMeta {
    /// Wallet ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Whether this is watch-only
    pub watch_only: bool,
    /// Birthday height
    pub birthday_height: u32,
    /// Created timestamp
    pub created_at: i64,
    /// Viewing key fingerprint (for identification, not the actual key)
    pub ivk_fingerprint: String,
}

impl WatchOnlyWalletMeta {
    /// Create new metadata
    pub fn new(id: String, name: String, birthday_height: u32, ivk_fingerprint: String) -> Self {
        Self {
            id,
            name,
            watch_only: true,
            birthday_height,
            created_at: chrono::Utc::now().timestamp(),
            ivk_fingerprint,
        }
    }

    /// Get capabilities
    pub fn capabilities(&self) -> WatchOnlyCapabilities {
        if self.watch_only {
            WatchOnlyCapabilities::watch_only()
        } else {
            WatchOnlyCapabilities::full_wallet()
        }
    }
}

/// Watch-only wallet manager
pub struct WatchOnlyManager {
    /// Screenshot guard for viewing key export
    screenshot_guard: ScreenshotGuard,
    /// Secure clipboard
    clipboard: SecureClipboard,
}

impl WatchOnlyManager {
    /// Create new manager
    pub fn new() -> Self {
        Self {
            screenshot_guard: ScreenshotGuard::new(),
            clipboard: SecureClipboard::new(),
        }
    }

    /// Export viewing key from full wallet
    /// 
    /// This requires the wallet to be unlocked and not watch-only.
    pub fn export_ivk(&self, wallet_id: &str, ivk: String) -> Result<IvkExportResult> {
        // Enable screenshot protection during export
        let _guard = self.screenshot_guard.enable(ProtectionReason::ViewingKey);

        tracing::info!("Exporting viewing key for wallet {}", wallet_id);

        Ok(IvkExportResult::new(ivk, wallet_id.to_string()))
    }

    /// Copy viewing key to clipboard with auto-clear
    pub fn copy_ivk_to_clipboard(&self, result: &IvkExportResult) -> Zeroizing<String> {
        let ivk_string = result.as_clipboard_string();
        self.clipboard.prepare_copy_sensitive(&ivk_string, ClipboardDataType::ViewingKey)
    }

    /// Import viewing key to create watch-only wallet
    pub fn validate_import(&self, request: &IvkImportRequest) -> Result<()> {
        request.validate()
    }

    /// Get clipboard remaining time
    pub fn clipboard_remaining_seconds(&self) -> Option<u64> {
        self.clipboard.timer().remaining_seconds()
    }

    /// Check if screenshots are blocked
    pub fn are_screenshots_blocked(&self) -> bool {
        self.screenshot_guard.is_active()
    }
}

impl Default for WatchOnlyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Banner type for watch-only indication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchOnlyBannerType {
    /// Standard info banner
    Info,
    /// Warning banner (e.g., when trying to send)
    Warning,
    /// Error banner (e.g., when spending attempted)
    Error,
}

/// Watch-only banner content
#[derive(Debug, Clone)]
pub struct WatchOnlyBanner {
    /// Banner type
    pub banner_type: WatchOnlyBannerType,
    /// Title text
    pub title: String,
    /// Subtitle/description
    pub subtitle: String,
    /// Icon name (for UI)
    pub icon: String,
}

impl WatchOnlyBanner {
    /// Create incoming-only banner (default for watch-only wallets)
    pub fn incoming_only() -> Self {
        Self {
            banner_type: WatchOnlyBannerType::Info,
            title: "Incoming Only".to_string(),
            subtitle: "This wallet can only view incoming transactions".to_string(),
            icon: "visibility".to_string(),
        }
    }

    /// Create cannot-spend banner
    pub fn cannot_spend() -> Self {
        Self {
            banner_type: WatchOnlyBannerType::Warning,
            title: "Watch-Only Wallet".to_string(),
            subtitle: "Spending is not available. Import full wallet to send funds.".to_string(),
            icon: "lock".to_string(),
        }
    }

    /// Create spend-blocked error banner
    pub fn spend_blocked() -> Self {
        Self {
            banner_type: WatchOnlyBannerType::Error,
            title: "Cannot Send".to_string(),
            subtitle: "This is a watch-only wallet without spending capability.".to_string(),
            icon: "block".to_string(),
        }
    }
}

/// Messages for watch-only UI
pub mod messages {
    /// Main info message
    pub const WATCH_ONLY_INFO: &str = 
        "This is a watch-only wallet. You can view your incoming balance \
         and generate addresses, but you cannot spend funds.";

    /// Import instructions
    pub const IMPORT_INSTRUCTIONS: &str = 
        "Enter the viewing key exported from another wallet. \
         This creates a view-only copy that cannot spend funds.";

    /// Export warning
    pub const EXPORT_WARNING: &str = 
        "Anyone with this viewing key can see your incoming transactions and balance. \
         Only share it with services or devices you trust.";

    /// Birthday height explanation
    pub const BIRTHDAY_EXPLANATION: &str = 
        "The birthday height is the block when this wallet was first created. \
         Scanning will start from this height to find your transactions.";

    /// Incoming only explanation
    pub const INCOMING_ONLY_EXPLANATION: &str =
        "Watch-only wallets can only detect incoming transactions. \
         Outgoing transactions require the spending key.";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities() {
        let watch_only = WatchOnlyCapabilities::watch_only();
        assert!(watch_only.can_view_incoming);
        assert!(!watch_only.can_spend);
        assert!(!watch_only.can_export_seed);

        let full = WatchOnlyCapabilities::full_wallet();
        assert!(full.can_view_incoming);
        assert!(full.can_spend);
        assert!(full.can_export_seed);
    }

    #[test]
    fn test_import_validation() {
        // Valid request
        let valid = IvkImportRequest::new(
            "My Watch Wallet".to_string(),
            "zxviews-test-key".to_string(),
            2_000_000,
        );
        assert!(valid.validate().is_ok());

        // Empty viewing key
        let empty_ivk = IvkImportRequest::new(
            "Test".to_string(),
            "".to_string(),
            1000,
        );
        assert!(empty_ivk.validate().is_err());

        // Empty name
        let empty_name = IvkImportRequest::new(
            "".to_string(),
            "zxviews-test-key".to_string(),
            1000,
        );
        assert!(empty_name.validate().is_err());

        // Zero birthday
        let zero_birthday = IvkImportRequest::new(
            "Test".to_string(),
            "zxviews-test-key".to_string(),
            0,
        );
        assert!(zero_birthday.validate().is_err());
    }

    #[test]
    fn test_ivk_export_result() {
        let result = IvkExportResult::new(
            "test_ivk_123".to_string(),
            "wallet_456".to_string(),
        );
        
        assert_eq!(result.ivk(), "test_ivk_123");
        assert_eq!(result.wallet_id, "wallet_456");
    }

    #[test]
    fn test_banner_types() {
        let incoming = WatchOnlyBanner::incoming_only();
        assert_eq!(incoming.banner_type, WatchOnlyBannerType::Info);
        assert!(incoming.title.contains("Incoming"));

        let cannot_spend = WatchOnlyBanner::cannot_spend();
        assert_eq!(cannot_spend.banner_type, WatchOnlyBannerType::Warning);

        let blocked = WatchOnlyBanner::spend_blocked();
        assert_eq!(blocked.banner_type, WatchOnlyBannerType::Error);
    }

    #[test]
    fn test_wallet_meta() {
        let meta = WatchOnlyWalletMeta::new(
            "id_123".to_string(),
            "My Watch Wallet".to_string(),
            2_000_000,
            "fingerprint".to_string(),
        );

        assert!(meta.watch_only);
        let caps = meta.capabilities();
        assert!(!caps.can_spend);
    }
}

