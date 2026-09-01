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

/// Address type (Sapling or Ironwood)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressType {
    /// Sapling address (zs1...)
    Sapling,
    /// Ironwood address (pirate1...)
    Ironwood,
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
    /// Complete ZIP-32 diversifier index in protocol little-endian form.
    ///
    /// `None` identifies a legacy row that has not yet been recovered from
    /// its viewing key and encoded address. `diversifier_index` remains the
    /// stable, user-facing address sequence number.
    #[serde(default)]
    pub diversifier_index_88: Option<[u8; 11]>,
    /// Address string
    pub address: String,
    /// Address type (Sapling or Ironwood)
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

/// User-managed display preferences for a derived wallet address.
///
/// These flags never affect address ownership, note detection, or balances.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressDisplayPreference {
    /// Address row ID.
    pub address_id: i64,
    /// Whether the address should sort ahead of other visible addresses.
    pub is_pinned: bool,
    /// Whether the address is hidden from the default address-history view.
    pub is_archived: bool,
}

/// Note type (Sapling or Ironwood)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteType {
    /// Sapling note
    Sapling,
    /// Ironwood note
    Ironwood,
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
    /// Note type (Sapling or Ironwood)
    pub note_type: NoteType,
    /// Value in arrrtoshis
    pub value: i64,
    /// Nullifier
    pub nullifier: Vec<u8>,
    /// Commitment (Sapling) or action commitment (Ironwood)
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
    /// Serialized note bytes (Sapling/Ironwood)
    pub note: Option<Vec<u8>>,
    /// Position in the Ironwood note commitment tree (Ironwood only)
    pub position: Option<i64>,
    /// Optional memo bytes
    pub memo: Option<Vec<u8>>,
}

/// Canonical historical receive output for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedNoteRecord {
    /// Canonical display transaction ID.
    pub txid: String,
    /// Shielded pool containing the output.
    pub note_type: NoteType,
    /// Output or action index within the pool.
    pub output_index: i64,
    /// Value in arrrtoshis.
    pub value: i64,
    /// Block height, or zero when unconfirmed.
    pub height: i64,
    /// Stored block timestamp when available.
    pub timestamp: Option<i64>,
    /// Linked address row ID when available.
    pub address_id: Option<i64>,
    /// Serialized note material used to recover legacy address links.
    pub note: Option<Vec<u8>>,
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
    /// Encrypted Ironwood extended spending key (optional)
    pub orchard_extsk: Option<Vec<u8>>,
    /// Encrypted Ironwood extended FVK bytes (optional)
    pub orchard_fvk: Option<Vec<u8>>,
    /// Encrypted mnemonic (seed accounts only)
    pub encrypted_mnemonic: Option<Vec<u8>>,
}

/// Provenance for a shielded account key derived from a wallet seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedDerivedAccountKey {
    /// Account-key row containing the encrypted spending and viewing material.
    pub key_id: i64,
    /// Wallet account that owns the derived key.
    pub account_id: i64,
    /// Hardened ZIP-32 account component used to derive the key.
    pub derivation_index: u32,
    /// Whether the key is temporary lookahead awaiting a successful scan.
    pub is_discovery_candidate: bool,
}

/// Outcome of finalizing a successful legacy Sapling account discovery scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedAccountDiscoveryFinalization {
    /// Candidate keys retained because at least one historical note matched.
    pub retained: usize,
    /// Candidate keys removed because no historical note matched.
    pub retired: usize,
    /// Highest retained ZIP-32 account index, if any.
    pub highest_used_index: Option<u32>,
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
    /// Encrypted Ironwood extended spending key bytes (optional) - None for watch-only
    pub orchard_extsk: Option<Vec<u8>>,
    /// Sapling IVK bytes (32 bytes) - for watch-only wallets
    pub sapling_ivk: Option<Vec<u8>>,
    /// Ironwood IVK bytes (64 bytes) - for watch-only wallets
    pub orchard_ivk: Option<Vec<u8>>,
    /// Encrypted mnemonic seed phrase (only for wallets created/restored from seed, None for private key imports or watch-only)
    pub encrypted_mnemonic: Option<Vec<u8>>,
    /// Stable mnemonic language key for deterministic export and re-derive.
    pub mnemonic_language: Option<String>,
    /// Created timestamp
    pub created_at: i64,
}

/// Metadata for an opt-in wallet-scoped signing credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningProtectionRecord {
    /// Wallet protected by the credential.
    pub wallet_id: String,
    /// Wallet account whose spending material is protected.
    pub account_id: i64,
    /// Argon2id salt used to derive the session encryption key.
    pub kdf_salt: Vec<u8>,
    /// Authenticated ciphertext used to verify a supplied credential.
    pub credential_check: Vec<u8>,
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
    /// Whether the locally scanned chain passed this transaction's expiry height
    /// without observing a confirmation.
    pub expired: bool,
    /// Consensus expiry height for wallet-authored transactions.
    pub expiry_height: Option<u32>,
}

/// Wallet-authored details retained independently of chain-derived state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingTransactionIntent {
    /// Transaction ID in display byte order.
    pub txid: String,
    /// Account that authored the transaction.
    pub account_id: i64,
    /// Total value sent to requested recipients, excluding fee and change.
    pub amount: u64,
    /// Transaction fee.
    pub fee: u64,
    /// Unix timestamp recorded after a successful broadcast.
    pub broadcast_at: i64,
    /// Consensus height after which an unmined transaction is invalid.
    ///
    /// Zero is reserved for intents written by wallet versions that did not
    /// persist this value.
    pub expiry_height: u32,
}
