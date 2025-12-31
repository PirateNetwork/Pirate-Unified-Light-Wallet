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

/// Address record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    /// Address ID
    pub id: Option<i64>,
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

