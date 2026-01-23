//! Database models

use serde::{Deserialize, Serialize};

use crate::address_book::ColorTag;

/// Account record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Account ID
    pub id: Option<i64>,
    /// Account name
    pub name: String,
    /// Created timestamp
    pub created_at: i64,
}

/// Address type (Sapling or Orchard)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressType {
    /// Sapling address (zs1...)
    Sapling,
    /// Orchard address (pirate1...)
    Orchard,
}

/// Address scope (external receive or internal change)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressScope {
    /// External address shown to users
    External,
    /// Internal address used for change/consolidation
    Internal,
}

/// Address record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    /// Address ID
    pub id: Option<i64>,
    /// Key group ID (seed/import key)
    pub key_id: Option<i64>,
    /// Account ID
    pub account_id: i64,
    /// Diversifier index
    pub diversifier_index: u32,
    /// Address string
    pub address: String,
    /// Address type (Sapling or Orchard)
    pub address_type: AddressType,
    /// Optional label for address book
    pub label: Option<String>,
    /// Created timestamp
    pub created_at: i64,
    /// Optional color tag
    pub color_tag: ColorTag,
    /// Address scope (external/internal)
    pub address_scope: AddressScope,
}

/// Note type (Sapling or Orchard)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteType {
    /// Sapling note
    Sapling,
    /// Orchard note
    Orchard,
}

/// Note record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    /// Note ID
    pub id: Option<i64>,
    /// Account ID
    pub account_id: i64,
    /// Key group ID (seed/import)
    pub key_id: Option<i64>,
    /// Note type (Sapling or Orchard)
    pub note_type: NoteType,
    /// Value in arrrtoshis
    pub value: i64,
    /// Nullifier
    pub nullifier: Vec<u8>,
    /// Commitment (Sapling) or Action Commitment (Orchard)
    pub commitment: Vec<u8>,
    /// Is spent
    pub spent: bool,
    /// Block height
    pub height: i64,
    /// Transaction ID (raw bytes)
    pub txid: Vec<u8>,
    /// Output index within transaction
    pub output_index: i64,
    /// Linked address row id (encrypted in storage)
    pub address_id: Option<i64>,
    /// Spending transaction ID (raw bytes) for spent notes
    pub spent_txid: Option<Vec<u8>>,
    /// Diversifier used to derive address (11 bytes, Sapling only)
    pub diversifier: Option<Vec<u8>>,
    /// Serialized Sapling merkle path bytes (Sapling only)
    pub merkle_path: Option<Vec<u8>>,
    /// Serialized note bytes (Sapling/Orchard)
    pub note: Option<Vec<u8>>,
    /// Anchor (Orchard commitment tree root, Orchard only)
    pub anchor: Option<Vec<u8>>,
    /// Position in Orchard note commitment tree (Orchard only)
    pub position: Option<i64>,
    /// Optional memo bytes
    pub memo: Option<Vec<u8>>,
}

/// Key source type for an account
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyType {
    /// Seed-based account
    Seed,
    /// Imported spending key
    ImportSpend,
    /// Imported viewing key (xFVK)
    ImportView,
}

/// Key scope (account-wide or single-address)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyScope {
    /// Account-level key (derives many addresses)
    Account,
    /// Single-address key (diversified import)
    SingleAddress,
}

/// Account key material and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountKey {
    /// Key group id
    pub id: Option<i64>,
    /// Account id for this key group
    pub account_id: i64,
    /// Key type (seed/import)
    pub key_type: KeyType,
    /// Key scope (account or single address)
    pub key_scope: KeyScope,
    /// Optional label for UI
    pub label: Option<String>,
    /// Wallet birthday height for this key
    pub birthday_height: i64,
    /// Created timestamp
    pub created_at: i64,
    /// Whether the key can spend
    pub spendable: bool,
    /// Encrypted Sapling extended spending key (optional)
    pub sapling_extsk: Option<Vec<u8>>,
    /// Encrypted Sapling DFVK bytes (optional)
    pub sapling_dfvk: Option<Vec<u8>>,
    /// Encrypted Orchard extended spending key (optional)
    pub orchard_extsk: Option<Vec<u8>>,
    /// Encrypted Orchard extended FVK bytes (optional)
    pub orchard_fvk: Option<Vec<u8>>,
    /// Encrypted mnemonic (seed accounts only)
    pub encrypted_mnemonic: Option<Vec<u8>>,
}

/// Wallet secret (encrypted spending key or IVK for watch-only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletSecret {
    /// Wallet ID (matches FFI wallet_id)
    pub wallet_id: String,
    /// Associated account id
    pub account_id: i64,
    /// Encrypted extended spending key bytes (Sapling) - None for watch-only
    pub extsk: Vec<u8>,
    /// Optional cached DFVK bytes (Sapling)
    pub dfvk: Option<Vec<u8>>,
    /// Encrypted Orchard extended spending key bytes (optional) - None for watch-only
    pub orchard_extsk: Option<Vec<u8>>,
    /// Sapling IVK bytes (32 bytes) - for watch-only wallets
    pub sapling_ivk: Option<Vec<u8>>,
    /// Orchard IVK bytes (64 bytes) - for watch-only wallets
    pub orchard_ivk: Option<Vec<u8>>,
    /// Encrypted mnemonic seed phrase (only for wallets created/restored from seed, None for private key imports or watch-only)
    pub encrypted_mnemonic: Option<Vec<u8>>,
    /// Created timestamp
    pub created_at: i64,
}

/// Transaction record for querying transaction history
#[derive(Debug, Clone)]
pub struct TransactionRecord {
    /// Transaction ID (hex string)
    pub txid: String,
    /// Block height (0 if unconfirmed)
    pub height: i64,
    /// Timestamp (block time if confirmed, note insertion time if unconfirmed)
    pub timestamp: i64,
    /// Net amount (positive for receive, negative for send)
    pub amount: i64,
    /// Transaction fee
    pub fee: u64,
    /// Memo (from first note with memo)
    pub memo: Option<Vec<u8>>,
}
