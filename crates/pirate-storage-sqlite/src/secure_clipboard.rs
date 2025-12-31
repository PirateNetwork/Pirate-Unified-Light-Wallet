//! Secure clipboard with automatic clearing
//!
//! Provides secure clipboard operations with:
//! - Automatic clearing after configurable timeout (default: 10s)
//! - Different timeouts for different data types
//! - Platform-specific clipboard access via FFI
//! - Memory zeroization of clipboard data

#![allow(missing_docs)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

/// Default clipboard auto-clear timeout in seconds
pub const DEFAULT_CLEAR_TIMEOUT_SECS: u64 = 10;

/// Timeout for seed phrase (10 seconds for extra security)
pub const SEED_CLEAR_TIMEOUT_SECS: u64 = 10;

/// Timeout for addresses
pub const ADDRESS_CLEAR_TIMEOUT_SECS: u64 = 60;

/// Timeout for transaction IDs
pub const TXID_CLEAR_TIMEOUT_SECS: u64 = 60;

/// Timeout for IVK export (10 seconds for security)
pub const IVK_CLEAR_TIMEOUT_SECS: u64 = 10;

/// Type of sensitive data being copied
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardDataType {
    /// Seed phrase (most sensitive)
    SeedPhrase,
    /// Viewing key (IVK)
    ViewingKey,
    /// Wallet address
    Address,
    /// Transaction ID
    TransactionId,
    /// Memo content
    Memo,
    /// Generic sensitive data
    Sensitive,
    /// Non-sensitive data (no auto-clear)
    Public,
}

impl ClipboardDataType {
    /// Get timeout for this data type
    pub fn timeout_seconds(&self) -> Option<u64> {
        match self {
            Self::SeedPhrase => Some(SEED_CLEAR_TIMEOUT_SECS),
            Self::ViewingKey => Some(IVK_CLEAR_TIMEOUT_SECS),
            Self::Address => Some(ADDRESS_CLEAR_TIMEOUT_SECS),
            Self::TransactionId => Some(TXID_CLEAR_TIMEOUT_SECS),
            Self::Memo => Some(DEFAULT_CLEAR_TIMEOUT_SECS),
            Self::Sensitive => Some(DEFAULT_CLEAR_TIMEOUT_SECS),
            Self::Public => None, // No auto-clear
        }
    }
    
    /// Check if this data type requires auto-clear
    pub fn requires_auto_clear(&self) -> bool {
        self.timeout_seconds().is_some()
    }
}

/// Clipboard clear timer state
#[derive(Debug)]
pub struct ClipboardTimer {
    /// Whether a timer is active
    active: AtomicBool,
    /// When the timer was started (Unix timestamp millis)
    start_time_ms: AtomicU64,
    /// Timeout in milliseconds
    timeout_ms: AtomicU64,
    /// Data type currently in clipboard
    data_type: std::sync::RwLock<ClipboardDataType>,
}

impl ClipboardTimer {
    /// Create new timer
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            start_time_ms: AtomicU64::new(0),
            timeout_ms: AtomicU64::new(0),
            data_type: std::sync::RwLock::new(ClipboardDataType::Public),
        }
    }
    
    /// Start timer for given data type
    pub fn start(&self, data_type: ClipboardDataType) {
        if let Some(timeout_secs) = data_type.timeout_seconds() {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            
            self.start_time_ms.store(now_ms, Ordering::SeqCst);
            self.timeout_ms.store(timeout_secs * 1000, Ordering::SeqCst);
            *self.data_type.write().unwrap() = data_type;
            self.active.store(true, Ordering::SeqCst);
        }
    }
    
    /// Cancel timer
    pub fn cancel(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
    
    /// Check if timer is active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
    
    /// Get remaining time in seconds (None if not active or expired)
    pub fn remaining_seconds(&self) -> Option<u64> {
        if !self.is_active() {
            return None;
        }
        
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        let start_ms = self.start_time_ms.load(Ordering::SeqCst);
        let timeout_ms = self.timeout_ms.load(Ordering::SeqCst);
        let elapsed_ms = now_ms.saturating_sub(start_ms);
        
        if elapsed_ms >= timeout_ms {
            None
        } else {
            Some((timeout_ms - elapsed_ms) / 1000)
        }
    }
    
    /// Check if timer has expired
    pub fn is_expired(&self) -> bool {
        if !self.is_active() {
            return false;
        }
        self.remaining_seconds().is_none()
    }
    
    /// Get current data type
    pub fn data_type(&self) -> ClipboardDataType {
        *self.data_type.read().unwrap()
    }
}

impl Default for ClipboardTimer {
    fn default() -> Self {
        Self::new()
    }
}

/// Secure clipboard manager
/// 
/// This struct manages secure clipboard operations. The actual clipboard
/// access is done via platform-specific FFI calls from Flutter.
pub struct SecureClipboard {
    /// Timer for auto-clear
    timer: Arc<ClipboardTimer>,
    /// Last copied content hash (for verification)
    last_content_hash: std::sync::RwLock<Option<[u8; 32]>>,
}

impl SecureClipboard {
    /// Create new secure clipboard manager
    pub fn new() -> Self {
        Self {
            timer: Arc::new(ClipboardTimer::new()),
            last_content_hash: std::sync::RwLock::new(None),
        }
    }
    
    /// Get timer reference (for UI countdown display)
    pub fn timer(&self) -> Arc<ClipboardTimer> {
        Arc::clone(&self.timer)
    }
    
    /// Prepare content for clipboard (returns content and starts timer)
    /// 
    /// The actual clipboard write should be done via platform FFI.
    /// This method:
    /// 1. Stores the content hash
    /// 2. Starts the auto-clear timer
    /// 3. Returns the content to copy
    pub fn prepare_copy(&self, content: &str, data_type: ClipboardDataType) -> String {
        // Hash content for later verification
        let hash = crate::security::hash_sha256(content.as_bytes());
        *self.last_content_hash.write().unwrap() = Some(hash);
        
        // Start timer
        self.timer.start(data_type);
        
        content.to_string()
    }
    
    /// Prepare sensitive content (zeroized on drop)
    pub fn prepare_copy_sensitive(&self, content: &str, data_type: ClipboardDataType) -> Zeroizing<String> {
        let _ = self.prepare_copy(content, data_type);
        Zeroizing::new(content.to_string())
    }
    
    /// Check if clipboard should be cleared
    pub fn should_clear(&self) -> bool {
        self.timer.is_expired()
    }
    
    /// Mark clipboard as cleared
    pub fn mark_cleared(&self) {
        self.timer.cancel();
        *self.last_content_hash.write().unwrap() = None;
    }
    
    /// Get remaining time until auto-clear
    pub fn remaining_time(&self) -> Option<Duration> {
        self.timer.remaining_seconds().map(Duration::from_secs)
    }
    
    /// Verify current clipboard content matches what we copied
    /// 
    /// Returns true if content matches our last copy, false otherwise.
    /// This is used to avoid clearing clipboard if user has copied something else.
    pub fn verify_content(&self, current_content: &str) -> bool {
        if let Some(expected_hash) = *self.last_content_hash.read().unwrap() {
            let current_hash = crate::security::hash_sha256(current_content.as_bytes());
            expected_hash == current_hash
        } else {
            false
        }
    }
}

impl Default for SecureClipboard {
    fn default() -> Self {
        Self::new()
    }
}

/// Platform clipboard interface (FFI bridge)
/// 
/// This trait defines the clipboard operations that must be implemented
/// via platform-specific code from Flutter.
pub trait ClipboardPlatform: Send + Sync {
    /// Copy text to clipboard
    fn copy(&self, text: &str) -> bool;
    
    /// Get current clipboard content
    fn paste(&self) -> Option<String>;
    
    /// Clear clipboard
    fn clear(&self) -> bool;
    
    /// Check if clipboard contains text
    fn has_text(&self) -> bool;
}

/// Mock clipboard for testing
pub struct MockClipboard {
    content: std::sync::RwLock<Option<String>>,
}

impl MockClipboard {
    pub fn new() -> Self {
        Self {
            content: std::sync::RwLock::new(None),
        }
    }
}

impl Default for MockClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardPlatform for MockClipboard {
    fn copy(&self, text: &str) -> bool {
        *self.content.write().unwrap() = Some(text.to_string());
        true
    }
    
    fn paste(&self) -> Option<String> {
        self.content.read().unwrap().clone()
    }
    
    fn clear(&self) -> bool {
        *self.content.write().unwrap() = None;
        true
    }
    
    fn has_text(&self) -> bool {
        self.content.read().unwrap().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_data_type_timeouts() {
        assert_eq!(ClipboardDataType::SeedPhrase.timeout_seconds(), Some(10));
        assert_eq!(ClipboardDataType::ViewingKey.timeout_seconds(), Some(10));
        assert_eq!(ClipboardDataType::Address.timeout_seconds(), Some(60));
        assert_eq!(ClipboardDataType::Public.timeout_seconds(), None);
    }
    
    #[test]
    fn test_timer_lifecycle() {
        let timer = ClipboardTimer::new();
        
        assert!(!timer.is_active());
        
        timer.start(ClipboardDataType::Address);
        assert!(timer.is_active());
        assert!(timer.remaining_seconds().is_some());
        
        timer.cancel();
        assert!(!timer.is_active());
    }
    
    #[test]
    fn test_secure_clipboard_prepare() {
        let clipboard = SecureClipboard::new();
        
        let content = clipboard.prepare_copy("test_address", ClipboardDataType::Address);
        assert_eq!(content, "test_address");
        assert!(clipboard.timer().is_active());
    }
    
    #[test]
    fn test_secure_clipboard_verify() {
        let clipboard = SecureClipboard::new();
        
        clipboard.prepare_copy("secret_data", ClipboardDataType::Sensitive);
        
        assert!(clipboard.verify_content("secret_data"));
        assert!(!clipboard.verify_content("different_data"));
    }
    
    #[test]
    fn test_mock_clipboard() {
        let clipboard = MockClipboard::new();
        
        assert!(!clipboard.has_text());
        
        clipboard.copy("hello");
        assert!(clipboard.has_text());
        assert_eq!(clipboard.paste(), Some("hello".to_string()));
        
        clipboard.clear();
        assert!(!clipboard.has_text());
    }
    
    #[test]
    fn test_zeroizing_content() {
        let clipboard = SecureClipboard::new();
        
        let sensitive = clipboard.prepare_copy_sensitive("my_seed_phrase", ClipboardDataType::SeedPhrase);
        assert_eq!(&*sensitive, "my_seed_phrase");
        
        // Zeroizing wrapper will clear memory on drop
        drop(sensitive);
    }
}

