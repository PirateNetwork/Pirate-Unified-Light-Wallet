//! FFI data models
//!
//! All types that cross the FFI boundary must be FFB-compatible.

use serde::{Deserialize, Serialize};

/// Wallet identifier
pub type WalletId = String;

/// Transaction identifier
pub type TxId = String;

/// Wallet metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletMeta {
    /// Wallet ID
    pub id: WalletId,
    /// Wallet name
    pub name: String,
    /// Created timestamp
    pub created_at: i64,
    /// Is watch-only
    pub watch_only: bool,
    /// Birthday height
    pub birthday_height: u32,
    /// Network type (mainnet, testnet, regtest)
    pub network_type: Option<String>, // Serialized as "mainnet", "testnet", "regtest"
}

/// Transaction output for send-to-many
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    /// Recipient address (Sapling zs1... or Orchard pirate1...)
    pub addr: String,
    /// Amount in arrrtoshis
    pub amount: u64,
    /// Optional memo (max 512 bytes UTF-8)
    pub memo: Option<String>,
}

impl Output {
    /// Create new output
    pub fn new(addr: String, amount: u64, memo: Option<String>) -> Self {
        Self { addr, amount, memo }
    }
    
    /// Validate output
    pub fn validate(&self) -> Result<(), String> {
        if self.amount == 0 {
            return Err("Amount cannot be zero".to_string());
        }
        
        let is_orchard = self.addr.starts_with("pirate1")
            || self.addr.starts_with("pirate-test1")
            || self.addr.starts_with("pirate-regtest1");
        let is_sapling = self.addr.starts_with("zs1")
            || self.addr.starts_with("ztestsapling1")
            || self.addr.starts_with("zregtestsapling1");
        if !is_orchard && !is_sapling {
            return Err(
                "Invalid address format (must start with zs1... or pirate1...)".to_string(),
            );
        }
        
        if let Some(ref memo) = self.memo {
            if memo.as_bytes().len() > 512 {
                return Err(format!("Memo too long: {} bytes (max 512)", memo.as_bytes().len()));
            }
        }
        
        Ok(())
    }
}

/// Pending transaction (built but not signed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTx {
    /// Temporary ID
    pub id: String,
    /// Outputs
    pub outputs: Vec<Output>,
    /// Total output amount (excluding fee)
    pub total_amount: u64,
    /// Transaction fee
    pub fee: u64,
    /// Change amount returned to sender
    pub change: u64,
    /// Total input amount (total_amount + fee + change)
    pub input_total: u64,
    /// Number of inputs (notes) used
    pub num_inputs: u32,
    /// Expiry height (tx invalid after this)
    pub expiry_height: u32,
    /// Created timestamp
    pub created_at: i64,
}

impl PendingTx {
    /// Check if transaction has memo(s)
    pub fn has_memo(&self) -> bool {
        self.outputs.iter().any(|o| o.memo.is_some())
    }
    
    /// Get total value being sent
    pub fn total_send_value(&self) -> u64 {
        self.total_amount + self.fee
    }
}

/// Signed transaction ready for broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTx {
    /// Transaction ID (double SHA-256 of raw tx)
    pub txid: TxId,
    /// Raw transaction bytes
    pub raw: Vec<u8>,
    /// Transaction size in bytes
    pub size: usize,
}

/// Transaction broadcast result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastResult {
    /// Transaction ID
    pub txid: TxId,
    /// Broadcast success
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Transaction build error for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxError {
    /// Insufficient funds
    InsufficientFunds { required: u64, available: u64 },
    /// Invalid address
    InvalidAddress { address: String, reason: String },
    /// Memo too long
    MemoTooLong { length: usize, max: usize },
    /// Network unavailable
    NetworkDown { reason: String },
    /// Broadcast failed
    BroadcastFailed { reason: String },
    /// Other error
    Other { message: String },
}

impl TxError {
    /// Get user-friendly message
    pub fn user_message(&self) -> String {
        match self {
            TxError::InsufficientFunds { required, available } => {
                format!(
                    "Insufficient funds: need {} ARRR, have {} ARRR",
                    *required as f64 / 100_000_000.0,
                    *available as f64 / 100_000_000.0
                )
            }
            TxError::InvalidAddress { address, reason } => {
                format!("Invalid address '{}': {}", address, reason)
            }
            TxError::MemoTooLong { length, max } => {
                format!("Memo too long: {} bytes (maximum {} bytes)", length, max)
            }
            TxError::NetworkDown { reason } => {
                format!("Network unavailable: {}", reason)
            }
            TxError::BroadcastFailed { reason } => {
                format!("Failed to broadcast: {}", reason)
            }
            TxError::Other { message } => message.clone(),
        }
    }
}

/// Sync mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SyncMode {
    /// Compact block sync
    Compact,
    /// Deep scan (trial decrypt all notes)
    Deep,
}

/// Sync stage
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SyncStage {
    /// Fetching headers
    Headers,
    /// Scanning notes
    Notes,
    /// Building witness tree
    Witness,
    /// Verifying chain
    Verify,
}

/// Sync status with full performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    /// Local block height
    pub local_height: u64,
    /// Target block height
    pub target_height: u64,
    /// Progress percentage (0.0 - 100.0)
    pub percent: f64,
    /// Estimated time remaining (seconds)
    pub eta: Option<u64>,
    /// Current stage
    pub stage: SyncStage,
    /// Last checkpoint height
    pub last_checkpoint: Option<u64>,
    /// Blocks processed per second (performance metric)
    pub blocks_per_second: f64,
    /// Number of notes decrypted in current session
    pub notes_decrypted: u64,
    /// Duration of last batch processing in milliseconds
    pub last_batch_ms: u64,
}

impl SyncStatus {
    /// Check if sync is actively running
    pub fn is_syncing(&self) -> bool {
        self.local_height < self.target_height && self.target_height > 0
    }
    
    /// Check if sync is complete
    pub fn is_complete(&self) -> bool {
        self.local_height >= self.target_height && self.target_height > 0
    }
    
    /// Get formatted ETA string
    pub fn eta_formatted(&self) -> String {
        match self.eta {
            Some(secs) if secs > 3600 => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
            Some(secs) if secs > 60 => format!("{}m {}s", secs / 60, secs % 60),
            Some(secs) => format!("{}s", secs),
            None => "Calculating...".to_string(),
        }
    }
    
    /// Get stage display name
    pub fn stage_name(&self) -> &'static str {
        match self.stage {
            SyncStage::Headers => "Fetching Headers",
            SyncStage::Notes => "Scanning Notes",
            SyncStage::Witness => "Building Witnesses",
            SyncStage::Verify => "Synching Chain",
        }
    }
}

/// Network tunnel mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TunnelMode {
    /// Tor (default)
    Tor,
    /// SOCKS5 proxy
    Socks5 {
        /// Proxy URL
        url: String,
    },
    /// Direct connection (no privacy)
    Direct,
}

/// Balance info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// Total balance
    pub total: u64,
    /// Spendable balance
    pub spendable: u64,
    /// Pending balance (unconfirmed)
    pub pending: u64,
}

/// Transaction info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInfo {
    /// Transaction ID
    pub txid: TxId,
    /// Block height (None if unconfirmed)
    pub height: Option<u32>,
    /// Timestamp
    pub timestamp: i64,
    /// Amount (positive for receive, negative for send)
    pub amount: i64,
    /// Fee
    pub fee: u64,
    /// Memo
    pub memo: Option<String>,
    /// Confirmed
    pub confirmed: bool,
}

/// Address with label
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressInfo {
    /// Address string
    pub address: String,
    /// Diversifier index
    pub diversifier_index: u32,
    /// Label
    pub label: Option<String>,
    /// Created timestamp (unix seconds)
    pub created_at: i64,
    /// Color tag
    pub color_tag: AddressBookColorTag,
}

/// Network information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// Network name
    pub name: String,
    /// Coin type
    pub coin_type: u32,
    /// RPC port
    pub rpc_port: u16,
    /// Default birthday height
    pub default_birthday: u32,
}

/// Build information for reproducible verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Version string
    pub version: String,
    /// Git commit hash
    pub git_commit: String,
    /// Build date
    pub build_date: String,
    /// Rust compiler version
    pub rust_version: String,
    /// Target triple
    pub target_triple: String,
}

// ============================================================================
// Security Feature Models
// ============================================================================

/// Vault mode (real or decoy)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VaultMode {
    /// Normal wallet with real data
    Real,
    /// Decoy vault with empty data (panic PIN activated)
    Decoy,
}

/// Decoy vault configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoyVaultInfo {
    /// Whether decoy vault is enabled
    pub enabled: bool,
    /// Current mode (real or decoy)
    pub mode: VaultMode,
    /// Decoy wallet name
    pub decoy_name: String,
    /// Number of times decoy was activated
    pub activation_count: u32,
}

/// Seed export flow state
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SeedExportState {
    /// Not started
    NotStarted,
    /// Warning displayed
    WarningDisplayed,
    /// Awaiting biometric
    AwaitingBiometric,
    /// Awaiting passphrase
    AwaitingPassphrase,
    /// Seed ready for display
    SeedReady,
    /// Export complete
    Complete,
    /// Cancelled
    Cancelled,
    /// Failed
    Failed,
}

/// Seed export flow info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedExportInfo {
    /// Current state
    pub state: SeedExportState,
    /// Whether screenshots are blocked
    pub screenshots_blocked: bool,
    /// Clipboard auto-clear remaining seconds
    pub clipboard_remaining: Option<u64>,
}

/// Watch-only wallet info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchOnlyInfo {
    /// Whether this is a watch-only wallet
    pub is_watch_only: bool,
    /// Can view incoming transactions
    pub can_view_incoming: bool,
    /// Can spend funds
    pub can_spend: bool,
    /// Can export seed
    pub can_export_seed: bool,
    /// Banner to display
    pub banner: Option<WatchOnlyBanner>,
}

/// Watch-only banner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchOnlyBanner {
    /// Banner type (info, warning, error)
    pub banner_type: String,
    /// Title text
    pub title: String,
    /// Subtitle text
    pub subtitle: String,
    /// Icon name
    pub icon: String,
}

/// Background sync result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundSyncResult {
    /// Sync mode that was executed
    pub mode: String, // "compact" or "deep"
    /// Number of blocks synced
    pub blocks_synced: u64,
    /// Starting height
    pub start_height: u64,
    /// Ending height
    pub end_height: u64,
    /// Duration in seconds
    pub duration_secs: u64,
    /// Any errors encountered (non-fatal)
    pub errors: Vec<String>,
    /// New balance after sync (if changed)
    pub new_balance: Option<u64>,
    /// Number of new transactions
    pub new_transactions: u32,
}

/// Background sync result for a specific wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBackgroundSyncResult {
    /// Wallet ID
    pub wallet_id: WalletId,
    /// Sync mode that was executed
    pub mode: String, // "compact" or "deep"
    /// Number of blocks synced
    pub blocks_synced: u64,
    /// Starting height
    pub start_height: u64,
    /// Ending height
    pub end_height: u64,
    /// Duration in seconds
    pub duration_secs: u64,
    /// Any errors encountered (non-fatal)
    pub errors: Vec<String>,
    /// New balance after sync (if changed)
    pub new_balance: Option<u64>,
    /// Number of new transactions
    pub new_transactions: u32,
}

/// Sync log entry for diagnostics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncLogEntryFfi {
    /// Unix timestamp
    pub timestamp: i64,
    /// Log level (DEBUG, INFO, WARN, ERROR)
    pub level: String,
    /// Module name
    pub module: String,
    /// Log message
    pub message: String,
}

/// Node test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTestResult {
    /// Whether the connection was successful
    pub success: bool,
    /// Latest block height from the node
    pub latest_block_height: Option<u64>,
    /// Transport mode used (Tor/SOCKS5/Direct)
    pub transport_mode: String,
    /// Whether TLS was used
    pub tls_enabled: bool,
    /// Whether the TLS pin matched (None if no pin was set)
    pub tls_pin_matched: Option<bool>,
    /// The SPKI pin that was expected (if set)
    pub expected_pin: Option<String>,
    /// The actual SPKI pin from the server (if TLS was used)
    pub actual_pin: Option<String>,
    /// Error message if connection failed
    pub error_message: Option<String>,
    /// Response time in milliseconds
    pub response_time_ms: u64,
    /// Server version info (if available)
    pub server_version: Option<String>,
    /// Chain name from server
    pub chain_name: Option<String>,
}

// ============================================================================
// Address Book
// ============================================================================

/// Color tag for address book entries
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AddressBookColorTag {
    None,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Gray,
}

/// Address book entry for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressBookEntryFfi {
    pub id: i64,
    pub wallet_id: String,
    pub address: String,
    pub label: String,
    pub notes: Option<String>,
    pub color_tag: AddressBookColorTag,
    pub is_favorite: bool,
    /// Unix timestamp (seconds)
    pub created_at: i64,
    /// Unix timestamp (seconds)
    pub updated_at: i64,
    /// Unix timestamp (seconds)
    pub last_used_at: Option<i64>,
    pub use_count: u32,
}
