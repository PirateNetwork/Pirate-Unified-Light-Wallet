//! Public API exposed to Flutter via flutter_rust_bridge
//!
//! This module defines the complete FFI surface for the Pirate Unified Wallet.
//! All functions are designed to be called from Flutter through FRB-generated bindings.
//!
//! ## Architecture
//!
//! - **Wallet Management**: Create, restore, list, switch wallets
//! - **Addresses**: Generate, label, list Sapling addresses
//! - **Transactions**: Build, sign, broadcast transactions
//! - **Sync**: Start/stop sync, rescan, progress tracking
//! - **Security**: Panic PIN, seed export, viewing key export
//! - **Network**: Endpoint management, tunnel configuration
//!
//! ## State Management
//!
//! Global state is managed via `lazy_static` RwLocks. This is suitable for
//! single-process mobile/desktop apps. State is persisted to encrypted SQLite.

use crate::models::*;
use anyhow::{anyhow, Result};
use bech32::{Bech32, Hrp};
use directories::ProjectDirs;
use hex;
use orchard::note::ExtractedNoteCommitment;
use orchard::tree::MerkleHashOrchard;
use orchard::Address as OrchardAddress;
use parking_lot::RwLock;
use pirate_core::keys::{
    orchard_extsk_hrp_for_network, ExtendedFullViewingKey, ExtendedSpendingKey,
    OrchardExtendedFullViewingKey, OrchardExtendedSpendingKey, OrchardPaymentAddress,
    PaymentAddress,
};
use pirate_core::wallet::Wallet;
use pirate_params::{Network, NetworkType};
use pirate_storage_sqlite::FrontierStorage;
use pirate_storage_sqlite::{
    address_book::{
        AddressBookEntry as DbAddressBookEntry, AddressBookStorage, ColorTag as DbColorTag,
    },
    passphrase_store, platform_keystore,
    security::{generate_salt, AppPassphrase, EncryptionAlgorithm, MasterKey, SealedKey},
    Account, AccountKey, AddressType, Database, EncryptionKey, KeyScope, KeyType, KeystoreResult,
    Repository, WalletSecret,
};
use pirate_sync_lightd::client::{
    LightClient, LightClientConfig, RetryConfig, TlsConfig, TransportMode,
};
use pirate_sync_lightd::OrchardFrontier;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use zcash_client_backend::encoding::{
    encode_extended_full_viewing_key, encode_extended_spending_key,
};
use zcash_primitives::merkle_tree::{read_frontier_v0, read_frontier_v1};
use zcash_primitives::sapling::PaymentAddress as SaplingPaymentAddress;
use zcash_primitives::zip32::ExtendedFullViewingKey as SaplingExtendedFullViewingKey;

// Global state with thread-safe access
lazy_static::lazy_static! {
    /// Active wallet metadata (persisted to encrypted storage)
    static ref WALLETS: Arc<RwLock<Vec<WalletMeta>>> = Arc::new(RwLock::new(Vec::new()));
    /// Currently active wallet ID
    static ref ACTIVE_WALLET: Arc<RwLock<Option<WalletId>>> = Arc::new(RwLock::new(None));
    /// Network tunnel configuration (Tor default)
    static ref TUNNEL_MODE: Arc<RwLock<TunnelMode>> = Arc::new(RwLock::new(TunnelMode::Tor));
    /// Pending tunnel mode to persist once registry is available.
    static ref PENDING_TUNNEL_MODE: Arc<RwLock<Option<TunnelMode>>> = Arc::new(RwLock::new(None));
}

static REGISTRY_LOADED: AtomicBool = AtomicBool::new(false);
const REGISTRY_APP_PASSPHRASE_KEY: &str = "app_passphrase_hash";
const REGISTRY_DURESS_PASSPHRASE_HASH_KEY: &str = "duress_passphrase_hash";
const REGISTRY_DURESS_USE_REVERSE_KEY: &str = "duress_passphrase_use_reverse";
const REGISTRY_TUNNEL_MODE_KEY: &str = "tunnel_mode";
const REGISTRY_TUNNEL_SOCKS5_URL_KEY: &str = "tunnel_socks5_url";
const DECOY_WALLET_ID: &str = "decoy_wallet";

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        env::current_dir()
            .map(|dir| dir.join(".cursor").join("debug.log"))
            .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

// ============================================================================
// Wallet Lifecycle
// ============================================================================

fn resolve_wallet_birthday_height(birthday_opt: Option<u32>) -> u32 {
    if let Some(birthday) = birthday_opt {
        return birthday;
    }

    let endpoint = LightdEndpoint::default();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let client_config = LightClientConfig {
        endpoint: endpoint.url(),
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: endpoint.use_tls,
            spki_pin: endpoint.tls_pin.clone(),
            server_name: None,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(10),
        request_timeout: std::time::Duration::from_secs(10),
        allow_direct_fallback,
    };
    let client = LightClient::with_config(client_config);
    let fetch_latest = || async {
        if client.connect().await.is_err() {
            return None;
        }
        client.get_latest_block().await.ok().map(|h| h as u32)
    };
    let latest_height = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(fetch_latest()),
        Err(_) => {
            let runtime = tokio::runtime::Runtime::new().ok();
            runtime.as_ref().and_then(|rt| rt.block_on(fetch_latest()))
        }
    };

    latest_height.unwrap_or_else(|| Network::mainnet().default_birthday_height)
}

fn log_orchard_address_samples(wallet_id: &WalletId) {
    let (_db, repo) = match open_wallet_db_for(wallet_id) {
        Ok(result) => result,
        Err(_) => return,
    };
    let secret = match repo.get_wallet_secret(wallet_id) {
        Ok(Some(secret)) => secret,
        _ => return,
    };
    let orchard_extsk_bytes = match secret.orchard_extsk.as_ref() {
        Some(bytes) => bytes,
        None => return,
    };
    let orchard_extsk = match OrchardExtendedSpendingKey::from_bytes(orchard_extsk_bytes) {
        Ok(key) => key,
        Err(_) => return,
    };
    let orchard_fvk = orchard_extsk.to_extended_fvk();

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        let ts = chrono::Utc::now().timestamp_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_orchard_address_samples","timestamp":{},"location":"api.rs:log_orchard_address_samples","message":"orchard address sample header","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
            ts, wallet_id
        );
    }

    for index in 0u32..10u32 {
        let address = orchard_fvk.address_at(index);
        let addr_mainnet = address
            .encode_for_network(NetworkType::Mainnet)
            .unwrap_or_default();
        let addr_testnet = address
            .encode_for_network(NetworkType::Testnet)
            .unwrap_or_default();
        let addr_regtest = address
            .encode_for_network(NetworkType::Regtest)
            .unwrap_or_default();

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_orchard_address_sample","timestamp":{},"location":"api.rs:log_orchard_address_samples","message":"orchard address sample","data":{{"wallet_id":"{}","index":{},"mainnet":"{}","testnet":"{}","regtest":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, index, addr_mainnet, addr_testnet, addr_regtest
            );
        }
    }
}

/// Create new wallet
///
/// Always generates a 24-word mnemonic seed phrase for new wallets.
/// For restoring wallets with 12 or 18 word seeds, use `restore_wallet()`.
pub fn create_wallet(
    name: String,
    _entropy_len: Option<u32>, // Deprecated: always generates 24-word seed
    birthday_opt: Option<u32>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    // Always generate 24-word mnemonic for new wallets
    // (12 and 18 word seeds are only supported for restoring old wallets)
    let mnemonic = ExtendedSpendingKey::generate_mnemonic(Some(24));
    let network = pirate_params::Network::mainnet(); // Will be updated when endpoint is set
    let extsk =
        ExtendedSpendingKey::from_mnemonic_with_account(&mnemonic, "", network.network_type, 0)?;
    let _wallet = Wallet::from_mnemonic(&mnemonic, "")?;

    // Derive Orchard key from same seed
    // Derive account-level key: m/32'/coin_type'/account'
    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(&mnemonic, "")?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;

    let coin_type = network.coin_type;
    let account = 0; // First account
    let orchard_extsk = orchard_master.derive_account(coin_type, account)?;

    let birthday_height = resolve_wallet_birthday_height(birthday_opt);

    let name_for_account = name.clone();
    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height,
        network_type: Some("mainnet".to_string()), // Default to mainnet, can be updated when endpoint is set
    };

    WALLETS.write().push(meta.clone());
    *ACTIVE_WALLET.write() = Some(wallet_id.clone());

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, &meta)?;
    set_active_wallet_registry(&registry_db, Some(&wallet_id))?;
    touch_wallet_last_used(&registry_db, &wallet_id)?;

    // Persist account + wallet secret (encrypted DB)
    if let Ok((_db, repo)) = open_wallet_db_for(&wallet_id) {
        let account = Account {
            id: None,
            name: name_for_account,
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account)?;

        let dfvk_bytes = extsk.to_extended_fvk().to_bytes();
        // Store encrypted mnemonic (field-level + SQLCipher encryption)
        let encrypted_mnemonic = Some(mnemonic.as_bytes().to_vec());
        let secret = WalletSecret {
            wallet_id: wallet_id.clone(),
            account_id,
            extsk: extsk.to_bytes(),
            dfvk: Some(dfvk_bytes),
            orchard_extsk: Some(orchard_extsk.to_bytes()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic,
            created_at: chrono::Utc::now().timestamp(),
        };
        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
        repo.upsert_wallet_secret(&encrypted_secret)?;
        let _ = ensure_primary_account_key(&repo, &wallet_id, &secret)?;

        tracing::info!(
            "Persisted wallet secret (Sapling + Orchard) for wallet {}",
            wallet_id
        );
    }

    Ok(wallet_id)
}

/// Restore wallet from mnemonic
///
/// Supports restoring wallets with 12, 18, or 24 word mnemonic seeds
/// (for backward compatibility with old wallets that used 12 or 18 word seeds).
/// New wallets created with `create_wallet()` always use 24-word seeds.
pub fn restore_wallet(
    name: String,
    mnemonic: String,
    passphrase_opt: Option<String>,
    birthday_opt: Option<u32>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    let passphrase = passphrase_opt.unwrap_or_default();

    // Validate mnemonic by attempting to create wallet (accepts 12, 18, or 24 words)
    let network = pirate_params::Network::mainnet(); // Will be updated when endpoint is set
    let extsk = ExtendedSpendingKey::from_mnemonic_with_account(
        &mnemonic,
        &passphrase,
        network.network_type,
        0,
    )?;
    let _wallet = Wallet::from_mnemonic(&mnemonic, &passphrase)?;

    // Derive Orchard key from same seed
    // Derive account-level key: m/32'/coin_type'/account'
    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(&mnemonic, &passphrase)?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;

    let coin_type = network.coin_type;
    let account = 0; // First account
    let orchard_extsk = orchard_master.derive_account(coin_type, account)?;

    let birthday_height =
        birthday_opt.unwrap_or_else(|| pirate_params::Network::mainnet().default_birthday_height);

    let name_for_account = name.clone();
    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height,
        network_type: Some("mainnet".to_string()), // Default to mainnet, can be updated when endpoint is set
    };

    WALLETS.write().push(meta.clone());
    *ACTIVE_WALLET.write() = Some(wallet_id.clone());

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, &meta)?;
    set_active_wallet_registry(&registry_db, Some(&wallet_id))?;
    touch_wallet_last_used(&registry_db, &wallet_id)?;

    // Persist account + wallet secret (encrypted DB)
    if let Ok((_db, repo)) = open_wallet_db_for(&wallet_id) {
        let account = Account {
            id: None,
            name: name_for_account,
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account)?;

        let dfvk_bytes = extsk.to_extended_fvk().to_bytes();
        // Create wallet secret with plaintext fields (will be encrypted before storage)
        let secret = WalletSecret {
            wallet_id: wallet_id.clone(),
            account_id,
            extsk: extsk.to_bytes(),
            dfvk: Some(dfvk_bytes),
            orchard_extsk: Some(orchard_extsk.to_bytes()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(mnemonic.as_bytes().to_vec()),
            created_at: chrono::Utc::now().timestamp(),
        };
        // Encrypt sensitive fields before storage
        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
        repo.upsert_wallet_secret(&encrypted_secret)?;
        let _ = ensure_primary_account_key(&repo, &wallet_id, &secret)?;

        tracing::info!("Persisted encrypted wallet secret for wallet {}", wallet_id);
    }

    Ok(wallet_id)
}

/// Check if wallet registry database file exists (without opening it)
///
/// This allows checking if wallets exist before the database is created or opened.
pub fn wallet_registry_exists() -> Result<bool> {
    let path = wallet_registry_path()?;
    Ok(path.exists())
}

/// List all wallets
///
/// Returns empty list if database can't be opened (e.g., passphrase not set)
/// NOTE: This will CREATE the database file if it doesn't exist (via open_wallet_registry)
pub fn list_wallets() -> Result<Vec<WalletMeta>> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(WALLETS.read().clone());
    }
    // Try to load registry, but don't fail if it can't be opened
    // This allows checking if wallets exist before unlock
    match ensure_wallet_registry_loaded() {
        Ok(_) => Ok(WALLETS.read().clone()),
        Err(e) => {
            // If database can't be opened, check if file exists
            // If file doesn't exist, no wallets have been created yet
            let path = wallet_registry_path()?;
            if path.exists() {
                // File exists but can't be opened - likely wrong passphrase
                // Return error so caller knows something is wrong
                Err(e)
            } else {
                // File doesn't exist - no wallets created yet
                Ok(Vec::new())
            }
        }
    }
}

/// Switch active wallet
pub fn switch_wallet(wallet_id: WalletId) -> Result<()> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(());
    }
    ensure_wallet_registry_loaded()?;
    let wallets = WALLETS.read();
    if !wallets.iter().any(|w| w.id == wallet_id) {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    }

    *ACTIVE_WALLET.write() = Some(wallet_id);
    let registry_db = open_wallet_registry()?;
    set_active_wallet_registry(&registry_db, ACTIVE_WALLET.read().as_deref())?;
    if let Some(active) = ACTIVE_WALLET.read().clone() {
        touch_wallet_last_used(&registry_db, &active)?;
    }
    Ok(())
}

fn wallet_base_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PIRATE_WALLET_DB_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    if let Ok(path) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if path.contains("{wallet_id}") {
            let parent = Path::new(&path).parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }

        let parsed = PathBuf::from(&path);
        if parsed.extension().is_some() {
            let parent = parsed.parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }
        return Ok(parsed);
    }

    let base = ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join("wallets"))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base)
}

fn wallet_db_path_for(wallet_id: &str) -> Result<PathBuf> {
    if let Ok(template) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if template.contains("{wallet_id}") {
            return Ok(PathBuf::from(template.replace("{wallet_id}", wallet_id)));
        }
    }

    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.db", wallet_id)))
}

fn wallet_registry_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.db"))
}

fn app_passphrase() -> Result<String> {
    let passphrase =
        passphrase_store::get_passphrase().map_err(|e| anyhow!("App is locked: {}", e))?;
    Ok(passphrase.as_str().to_string())
}

fn wallet_registry_salt_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.salt"))
}

fn wallet_registry_key_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.dbkey"))
}

fn wallet_db_salt_path(wallet_id: &str) -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.salt", wallet_id)))
}

fn wallet_db_key_path(wallet_id: &str) -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.dbkey", wallet_id)))
}

fn load_salt(path: &Path) -> Result<[u8; 32]> {
    let data = fs::read(path)?;
    if data.len() != 32 {
        return Err(anyhow!("Invalid salt length in {}", path.display()));
    }
    let mut salt = [0u8; 32];
    salt.copy_from_slice(&data);
    Ok(salt)
}

fn write_salt(path: &Path, salt: &[u8; 32]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, salt)?;
    Ok(())
}

fn load_sealed_key(path: &Path) -> Result<Option<SealedKey>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path)?;
    Ok(Some(SealedKey::deserialize(&data)?))
}

fn store_sealed_key(path: &Path, sealed: &SealedKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, sealed.serialize())?;
    Ok(())
}

fn try_unseal_db_key(sealed: &SealedKey) -> Result<Option<EncryptionKey>> {
    let Some(keystore) = platform_keystore() else {
        return Ok(None);
    };

    match keystore.unseal_key(sealed) {
        KeystoreResult::Success(master) => {
            let key = EncryptionKey::from_bytes(*master.as_bytes());
            Ok(Some(key))
        }
        KeystoreResult::NotAvailable => Ok(None),
        KeystoreResult::Cancelled => Err(anyhow!("Keystore unlock cancelled")),
        KeystoreResult::AuthFailed => Err(anyhow!("Keystore authentication failed")),
        KeystoreResult::Error(e) => Err(anyhow!("Keystore error: {}", e)),
    }
}

fn maybe_store_sealed_db_key(key: &EncryptionKey, key_id: &str, sealed_path: &Path) -> Result<()> {
    let Some(keystore) = platform_keystore() else {
        return Ok(());
    };

    if sealed_path.exists() {
        return Ok(());
    }

    let master = MasterKey::from_bytes(key.as_bytes(), EncryptionAlgorithm::ChaCha20Poly1305)
        .map_err(|e| anyhow!("Failed to wrap db key: {}", e))?;
    let sealed = match keystore.seal_key(&master, key_id) {
        Ok(sealed) => sealed,
        Err(e) => {
            tracing::warn!("Failed to seal db key (keystore unavailable?): {}", e);
            return Ok(());
        }
    };
    if let Err(e) = store_sealed_key(sealed_path, &sealed) {
        tracing::warn!("Failed to persist sealed db key: {}", e);
    }
    Ok(())
}

fn force_store_sealed_db_key(
    key: &EncryptionKey,
    key_id: &str,
    sealed_path: &Path,
) -> Result<bool> {
    let Some(keystore) = platform_keystore() else {
        return Ok(false);
    };

    let master = MasterKey::from_bytes(key.as_bytes(), EncryptionAlgorithm::ChaCha20Poly1305)
        .map_err(|e| anyhow!("Failed to wrap db key: {}", e))?;
    let sealed = match keystore.seal_key(&master, key_id) {
        Ok(sealed) => sealed,
        Err(e) => {
            tracing::warn!("Failed to seal db key for reseal: {}", e);
            return Ok(false);
        }
    };
    if let Err(e) = store_sealed_key(sealed_path, &sealed) {
        tracing::warn!("Failed to persist resealed db key: {}", e);
        return Ok(false);
    }
    Ok(true)
}

fn reseal_registry_db_key(passphrase: &str) -> Result<bool> {
    let registry_path = wallet_registry_path()?;
    if !registry_path.exists() {
        return Ok(false);
    }

    let _db = open_wallet_registry_with_passphrase(passphrase)?;
    let salt_path = wallet_registry_salt_path()?;
    let key_path = wallet_registry_key_path()?;
    if !salt_path.exists() {
        return Ok(false);
    }
    let salt = load_salt(&salt_path)?;
    let key = derive_db_key(passphrase, &salt)?;
    force_store_sealed_db_key(&key, "pirate_wallet_registry_db", &key_path)
}

fn reseal_wallet_db_key(wallet_id: &str, passphrase: &str) -> Result<bool> {
    let (_db, key, _master_key) = open_wallet_db_with_passphrase(wallet_id, passphrase)?;
    let key_path = wallet_db_key_path(wallet_id)?;
    let key_id = format!("pirate_wallet_{}_db", wallet_id);
    force_store_sealed_db_key(&key, &key_id, &key_path)
}

fn derive_db_key(passphrase: &str, salt: &[u8; 32]) -> Result<EncryptionKey> {
    EncryptionKey::from_passphrase(passphrase, salt)
        .map_err(|e| anyhow!("Failed to derive db key: {}", e))
}

fn open_encrypted_db_with_migration(
    db_path: &Path,
    passphrase: &str,
    salt_path: &Path,
    sealed_key_path: &Path,
    legacy_key: Option<EncryptionKey>,
    master_key: &MasterKey,
    key_id: &str,
) -> Result<(Database, EncryptionKey)> {
    let db_exists = db_path.exists();
    let salt_exists = salt_path.exists();

    if salt_exists {
        let salt = load_salt(salt_path)?;
        let mut used_sealed = false;
        let key = match load_sealed_key(sealed_key_path)? {
            Some(sealed) => {
                if let Some(unsealed) = try_unseal_db_key(&sealed)? {
                    used_sealed = true;
                    unsealed
                } else {
                    derive_db_key(passphrase, &salt)?
                }
            }
            None => derive_db_key(passphrase, &salt)?,
        };

        match Database::open(db_path, &key, master_key.clone()) {
            Ok(db) => {
                maybe_store_sealed_db_key(&key, key_id, sealed_key_path)?;
                return Ok((db, key));
            }
            Err(e) if used_sealed => {
                let derived = derive_db_key(passphrase, &salt)?;
                let db = Database::open(db_path, &derived, master_key.clone()).map_err(|_| e)?;
                maybe_store_sealed_db_key(&derived, key_id, sealed_key_path)?;
                return Ok((db, derived));
            }
            Err(e) => return Err(e.into()),
        }
    }

    if db_exists {
        let legacy = legacy_key.ok_or_else(|| anyhow!("Legacy key not available"))?;
        let db = Database::open(db_path, &legacy, master_key.clone())?;
        let salt = generate_salt();
        let new_key = derive_db_key(passphrase, &salt)?;
        db.rekey(&new_key)?;
        if let Err(e) = write_salt(salt_path, &salt) {
            let _ = db.rekey(&legacy);
            return Err(e);
        }
        maybe_store_sealed_db_key(&new_key, key_id, sealed_key_path)?;
        return Ok((db, new_key));
    }

    let salt = generate_salt();
    write_salt(salt_path, &salt)?;
    let key = derive_db_key(passphrase, &salt)?;
    let db = Database::open(db_path, &key, master_key.clone())?;
    maybe_store_sealed_db_key(&key, key_id, sealed_key_path)?;
    Ok((db, key))
}

fn registry_master_key(passphrase: &str) -> Result<MasterKey> {
    let salt = Sha256::digest(b"wallet-registry");
    AppPassphrase::derive_key(passphrase, &salt[..16])
        .map_err(|e| anyhow!("Failed to derive registry master key: {}", e))
}

fn open_wallet_registry_with_passphrase(passphrase: &str) -> Result<Database> {
    let path = wallet_registry_path()?;
    let salt_path = wallet_registry_salt_path()?;
    let key_path = wallet_registry_key_path()?;
    let master_key = registry_master_key(passphrase)?;
    let legacy_key = EncryptionKey::from_legacy_password(passphrase);
    let key_id = "pirate_wallet_registry_db";

    let (db, _key) = open_encrypted_db_with_migration(
        &path,
        passphrase,
        &salt_path,
        &key_path,
        Some(legacy_key),
        &master_key,
        key_id,
    )?;

    ensure_wallet_registry_schema(&db)?;
    Ok(db)
}

fn open_wallet_registry() -> Result<Database> {
    let passphrase = app_passphrase()?;
    open_wallet_registry_with_passphrase(&passphrase)
}

fn ensure_wallet_registry_schema(db: &Database) -> Result<()> {
    db.conn().execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS wallet_registry (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            watch_only INTEGER NOT NULL,
            birthday_height INTEGER NOT NULL,
            network_type TEXT,
            last_used_at INTEGER,
            last_synced_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS wallet_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn get_registry_setting(db: &Database, key: &str) -> Result<Option<String>> {
    let value = db
        .conn()
        .query_row(
            "SELECT value FROM wallet_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(value)
}

fn set_registry_setting(db: &Database, key: &str, value: Option<&str>) -> Result<()> {
    if let Some(val) = value {
        // Use direct execute() calls instead of prepared statements
        // SQLCipher may have issues with prepared statement execute() returning results
        let conn = db.conn();

        // Delete any existing row
        conn.execute("DELETE FROM wallet_settings WHERE key = ?1", params![key])?;

        // Insert new row
        conn.execute(
            "INSERT INTO wallet_settings (key, value) VALUES (?1, ?2)",
            params![key, val],
        )?;
    } else {
        // Delete if value is None
        db.conn()
            .execute("DELETE FROM wallet_settings WHERE key = ?1", params![key])?;
    }
    Ok(())
}

async fn run_sync_engine_task<F, T>(sync: Arc<tokio::sync::Mutex<SyncEngine>>, task: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a mut SyncEngine) -> Pin<Box<dyn Future<Output = Result<T>> + 'a>>
        + Send
        + 'static,
    T: Send + 'static,
{
    let join = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow!("Failed to build sync runtime: {}", e))?;
        runtime.block_on(async move {
            let mut engine = sync.lock().await;
            task(&mut engine).await
        })
    });

    join.await
        .map_err(|e| anyhow!("Sync task join error: {}", e))?
}

async fn run_on_runtime<F, Fut, T>(task: F) -> Result<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<T> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow!("Failed to build runtime: {}", e))?;
            runtime.block_on(task())
        })();
        let _ = tx.send(result);
    });

    rx.await
        .map_err(|e| anyhow!("Runtime task join error: {}", e))?
}

fn parse_tunnel_mode_setting(mode: &str, socks5_url: Option<String>) -> Option<TunnelMode> {
    let normalized = mode.trim().to_lowercase();
    match normalized.as_str() {
        "tor" => Some(TunnelMode::Tor),
        "i2p" => Some(TunnelMode::I2p),
        "socks5" => {
            let url = socks5_url
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "socks5://localhost:1080".to_string());
            Some(TunnelMode::Socks5 { url })
        }
        "direct" => Some(TunnelMode::Direct),
        _ => None,
    }
}

fn load_registry_tunnel_mode(db: &Database) -> Result<Option<TunnelMode>> {
    let mode = get_registry_setting(db, REGISTRY_TUNNEL_MODE_KEY)?;
    let Some(mode_str) = mode else {
        return Ok(None);
    };
    let socks5_url = get_registry_setting(db, REGISTRY_TUNNEL_SOCKS5_URL_KEY)?;
    let parsed = parse_tunnel_mode_setting(&mode_str, socks5_url);
    if parsed.is_none() {
        tracing::warn!("Unknown tunnel mode setting: {}", mode_str);
    }
    Ok(parsed)
}

fn persist_registry_tunnel_mode(db: &Database, mode: &TunnelMode) -> Result<()> {
    let (mode_str, socks5_url) = match mode {
        TunnelMode::Tor => ("tor", None),
        TunnelMode::I2p => ("i2p", None),
        TunnelMode::Socks5 { url } => ("socks5", Some(url.as_str())),
        TunnelMode::Direct => ("direct", None),
    };
    set_registry_setting(db, REGISTRY_TUNNEL_MODE_KEY, Some(mode_str))?;
    set_registry_setting(db, REGISTRY_TUNNEL_SOCKS5_URL_KEY, socks5_url)?;
    Ok(())
}

fn redact_socks5_url(url: &str) -> String {
    if let Some(scheme_pos) = url.find("://") {
        let auth_start = scheme_pos + 3;
        if let Some(at_pos) = url[auth_start..].find('@') {
            let at_pos = auth_start + at_pos;
            let mut redacted = String::new();
            redacted.push_str(&url[..auth_start]);
            redacted.push_str("***@");
            redacted.push_str(&url[at_pos + 1..]);
            return redacted;
        }
    }
    url.to_string()
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn tunnel_transport_config_for(mode: &TunnelMode) -> (TransportMode, Option<String>, bool) {
    match mode {
        TunnelMode::Tor => (TransportMode::Tor, None, false),
        TunnelMode::I2p => (TransportMode::I2p, None, false),
        TunnelMode::Socks5 { url } => (TransportMode::Socks5, Some(url.clone()), false),
        TunnelMode::Direct => (TransportMode::Direct, None, true),
    }
}

fn tunnel_transport_config() -> (TransportMode, Option<String>, bool) {
    let tunnel_mode = TUNNEL_MODE.read().clone();
    tunnel_transport_config_for(&tunnel_mode)
}

fn spawn_bootstrap_transport(mode: TunnelMode) {
    let (transport, socks5_url, _) = tunnel_transport_config_for(&mode);
    let task = async move {
        if let Err(e) = pirate_sync_lightd::bootstrap_transport(transport, socks5_url).await {
            tracing::warn!("Failed to bootstrap transport: {}", e);
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(task);
    } else {
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Runtime::new() {
                runtime.block_on(task);
            }
        });
    }
}

/// Store app passphrase hash for local verification
///
/// IMPORTANT: This function opens/creates the database with the passphrase,
/// then stores the hash and caches the passphrase in memory for this session.
pub fn set_app_passphrase(passphrase: String) -> Result<()> {
    // Hash the passphrase for storage (validates strength)
    let app_passphrase = AppPassphrase::hash(&passphrase)
        .map_err(|e| anyhow!("Failed to hash passphrase: {}", e))?;

    // Now open the database (will be created with the correct passphrase)
    let registry_db = open_wallet_registry_with_passphrase(&passphrase)?;
    set_registry_setting(
        &registry_db,
        REGISTRY_APP_PASSPHRASE_KEY,
        Some(app_passphrase.hash_string()),
    )?;
    passphrase_store::set_passphrase(passphrase);
    Ok(())
}

/// Check if app passphrase is configured
pub fn has_app_passphrase() -> Result<bool> {
    Ok(wallet_registry_path()?.exists())
}

/// Verify app passphrase by attempting to open the database with it
pub fn verify_app_passphrase(passphrase: String) -> Result<bool> {
    // First check if database file exists
    let path = wallet_registry_path()?;
    if !path.exists() {
        // Database doesn't exist - can't verify passphrase
        return Err(anyhow!("Wallet registry database not found"));
    }

    // Try to open the registry database with this passphrase
    let result = match open_wallet_registry_with_passphrase(&passphrase) {
        Ok(db) => verify_app_passphrase_with_db(&db, &passphrase)?,
        Err(e) => {
            // Database couldn't be opened - passphrase is wrong or database is corrupted
            tracing::debug!("Failed to open database with provided passphrase: {}", e);
            false
        }
    };

    Ok(result)
}

fn verify_app_passphrase_with_db(db: &Database, passphrase: &str) -> Result<bool> {
    // Database opened successfully - verify the stored hash matches
    match get_registry_setting(db, REGISTRY_APP_PASSPHRASE_KEY) {
        Ok(Some(stored_hash)) => {
            let app_passphrase = AppPassphrase::from_hash(stored_hash);
            Ok(app_passphrase
                .verify(passphrase)
                .map_err(|e| anyhow!("Passphrase verification failed: {}", e))
                .unwrap_or(false))
        }
        Ok(None) => {
            // Hash not found - this shouldn't happen if database was created properly
            // But if database opened, passphrase is at least correct for decryption
            tracing::warn!(
                "App passphrase hash not found in database, but database opened successfully"
            );
            Ok(true)
        }
        Err(e) => {
            tracing::error!("Failed to read passphrase hash from database: {}", e);
            Ok(false)
        }
    }
}

/// Unlock app with passphrase (caches passphrase in memory for wallet access)
/// This allows wallets to be decrypted using the passphrase
pub fn unlock_app(passphrase: String) -> Result<()> {
    let path = wallet_registry_path()?;
    if !path.exists() {
        return Err(anyhow!("Wallet registry database not found"));
    }

    // Open registry once and verify passphrase against stored hash.
    let db = open_wallet_registry_with_passphrase(&passphrase)?;
    let is_valid = verify_app_passphrase_with_db(&db, &passphrase)?;
    if !is_valid {
        return Err(anyhow!("Invalid passphrase"));
    }

    {
        let vault = DECOY_VAULT.read();
        vault.deactivate_decoy();
    }

    // Cache passphrase in memory for wallet decryption
    passphrase_store::set_passphrase(passphrase);

    // Reload wallet registry with the correct passphrase
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    load_wallet_registry_state(&db)?;

    tracing::info!("App unlocked successfully");
    Ok(())
}

fn reencrypt_blob(old_key: &MasterKey, new_key: &MasterKey, blob: &[u8]) -> Result<Vec<u8>> {
    let plaintext = old_key
        .decrypt(blob)
        .map_err(|e| anyhow!("Failed to decrypt existing data: {}", e))?;
    new_key
        .encrypt(&plaintext)
        .map_err(|e| anyhow!("Failed to encrypt with new key: {}", e))
}

fn reencrypt_optional_blob(
    old_key: &MasterKey,
    new_key: &MasterKey,
    blob: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    match blob {
        Some(value) => Ok(Some(reencrypt_blob(old_key, new_key, &value)?)),
        None => Ok(None),
    }
}

fn reencrypt_wallet_tables(
    conn: &rusqlite::Connection,
    old_key: &MasterKey,
    new_key: &MasterKey,
) -> Result<()> {
    {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo FROM notes",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, Vec<u8>>(8)?,
                row.get::<_, Option<Vec<u8>>>(9)?,
                row.get::<_, Vec<u8>>(10)?,
                row.get::<_, Vec<u8>>(11)?,
                row.get::<_, Vec<u8>>(12)?,
                row.get::<_, Option<Vec<u8>>>(13)?,
                row.get::<_, Option<Vec<u8>>>(14)?,
                row.get::<_, Option<Vec<u8>>>(15)?,
            ))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (
            id,
            account_id,
            value,
            nullifier,
            commitment,
            spent,
            height,
            txid,
            output_index,
            spent_txid,
            diversifier,
            merkle_path,
            note,
            anchor,
            position,
            memo,
        ) in rows_cache
        {
            conn.execute(
                "UPDATE notes SET account_id = ?1, value = ?2, nullifier = ?3, commitment = ?4, spent = ?5, height = ?6, txid = ?7, output_index = ?8, spent_txid = ?9, diversifier = ?10, merkle_path = ?11, note = ?12, anchor = ?13, position = ?14, memo = ?15 WHERE id = ?16",
                params![
                    reencrypt_blob(old_key, new_key, &account_id)?,
                    reencrypt_blob(old_key, new_key, &value)?,
                    reencrypt_blob(old_key, new_key, &nullifier)?,
                    reencrypt_blob(old_key, new_key, &commitment)?,
                    reencrypt_blob(old_key, new_key, &spent)?,
                    reencrypt_blob(old_key, new_key, &height)?,
                    reencrypt_blob(old_key, new_key, &txid)?,
                    reencrypt_blob(old_key, new_key, &output_index)?,
                    reencrypt_optional_blob(old_key, new_key, spent_txid)?,
                    reencrypt_blob(old_key, new_key, &diversifier)?,
                    reencrypt_blob(old_key, new_key, &merkle_path)?,
                    reencrypt_blob(old_key, new_key, &note)?,
                    reencrypt_optional_blob(old_key, new_key, anchor)?,
                    reencrypt_optional_blob(old_key, new_key, position)?,
                    reencrypt_optional_blob(old_key, new_key, memo)?,
                    id,
                ],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT rowid, wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at FROM wallet_secrets",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, Option<Vec<u8>>>(7)?,
                row.get::<_, Option<Vec<u8>>>(8)?,
                row.get::<_, Vec<u8>>(9)?,
            ))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (
            row_id,
            wallet_id,
            account_id,
            extsk,
            dfvk,
            orchard_extsk,
            sapling_ivk,
            orchard_ivk,
            encrypted_mnemonic,
            created_at,
        ) in rows_cache
        {
            conn.execute(
                "UPDATE wallet_secrets SET wallet_id = ?1, account_id = ?2, extsk = ?3, dfvk = ?4, orchard_extsk = ?5, sapling_ivk = ?6, orchard_ivk = ?7, encrypted_mnemonic = ?8, created_at = ?9 WHERE rowid = ?10",
                params![
                    reencrypt_blob(old_key, new_key, &wallet_id)?,
                    reencrypt_blob(old_key, new_key, &account_id)?,
                    reencrypt_blob(old_key, new_key, &extsk)?,
                    reencrypt_optional_blob(old_key, new_key, dfvk)?,
                    reencrypt_optional_blob(old_key, new_key, orchard_extsk)?,
                    reencrypt_optional_blob(old_key, new_key, sapling_ivk)?,
                    reencrypt_optional_blob(old_key, new_key, orchard_ivk)?,
                    reencrypt_optional_blob(old_key, new_key, encrypted_mnemonic)?,
                    reencrypt_blob(old_key, new_key, &created_at)?,
                    row_id,
                ],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare("SELECT id, memo FROM memos")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (id, memo) in rows_cache {
            conn.execute(
                "UPDATE memos SET memo = ?1 WHERE id = ?2",
                params![reencrypt_blob(old_key, new_key, &memo)?, id],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare("SELECT height, frontier FROM frontier_snapshots")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (height, frontier) in rows_cache {
            conn.execute(
                "UPDATE frontier_snapshots SET frontier = ?1 WHERE height = ?2",
                params![reencrypt_blob(old_key, new_key, &frontier)?, height],
            )?;
        }
    }

    Ok(())
}

/// Change app passphrase and re-encrypt all wallet data with the new keys.
pub fn change_app_passphrase(current_passphrase: String, new_passphrase: String) -> Result<()> {
    // Validate new passphrase strength and current passphrase validity.
    AppPassphrase::validate(&new_passphrase)
        .map_err(|e| anyhow!("New passphrase does not meet requirements: {}", e))?;
    if !verify_app_passphrase(current_passphrase.clone())? {
        return Err(anyhow!("Invalid current passphrase"));
    }

    passphrase_store::set_passphrase(current_passphrase.clone());
    REGISTRY_LOADED.store(false, Ordering::SeqCst);

    struct WalletRekey {
        wallet_id: String,
        old_db_key: [u8; 32],
        old_master_key: MasterKey,
        new_master_key: MasterKey,
    }

    let rollback_wallets = |updated: &[WalletRekey]| -> Result<()> {
        for info in updated.iter().rev() {
            let (mut db, _key, _master_key) =
                open_wallet_db_with_passphrase(&info.wallet_id, &new_passphrase)?;
            let old_db_key = EncryptionKey::from_bytes(info.old_db_key);
            db.rekey(&old_db_key)?;
            let tx = db.transaction()?;
            reencrypt_wallet_tables(&tx, &info.new_master_key, &info.old_master_key)?;
            tx.commit()?;
            let key_path = wallet_db_key_path(&info.wallet_id)?;
            let key_id = format!("pirate_wallet_{}_db", info.wallet_id);
            let _ = force_store_sealed_db_key(&old_db_key, &key_id, &key_path);
        }
        Ok(())
    };

    let wallet_ids: Vec<String> = {
        ensure_wallet_registry_loaded()?;
        WALLETS.read().iter().map(|w| w.id.clone()).collect()
    };

    let registry_db = open_wallet_registry_with_passphrase(&current_passphrase)?;
    let registry_salt = load_salt(&wallet_registry_salt_path()?)?;
    let old_registry_key = derive_db_key(&current_passphrase, &registry_salt)?;
    let new_registry_key = derive_db_key(&new_passphrase, &registry_salt)?;
    let _ = registry_master_key(&new_passphrase)?;

    let mut updated_wallets = Vec::new();
    for wallet_id in &wallet_ids {
        let (mut db, old_db_key, old_master_key) =
            open_wallet_db_with_passphrase(wallet_id, &current_passphrase)?;
        let wallet_salt = load_salt(&wallet_db_salt_path(wallet_id)?)?;
        let new_db_key = derive_db_key(&new_passphrase, &wallet_salt)?;
        let new_master_key = wallet_master_key(wallet_id, &new_passphrase)?;

        if let Err(e) = db.rekey(&new_db_key) {
            let _ = rollback_wallets(&updated_wallets);
            return Err(anyhow!(
                "Failed to rekey wallet database {}: {}",
                wallet_id,
                e
            ));
        }

        let reencrypt_result: Result<()> = {
            let tx = db.transaction()?;
            if let Err(e) = reencrypt_wallet_tables(&tx, &old_master_key, &new_master_key) {
                let _ = tx.rollback();
                return Err(e);
            }
            tx.commit()
                .map_err(|e| anyhow!("Failed to commit re-encrypted wallet data: {}", e))?;
            Ok(())
        };

        if let Err(e) = reencrypt_result {
            let _ = db.rekey(&old_db_key);
            let _ = rollback_wallets(&updated_wallets);
            return Err(anyhow!(
                "Failed to re-encrypt wallet data {}: {}",
                wallet_id,
                e
            ));
        }

        let key_path = wallet_db_key_path(wallet_id)?;
        let key_id = format!("pirate_wallet_{}_db", wallet_id);
        let _ = force_store_sealed_db_key(&new_db_key, &key_id, &key_path);
        updated_wallets.push(WalletRekey {
            wallet_id: wallet_id.clone(),
            old_db_key: *old_db_key.as_bytes(),
            old_master_key,
            new_master_key,
        });
    }

    registry_db.rekey(&new_registry_key).map_err(|e| {
        let _ = rollback_wallets(&updated_wallets);
        anyhow!("Failed to rekey registry database: {}", e)
    })?;

    // Update registry passphrase hash after successful wallet updates.
    let new_hash = AppPassphrase::hash(&new_passphrase)
        .map_err(|e| anyhow!("Failed to hash new passphrase: {}", e))?;
    if let Err(e) = set_registry_setting(
        &registry_db,
        REGISTRY_APP_PASSPHRASE_KEY,
        Some(new_hash.hash_string()),
    ) {
        let _ = registry_db.rekey(&old_registry_key);
        let _ = rollback_wallets(&updated_wallets);
        return Err(anyhow!("Failed to update passphrase hash: {}", e));
    }

    if let Err(e) = refresh_duress_reverse_hash(&registry_db, &new_passphrase) {
        tracing::warn!("Failed to refresh duress passphrase: {}", e);
        let _ = set_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None);
        let _ = set_registry_setting(&registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None);
    }

    let registry_key_path = wallet_registry_key_path()?;
    let _ = force_store_sealed_db_key(
        &new_registry_key,
        "pirate_wallet_registry_db",
        &registry_key_path,
    );

    passphrase_store::set_passphrase(new_passphrase);
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    ensure_wallet_registry_loaded()?;

    // Clear sync sessions to force re-open with new keys.
    SYNC_SESSIONS.write().clear();
    tracing::info!("App passphrase updated successfully");
    Ok(())
}

/// Change passphrase using the cached passphrase from the current session.
pub fn change_app_passphrase_with_cached(new_passphrase: String) -> Result<()> {
    let current = app_passphrase()?;
    change_app_passphrase(current, new_passphrase)
}

/// Reseal registry + wallet DB keys using current platform keystore mode.
///
/// This is used when biometrics are enabled/disabled to rewrap the DB keys
/// under the appropriate keystore policy without changing the passphrase.
pub fn reseal_db_keys_for_biometrics() -> Result<()> {
    let passphrase = app_passphrase()?;
    let mut resealed = 0;

    if reseal_registry_db_key(&passphrase)? {
        resealed += 1;
    }

    ensure_wallet_registry_loaded()?;
    let wallet_ids: Vec<String> = WALLETS.read().iter().map(|w| w.id.clone()).collect();
    for wallet_id in wallet_ids {
        if reseal_wallet_db_key(&wallet_id, &passphrase)? {
            resealed += 1;
        }
    }

    tracing::info!(
        "Resealed {} database key(s) using current keystore mode",
        resealed
    );
    Ok(())
}

fn load_wallet_registry(db: &Database) -> Result<(Vec<WalletMeta>, Option<WalletId>)> {
    let mut wallets = Vec::new();
    let mut stmt = db.conn().prepare(
        "SELECT id, name, created_at, watch_only, birthday_height, network_type
         FROM wallet_registry
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WalletMeta {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            watch_only: row.get::<_, i64>(3)? != 0,
            birthday_height: row.get::<_, i64>(4)? as u32,
            network_type: row.get(5)?,
        })
    })?;
    for row in rows {
        wallets.push(row?);
    }

    let active_wallet_id = get_registry_setting(db, "active_wallet_id")?;

    Ok((wallets, active_wallet_id))
}

#[derive(Debug, Clone)]
struct WalletRegistryActivity {
    id: WalletId,
    last_used_at: Option<i64>,
    last_synced_at: Option<i64>,
}

fn load_wallet_registry_activity(db: &Database) -> Result<Vec<WalletRegistryActivity>> {
    let mut wallets = Vec::new();
    let mut stmt = db.conn().prepare(
        "SELECT id, last_used_at, last_synced_at
         FROM wallet_registry
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WalletRegistryActivity {
            id: row.get(0)?,
            last_used_at: row.get(1)?,
            last_synced_at: row.get(2)?,
        })
    })?;
    for row in rows {
        wallets.push(row?);
    }
    Ok(wallets)
}

fn persist_wallet_meta(db: &Database, meta: &WalletMeta) -> Result<()> {
    db.conn().execute(
        r#"
        INSERT INTO wallet_registry
            (id, name, created_at, watch_only, birthday_height, network_type, last_used_at, last_synced_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            watch_only = excluded.watch_only,
            birthday_height = excluded.birthday_height,
            network_type = excluded.network_type
        "#,
        params![
            meta.id,
            meta.name,
            meta.created_at,
            if meta.watch_only { 1 } else { 0 },
            meta.birthday_height as i64,
            meta.network_type,
        ],
    )?;
    Ok(())
}

fn delete_wallet_meta(db: &Database, wallet_id: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM wallet_registry WHERE id = ?1",
        params![wallet_id],
    )?;
    Ok(())
}

fn set_active_wallet_registry(db: &Database, wallet_id: Option<&str>) -> Result<()> {
    set_registry_setting(db, "active_wallet_id", wallet_id)
}

fn touch_wallet_last_used(db: &Database, wallet_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.conn().execute(
        "UPDATE wallet_registry SET last_used_at = ?1 WHERE id = ?2",
        params![now, wallet_id],
    )?;
    Ok(())
}

fn touch_wallet_last_synced(db: &Database, wallet_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.conn().execute(
        "UPDATE wallet_registry SET last_synced_at = ?1 WHERE id = ?2",
        params![now, wallet_id],
    )?;
    Ok(())
}

fn ensure_wallet_registry_loaded() -> Result<()> {
    if is_decoy_mode_active() {
        return Ok(());
    }
    if REGISTRY_LOADED.load(Ordering::SeqCst) {
        return Ok(());
    }

    let db = open_wallet_registry()?;
    load_wallet_registry_state(&db)
}

fn load_wallet_registry_state(db: &Database) -> Result<()> {
    let (wallets, active) = load_wallet_registry(db)?;
    *WALLETS.write() = wallets;
    *ACTIVE_WALLET.write() = active;

    // Hydrate per-wallet lightwalletd endpoints from registry settings.
    {
        let mut endpoints = LIGHTD_ENDPOINTS.write();
        endpoints.clear();

        for wallet in WALLETS.read().iter() {
            let endpoint_key = format!("lightd_endpoint_{}", wallet.id);
            let pin_key = format!("lightd_tls_pin_{}", wallet.id);
            let endpoint_url = get_registry_setting(db, &endpoint_key)?;
            let tls_pin = get_registry_setting(db, &pin_key)?;

            if let Some(url) = endpoint_url {
                match parse_endpoint_url(&url) {
                    Ok((host, port, use_tls)) => {
                        let endpoint = LightdEndpoint {
                            host,
                            port,
                            use_tls,
                            tls_pin,
                            label: Some("Custom".to_string()),
                        };
                        endpoints.insert(wallet.id.clone(), endpoint);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse stored endpoint for wallet {}: {}",
                            wallet.id,
                            e
                        );
                    }
                }
            }
        }
    }

    if let Ok(Some(mode)) = load_registry_tunnel_mode(db) {
        *TUNNEL_MODE.write() = mode;
    }

    if let Some(pending) = PENDING_TUNNEL_MODE.write().take() {
        if let Err(e) = persist_registry_tunnel_mode(db, &pending) {
            tracing::warn!("Failed to persist pending tunnel mode: {}", e);
            *PENDING_TUNNEL_MODE.write() = Some(pending);
        } else {
            *TUNNEL_MODE.write() = pending;
        }
    }

    REGISTRY_LOADED.store(true, Ordering::SeqCst);
    Ok(())
}

fn get_wallet_meta(wallet_id: &str) -> Result<WalletMeta> {
    if is_decoy_mode_active() {
        return Ok(decoy_wallet_meta());
    }
    ensure_wallet_registry_loaded()?;
    let wallets = WALLETS.read();
    wallets
        .iter()
        .find(|w| w.id == wallet_id)
        .cloned()
        .ok_or_else(|| anyhow!("Wallet not found"))
}

fn auto_consolidation_setting_key(wallet_id: &WalletId) -> String {
    format!("auto_consolidation_enabled_{}", wallet_id)
}

fn auto_consolidation_enabled(wallet_id: &WalletId) -> Result<bool> {
    if !wallet_registry_path()?.exists() {
        return Ok(false);
    }
    let registry_db = open_wallet_registry()?;
    let key = auto_consolidation_setting_key(wallet_id);
    let enabled = get_registry_setting(&registry_db, &key)?
        .map(|value| value == "true")
        .unwrap_or(false);
    Ok(enabled)
}

/// Get auto-consolidation setting for a wallet.
pub fn get_auto_consolidation_enabled(wallet_id: WalletId) -> Result<bool> {
    ensure_wallet_registry_loaded()?;
    auto_consolidation_enabled(&wallet_id)
}

/// Enable or disable auto-consolidation for a wallet.
pub fn set_auto_consolidation_enabled(wallet_id: WalletId, enabled: bool) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    let registry_db = open_wallet_registry()?;
    let key = auto_consolidation_setting_key(&wallet_id);
    let value = if enabled { Some("true") } else { None };
    set_registry_setting(&registry_db, &key, value)?;
    Ok(())
}

/// Get the note count threshold that triggers auto-consolidation prompts.
pub fn get_auto_consolidation_threshold() -> Result<u32> {
    Ok(AUTO_CONSOLIDATION_THRESHOLD as u32)
}

/// Count selectable notes eligible for auto-consolidation.
pub fn get_auto_consolidation_candidate_count(wallet_id: WalletId) -> Result<u32> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let selectable_notes =
        repo.get_unspent_selectable_notes_filtered(secret.account_id, None, None)?;
    let count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    Ok(count as u32)
}

/// Derive master key for field-level encryption from passphrase
fn wallet_master_key(wallet_id: &str, passphrase: &str) -> Result<MasterKey> {
    let salt = Sha256::digest(wallet_id.as_bytes());
    AppPassphrase::derive_key(passphrase, &salt[..16])
        .map_err(|e| anyhow!("Failed to derive master key: {}", e))
}

fn open_wallet_db_with_passphrase(
    wallet_id: &str,
    passphrase: &str,
) -> Result<(Database, EncryptionKey, MasterKey)> {
    let path = wallet_db_path_for(wallet_id)?;
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let path_str = path.to_string_lossy();
            let _ = writeln!(
                file,
                r#"{{"id":"log_db_path","timestamp":{},"location":"api.rs:954","message":"open_wallet_db_with_passphrase","data":{{"wallet_id":"{}","path":"{}","cwd":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, path_str, cwd
            );
        }
    }
    // #endregion
    let salt_path = wallet_db_salt_path(wallet_id)?;
    let key_path = wallet_db_key_path(wallet_id)?;
    let master_key = wallet_master_key(wallet_id, passphrase)?;
    let legacy_key = EncryptionKey::from_legacy_password(&format!("{}:{}", wallet_id, passphrase));
    let key_id = format!("pirate_wallet_{}_db", wallet_id);

    let (db, key) = open_encrypted_db_with_migration(
        &path,
        passphrase,
        &salt_path,
        &key_path,
        Some(legacy_key),
        &master_key,
        &key_id,
    )?;

    Ok((db, key, master_key))
}

fn wallet_db_keys(wallet_id: &str) -> Result<(EncryptionKey, MasterKey)> {
    let passphrase = app_passphrase()?;
    let (_db, key, master_key) = open_wallet_db_with_passphrase(wallet_id, &passphrase)?;
    Ok((key, master_key))
}

fn open_wallet_db_for(wallet_id: &str) -> Result<(&'static Database, Repository<'static>)> {
    let passphrase = app_passphrase()?;
    let (db, _key, _master_key) = open_wallet_db_with_passphrase(wallet_id, &passphrase)?;
    let db_ref: &'static Database = Box::leak(Box::new(db));
    Ok((db_ref, Repository::new(db_ref)))
}

fn ensure_primary_account_key(
    repo: &Repository,
    wallet_id: &str,
    secret: &WalletSecret,
) -> Result<i64> {
    let keys = repo.get_account_keys(secret.account_id)?;
    let meta = get_wallet_meta(wallet_id)?;
    if let Some(existing) = keys
        .iter()
        .find(|k| k.key_type == KeyType::Seed && k.key_scope == KeyScope::Account)
    {
        if let Some(id) = existing.id {
            if existing.birthday_height != meta.birthday_height as i64 {
                let mut updated = existing.clone();
                updated.birthday_height = meta.birthday_height as i64;
                let encrypted = repo.encrypt_account_key_fields(&updated)?;
                let _ = repo.upsert_account_key(&encrypted);
            }
            let _ = repo.backfill_address_key_id(secret.account_id, id);
            let _ = repo.backfill_note_key_id(id);
            return Ok(id);
        }
    }

    let sapling_extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)?;
    let dfvk_bytes = match secret.dfvk.as_ref() {
        Some(bytes) => Some(bytes.clone()),
        None => Some(sapling_extsk.to_extended_fvk().to_bytes()),
    };

    let orchard_fvk_bytes = match secret.orchard_extsk.as_ref() {
        Some(bytes) => {
            let extsk = OrchardExtendedSpendingKey::from_bytes(bytes)
                .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
            Some(extsk.to_extended_fvk().to_bytes())
        }
        None => None,
    };

    let key = AccountKey {
        id: None,
        account_id: secret.account_id,
        key_type: KeyType::Seed,
        key_scope: KeyScope::Account,
        label: Some("Seed".to_string()),
        birthday_height: meta.birthday_height as i64,
        created_at: chrono::Utc::now().timestamp(),
        spendable: true,
        sapling_extsk: Some(secret.extsk.clone()),
        sapling_dfvk: dfvk_bytes,
        orchard_extsk: secret.orchard_extsk.clone(),
        orchard_fvk: orchard_fvk_bytes,
        encrypted_mnemonic: secret.encrypted_mnemonic.clone(),
    };

    let encrypted_key = repo.encrypt_account_key_fields(&key)?;
    let key_id = repo
        .upsert_account_key(&encrypted_key)
        .map_err(|e| anyhow!(e.to_string()))?;
    let _ = repo.backfill_address_key_id(secret.account_id, key_id);
    let _ = repo.backfill_note_key_id(key_id);
    Ok(key_id)
}

/// Get active wallet ID
pub fn get_active_wallet() -> Result<Option<WalletId>> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(Some(DECOY_WALLET_ID.to_string()));
    }
    ensure_wallet_registry_loaded()?;
    if ACTIVE_WALLET.read().is_none() {
        if let Some(first) = WALLETS.read().first() {
            let id = first.id.clone();
            *ACTIVE_WALLET.write() = Some(id.clone());
            let registry_db = open_wallet_registry()?;
            set_active_wallet_registry(&registry_db, Some(&id))?;
            touch_wallet_last_used(&registry_db, &id)?;
        }
    }
    Ok(ACTIVE_WALLET.read().clone())
}

/// Rename wallet
pub fn rename_wallet(wallet_id: WalletId, new_name: String) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    let mut wallets = WALLETS.write();
    let Some(meta) = wallets.iter_mut().find(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    meta.name = new_name;

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, meta)?;
    Ok(())
}

/// Update wallet birthday height
pub fn set_wallet_birthday_height(wallet_id: WalletId, birthday_height: u32) -> Result<()> {
    if birthday_height == 0 {
        return Err(anyhow!("Invalid birthday height"));
    }
    ensure_wallet_registry_loaded()?;
    let mut wallets = WALLETS.write();
    let Some(meta) = wallets.iter_mut().find(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    meta.birthday_height = birthday_height;

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, meta)?;
    Ok(())
}

/// Delete wallet and its local database
pub fn delete_wallet(wallet_id: WalletId) -> Result<()> {
    ensure_wallet_registry_loaded()?;

    let mut wallets = WALLETS.write();
    let Some(index) = wallets.iter().position(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    wallets.remove(index);

    {
        let registry_db = open_wallet_registry()?;
        delete_wallet_meta(&registry_db, &wallet_id)?;
        let endpoint_key = format!("lightd_endpoint_{}", wallet_id);
        let pin_key = format!("lightd_tls_pin_{}", wallet_id);
        set_registry_setting(&registry_db, &endpoint_key, None)?;
        set_registry_setting(&registry_db, &pin_key, None)?;

        if ACTIVE_WALLET.read().as_ref() == Some(&wallet_id) {
            let next_active = wallets.first().map(|w| w.id.clone());
            *ACTIVE_WALLET.write() = next_active.clone();
            set_active_wallet_registry(&registry_db, next_active.as_deref())?;
            if let Some(active) = next_active {
                touch_wallet_last_used(&registry_db, &active)?;
            }
        }
    }

    {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(&wallet_id);
    }

    {
        let mut endpoints = LIGHTD_ENDPOINTS.write();
        endpoints.remove(&wallet_id);
    }

    let db_path = wallet_db_path_for(&wallet_id)?;
    let _ = fs::remove_file(db_path);
    let _ = fs::remove_file(wallet_db_salt_path(&wallet_id)?);
    let _ = fs::remove_file(wallet_db_key_path(&wallet_id)?);

    if wallets.is_empty() {
        *ACTIVE_WALLET.write() = None;
        passphrase_store::clear_passphrase();
        REGISTRY_LOADED.store(false, Ordering::SeqCst);

        let _ = fs::remove_file(wallet_registry_path()?);
        let _ = fs::remove_file(wallet_registry_salt_path()?);
        let _ = fs::remove_file(wallet_registry_key_path()?);
    }

    Ok(())
}

// ============================================================================
// Addresses
// ============================================================================

/// Helper: Determine if Orchard addresses should be generated based on network and height
fn orchard_activation_override(wallet_id: &WalletId) -> Result<Option<u32>> {
    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;

    // Special case: testnet endpoint that uses mainnet address prefixes.
    if endpoint.host == "64.23.167.130" && endpoint.port == 8067 {
        return Ok(Some(61));
    }

    Ok(None)
}

fn wallet_network_type(wallet_id: &WalletId) -> Result<NetworkType> {
    let wallet = get_wallet_meta(wallet_id)?;
    let network_type = match wallet.network_type.as_deref().unwrap_or("mainnet") {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };
    Ok(network_type)
}

fn address_prefix_network_type_for_endpoint(
    endpoint: &LightdEndpoint,
    default_network: NetworkType,
) -> NetworkType {
    if endpoint.host == "64.23.167.130" && endpoint.port == 8067 {
        return NetworkType::Mainnet;
    }
    default_network
}

fn address_prefix_network_type(wallet_id: &WalletId) -> Result<NetworkType> {
    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    let default_network = wallet_network_type(wallet_id)?;
    Ok(address_prefix_network_type_for_endpoint(
        &endpoint,
        default_network,
    ))
}

fn should_generate_orchard(wallet_id: &WalletId) -> Result<bool> {
    let wallet = get_wallet_meta(wallet_id)?;
    let network = Network::from_type(wallet_network_type(wallet_id)?);

    // Get current block height from sync state
    let (_db, _repo) = open_wallet_db_for(wallet_id)?;
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(_db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height as u32;
    let effective_height = if current_height == 0 {
        wallet.birthday_height
    } else {
        current_height
    };

    // Check if Orchard is activated at current height
    if let Some(override_height) = orchard_activation_override(wallet_id)? {
        return Ok(effective_height >= override_height);
    }

    Ok(network.is_orchard_active(effective_height))
}

/// Get current receive address for wallet
///
/// Returns the current diversified Sapling address from storage.
/// If no address exists, generates and stores the first address (index 0).
/// Call `next_receive_address` to rotate to a new unlinkable address.
pub fn current_receive_address(wallet_id: WalletId) -> Result<String> {
    if is_decoy_mode_active() {
        return Ok(String::new());
    }
    tracing::info!("Getting current receive address for wallet {}", wallet_id);

    // Open encrypted wallet DB
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id and derive address
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    // Load spending key
    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;

    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;

    // Get current diversifier index from database
    let current_index = repo.get_current_diversifier_index(secret.account_id, key_id)?;

    // Check if address already exists in database
    if let Some(addr_record) =
        repo.get_address_by_index(secret.account_id, key_id, current_index)?
    {
        tracing::debug!(
            "Found existing address at index {}: {}",
            current_index,
            addr_record.address
        );
        return Ok(addr_record.address);
    }

    // Determine if we should generate Orchard addresses
    let use_orchard = should_generate_orchard(&wallet_id)?;

    // Address doesn't exist, generate it
    let (addr_string, address_type) = if use_orchard {
        // Generate Orchard address
        let orchard_extsk_bytes = secret.orchard_extsk.ok_or_else(|| {
            anyhow!("Orchard key not found - wallet needs to be recreated with Orchard support")
        })?;
        let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(&orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;

        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let orchard_addr = orchard_fvk.address_at(current_index);

        let network_type = address_prefix_network_type(&wallet_id)?;
        let addr_string = orchard_addr.encode_for_network(network_type)?;
        (addr_string, AddressType::Orchard)
    } else {
        // Generate Sapling address
        let fvk = extsk.to_extended_fvk();
        let payment_addr = fvk.derive_address(current_index);
        let addr_string = payment_addr.encode();
        (addr_string, AddressType::Sapling)
    };

    // Store address in database
    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id: secret.account_id,
        diversifier_index: current_index,
        address: addr_string.clone(),
        address_type,
        label: None, // No label by default
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };
    repo.upsert_address(&address)?;

    tracing::debug!(
        "Generated and stored {} address at index {}: {}",
        if use_orchard { "Orchard" } else { "Sapling" },
        current_index,
        addr_string
    );
    Ok(addr_string)
}

/// Generate next receive address (diversifier rotation)
///
/// Increments the diversifier index to generate a fresh, unlinkable address.
/// Address type (Sapling or Orchard) is determined by network and current block height.
/// Previous addresses remain valid for receiving funds.
pub fn next_receive_address(wallet_id: WalletId) -> Result<String> {
    if is_decoy_mode_active() {
        return Ok(String::new());
    }
    tracing::info!("Generating next receive address for wallet {}", wallet_id);

    // Determine if we should generate Orchard addresses
    let use_orchard = should_generate_orchard(&wallet_id)?;

    // Open encrypted wallet DB
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id and derive address
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;

    // Get next diversifier index (current + 1)
    let next_index = repo.get_next_diversifier_index(secret.account_id, key_id)?;

    // Generate address based on network/height
    let (addr_string, address_type) = if use_orchard {
        // Generate Orchard address
        let orchard_extsk_bytes = secret.orchard_extsk.ok_or_else(|| {
            anyhow!("Orchard key not found - wallet needs to be recreated with Orchard support")
        })?;
        let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(&orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;

        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let orchard_addr = orchard_fvk.address_at(next_index);

        let network_type = address_prefix_network_type(&wallet_id)?;
        let addr_string = orchard_addr.encode_for_network(network_type)?;
        (addr_string, AddressType::Orchard)
    } else {
        // Generate Sapling address
        let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
            .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
        let fvk = extsk.to_extended_fvk();
        let payment_addr = fvk.derive_address(next_index);
        let addr_string = payment_addr.encode();
        (addr_string, AddressType::Sapling)
    };

    // Store address in database
    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id: secret.account_id,
        diversifier_index: next_index,
        address: addr_string.clone(),
        address_type,
        label: None, // No label by default
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };
    repo.upsert_address(&address)?;

    tracing::info!(
        "Generated and stored next {} address at index {}: {}",
        if use_orchard { "Orchard" } else { "Sapling" },
        next_index,
        addr_string
    );
    Ok(addr_string)
}

/// Label an address for address book
pub fn label_address(wallet_id: WalletId, addr: String, label: String) -> Result<()> {
    ensure_not_decoy("Label address")?;
    // Open encrypted wallet DB
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Update address label (empty string means remove label)
    let label_opt = if label.is_empty() {
        None
    } else {
        Some(label.clone())
    };

    repo.update_address_label(secret.account_id, &addr, label_opt)?;

    tracing::info!("Labeled address {} as '{}'", addr, label);
    Ok(())
}

/// Set color tag for a wallet address
pub fn set_address_color_tag(
    wallet_id: WalletId,
    addr: String,
    color_tag: AddressBookColorTag,
) -> Result<()> {
    ensure_not_decoy("Update address color")?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let db_tag = address_book_color_from_ffi(color_tag);
    repo.update_address_color_tag(secret.account_id, &addr, db_tag)?;

    tracing::info!("Updated address color tag for {}", addr);
    Ok(())
}

/// Get all addresses for wallet with labels
pub fn list_addresses(wallet_id: WalletId) -> Result<Vec<AddressInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    // Open encrypted wallet DB
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Load all addresses for this account
    let mut addresses = repo.get_all_addresses(secret.account_id)?;
    addresses.retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);

    // Convert to AddressInfo with labels
    let address_infos: Vec<AddressInfo> = addresses
        .into_iter()
        .map(|addr| AddressInfo {
            address: addr.address,
            diversifier_index: addr.diversifier_index,
            label: addr.label,
            created_at: addr.created_at,
            color_tag: address_book_color_to_ffi(addr.color_tag),
        })
        .collect();

    Ok(address_infos)
}

/// Get per-address balances for a wallet (optionally filtered by key group).
pub fn list_address_balances(
    wallet_id: WalletId,
    key_id: Option<i64>,
) -> Result<Vec<AddressBalanceInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let _ = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    let orchard_active = should_generate_orchard(&wallet_id)?;

    let mut notes = repo.get_unspent_notes(secret.account_id)?;
    for note in notes.iter_mut() {
        if note.address_id.is_some() {
            continue;
        }
        let Some(note_bytes) = note.note.as_deref() else {
            continue;
        };
        let address_string = match note.note_type {
            pirate_storage_sqlite::models::NoteType::Sapling => {
                decode_sapling_address_bytes_from_note_bytes(note_bytes)
                    .and_then(|bytes| SaplingPaymentAddress::from_bytes(&bytes))
                    .map(|addr| PaymentAddress { inner: addr }.encode_for_network(network_type))
            }
            pirate_storage_sqlite::models::NoteType::Orchard => {
                if !orchard_active {
                    None
                } else {
                    decode_orchard_address_bytes_from_note_bytes(note_bytes)
                        .and_then(|bytes| {
                            Option::from(OrchardAddress::from_raw_address_bytes(&bytes))
                        })
                        .and_then(|addr| {
                            OrchardPaymentAddress { inner: addr }
                                .encode_for_network(network_type)
                                .ok()
                        })
                }
            }
        };
        let Some(address_string) = address_string else {
            continue;
        };
        let address_type = match note.note_type {
            pirate_storage_sqlite::models::NoteType::Sapling => AddressType::Sapling,
            pirate_storage_sqlite::models::NoteType::Orchard => AddressType::Orchard,
        };
        let address_record = pirate_storage_sqlite::Address {
            id: None,
            key_id: note.key_id,
            account_id: secret.account_id,
            diversifier_index: 0,
            address: address_string.clone(),
            address_type,
            label: None,
            created_at: chrono::Utc::now().timestamp(),
            color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
            address_scope: pirate_storage_sqlite::AddressScope::External,
        };
        let _ = repo.upsert_address(&address_record);
        if let Some(addr) = repo
            .get_address_by_string(secret.account_id, &address_string)?
            .and_then(|addr| addr.id)
        {
            note.address_id = Some(addr);
            repo.update_note_by_id(note)?;
        }
    }

    let mut addresses = if let Some(id) = key_id {
        repo.get_addresses_by_key(secret.account_id, id)?
    } else {
        repo.get_all_addresses(secret.account_id)?
    };
    if !orchard_active {
        addresses.retain(|addr| addr.address_type != AddressType::Orchard);
    }
    addresses.retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);

    // Load sync height for confirmation depth calculations.
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height.max(sync_state.target_height);
    const MIN_DEPTH: u64 = 10;
    let confirmation_threshold = current_height.saturating_sub(MIN_DEPTH);

    let mut balances: HashMap<i64, (u64, u64, u64)> = HashMap::new();

    for note in notes {
        let Some(address_id) = note.address_id else {
            continue;
        };
        if note.value <= 0 {
            continue;
        }
        let value = match u64::try_from(note.value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let entry = balances.entry(address_id).or_insert((0, 0, 0));
        entry.0 = entry
            .0
            .checked_add(value)
            .ok_or_else(|| anyhow!("Balance overflow"))?;

        let note_height = note.height as u64;
        if note_height > 0 && note_height <= confirmation_threshold {
            entry.1 = entry
                .1
                .checked_add(value)
                .ok_or_else(|| anyhow!("Balance overflow"))?;
        } else {
            entry.2 = entry
                .2
                .checked_add(value)
                .ok_or_else(|| anyhow!("Balance overflow"))?;
        }
    }

    if let Ok(current_addr) = current_receive_address(wallet_id.clone()) {
        if let Some(current_id) = addresses
            .iter()
            .find(|addr| addr.address == current_addr)
            .and_then(|addr| addr.id)
        {
            if let Some((total, _spendable, _pending)) = balances.get(&current_id) {
                if *total > 0 {
                    let _ = next_receive_address(wallet_id.clone());
                    addresses = if let Some(id) = key_id {
                        repo.get_addresses_by_key(secret.account_id, id)?
                    } else {
                        repo.get_all_addresses(secret.account_id)?
                    };
                    if !orchard_active {
                        addresses.retain(|addr| addr.address_type != AddressType::Orchard);
                    }
                }
            }
        }
    }

    let infos = addresses
        .into_iter()
        .filter_map(|addr| {
            let id = addr.id?;
            let (total, spendable, pending) = balances.get(&id).copied().unwrap_or((0, 0, 0));
            Some(AddressBalanceInfo {
                address: addr.address,
                balance: total,
                spendable,
                pending,
                key_id: addr.key_id,
                address_id: id,
                label: addr.label,
                created_at: addr.created_at,
                color_tag: address_book_color_to_ffi(addr.color_tag),
                diversifier_index: addr.diversifier_index,
            })
        })
        .collect::<Vec<_>>();

    Ok(infos)
}

const SAPLING_NOTE_BYTES_VERSION: u8 = 1;
const ORCHARD_NOTE_BYTES_VERSION: u8 = 1;

fn decode_sapling_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == SAPLING_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
}

fn decode_orchard_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == ORCHARD_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
}

// ============================================================================
// Address Book
// ============================================================================

fn address_book_color_from_ffi(tag: AddressBookColorTag) -> DbColorTag {
    match tag {
        AddressBookColorTag::None => DbColorTag::None,
        AddressBookColorTag::Red => DbColorTag::Red,
        AddressBookColorTag::Orange => DbColorTag::Orange,
        AddressBookColorTag::Yellow => DbColorTag::Yellow,
        AddressBookColorTag::Green => DbColorTag::Green,
        AddressBookColorTag::Blue => DbColorTag::Blue,
        AddressBookColorTag::Purple => DbColorTag::Purple,
        AddressBookColorTag::Pink => DbColorTag::Pink,
        AddressBookColorTag::Gray => DbColorTag::Gray,
    }
}

fn address_book_color_to_ffi(tag: DbColorTag) -> AddressBookColorTag {
    match tag {
        DbColorTag::None => AddressBookColorTag::None,
        DbColorTag::Red => AddressBookColorTag::Red,
        DbColorTag::Orange => AddressBookColorTag::Orange,
        DbColorTag::Yellow => AddressBookColorTag::Yellow,
        DbColorTag::Green => AddressBookColorTag::Green,
        DbColorTag::Blue => AddressBookColorTag::Blue,
        DbColorTag::Purple => AddressBookColorTag::Purple,
        DbColorTag::Pink => AddressBookColorTag::Pink,
        DbColorTag::Gray => AddressBookColorTag::Gray,
    }
}

fn key_type_to_info(key_type: KeyType) -> KeyTypeInfo {
    match key_type {
        KeyType::Seed => KeyTypeInfo::Seed,
        KeyType::ImportSpend => KeyTypeInfo::ImportedSpending,
        KeyType::ImportView => KeyTypeInfo::ImportedViewing,
    }
}

fn sapling_extfvk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "zxviews",
        NetworkType::Testnet => "zxviewtestsapling",
        NetworkType::Regtest => "zxviewregtestsapling",
    }
}

fn sapling_extsk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "secret-extended-key-main",
        NetworkType::Testnet => "secret-extended-key-test",
        NetworkType::Regtest => "secret-extended-key-regtest",
    }
}

fn encode_sapling_xfvk_from_bytes(bytes: &[u8], network: NetworkType) -> Option<String> {
    if bytes.len() != 169 {
        return None;
    }
    let extfvk = SaplingExtendedFullViewingKey::read(&mut &bytes[..]).ok()?;
    Some(encode_extended_full_viewing_key(
        sapling_extfvk_hrp_for_network(network),
        &extfvk,
    ))
}

fn encode_orchard_extsk(
    extsk: &OrchardExtendedSpendingKey,
    network: NetworkType,
) -> Result<String> {
    let hrp = Hrp::parse(orchard_extsk_hrp_for_network(network))
        .map_err(|e| anyhow!("Invalid Orchard HRP: {}", e))?;
    bech32::encode::<Bech32>(hrp, &extsk.to_bytes())
        .map_err(|e| anyhow!("Bech32 encoding failed: {}", e))
}

fn parse_rfc3339_timestamp(value: &str) -> Result<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|e| anyhow!("Invalid timestamp: {}", e))?;
    Ok(parsed.timestamp())
}

fn address_book_entry_to_ffi(entry: DbAddressBookEntry) -> Result<AddressBookEntryFfi> {
    Ok(AddressBookEntryFfi {
        id: entry.id,
        wallet_id: entry.wallet_id,
        address: entry.address,
        label: entry.label,
        notes: entry.notes,
        color_tag: address_book_color_to_ffi(entry.color_tag),
        is_favorite: entry.is_favorite,
        created_at: parse_rfc3339_timestamp(&entry.created_at)?,
        updated_at: parse_rfc3339_timestamp(&entry.updated_at)?,
        last_used_at: match entry.last_used_at {
            Some(value) => Some(parse_rfc3339_timestamp(&value)?),
            None => None,
        },
        use_count: entry.use_count,
    })
}

/// List address book entries for a wallet
pub fn list_address_book(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;

    let mut entries = AddressBookStorage::list(db.conn(), &wallet_id)?;
    if wallet_id != "legacy" {
        if let Ok(mut legacy) = AddressBookStorage::list(db.conn(), "legacy") {
            entries.append(&mut legacy);
        }
    }

    entries.sort_by(|a, b| {
        if a.is_favorite != b.is_favorite {
            return if a.is_favorite {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.label.cmp(&b.label)
    });

    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

/// Add an address book entry
pub fn add_address_book_entry(
    wallet_id: WalletId,
    address: String,
    label: String,
    notes: Option<String>,
    color_tag: AddressBookColorTag,
) -> Result<AddressBookEntryFfi> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;

    let mut entry = DbAddressBookEntry::new(wallet_id.clone(), address, label);
    if let Some(notes_value) = notes {
        if !notes_value.is_empty() {
            entry = entry.with_notes(notes_value);
        }
    }
    entry = entry.with_color_tag(address_book_color_from_ffi(color_tag));

    let id = AddressBookStorage::insert(db.conn(), &entry)?;
    let stored = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found after insert"))?;
    address_book_entry_to_ffi(stored)
}

/// Update an address book entry
pub fn update_address_book_entry(
    wallet_id: WalletId,
    id: i64,
    label: Option<String>,
    notes: Option<String>,
    color_tag: Option<AddressBookColorTag>,
    is_favorite: Option<bool>,
) -> Result<AddressBookEntryFfi> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let mut entry = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found"))?;

    if let Some(label_value) = label {
        entry.label = label_value;
    }
    if let Some(notes_value) = notes {
        entry.notes = if notes_value.is_empty() {
            None
        } else {
            Some(notes_value)
        };
    }
    if let Some(tag) = color_tag {
        entry.color_tag = address_book_color_from_ffi(tag);
    }
    if let Some(favorite) = is_favorite {
        entry.is_favorite = favorite;
    }

    AddressBookStorage::update(db.conn(), &entry)?;
    let updated = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found after update"))?;
    address_book_entry_to_ffi(updated)
}

/// Delete an address book entry
pub fn delete_address_book_entry(wallet_id: WalletId, id: i64) -> Result<()> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::delete(db.conn(), &wallet_id, id)?;
    Ok(())
}

/// Toggle favorite status for an entry
pub fn toggle_address_book_favorite(wallet_id: WalletId, id: i64) -> Result<bool> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::toggle_favorite(db.conn(), &wallet_id, id)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

/// Mark an address as used
pub fn mark_address_used(wallet_id: WalletId, address: String) -> Result<()> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::mark_used(db.conn(), &wallet_id, &address)?;
    Ok(())
}

/// Get label for an address
pub fn get_label_for_address(wallet_id: WalletId, address: String) -> Result<Option<String>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::get_label_for_address(db.conn(), &wallet_id, &address)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

/// Check if an address exists in the book
pub fn address_exists_in_book(wallet_id: WalletId, address: String) -> Result<bool> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::exists(db.conn(), &wallet_id, &address)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

/// Count address book entries
pub fn get_address_book_count(wallet_id: WalletId) -> Result<u32> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::count(db.conn(), &wallet_id)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

/// Get entry by ID
pub fn get_address_book_entry(wallet_id: WalletId, id: i64) -> Result<Option<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let entry = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?;
    match entry {
        Some(value) => Ok(Some(address_book_entry_to_ffi(value)?)),
        None => Ok(None),
    }
}

/// Get entry by address
pub fn get_address_book_entry_by_address(
    wallet_id: WalletId,
    address: String,
) -> Result<Option<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let entry = AddressBookStorage::get_by_address(db.conn(), &wallet_id, &address)?;
    match entry {
        Some(value) => Ok(Some(address_book_entry_to_ffi(value)?)),
        None => Ok(None),
    }
}

/// Search entries by query
pub fn search_address_book(wallet_id: WalletId, query: String) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::search(db.conn(), &wallet_id, &query)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

/// List favorites
pub fn get_address_book_favorites(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::list_favorites(db.conn(), &wallet_id)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

/// List recently used addresses
pub fn get_recently_used_addresses(
    wallet_id: WalletId,
    limit: u32,
) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::recently_used(db.conn(), &wallet_id, limit)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

// ============================================================================
// Watch-Only
// ============================================================================

/// Export Sapling viewing key from full wallet.
///
/// Uses the zxviews... Bech32 format for watch-only wallets.
pub fn export_ivk(wallet_id: WalletId) -> Result<String> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export viewing key from watch-only wallet"));
    }

    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Derive xFVK from stored spending key
    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    Ok(extsk.to_xfvk_bech32_for_network(network_type))
}

/// Export Orchard Extended Full Viewing Key as Bech32 (for watch-only wallets)
///
/// Returns Bech32-encoded string with the network-specific HRP.
/// Uses the standard Orchard viewing key export format.
/// Use export_ivk() for Sapling viewing keys (zxviews... format).
pub fn export_orchard_viewing_key(wallet_id: WalletId) -> Result<String> {
    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    // Derive Orchard Extended FVK from stored Orchard spending key
    if let Some(orchard_extsk_bytes) = secret.orchard_extsk.as_ref() {
        let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        orchard_fvk
            .to_bech32_for_network(network_type)
            .map_err(|e| anyhow!("Failed to encode Orchard viewing key: {}", e))
    } else {
        Err(anyhow!("Orchard keys not available for this wallet"))
    }
}

/// Export legacy Orchard viewing key (returns hex-encoded 64 bytes) - DEPRECATED
///
/// Use export_orchard_viewing_key() instead for watch-only wallets.
/// This method is kept for backward compatibility.
#[deprecated(note = "Use export_orchard_viewing_key() instead")]
pub fn export_orchard_ivk(wallet_id: WalletId) -> Result<String> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export viewing key from watch-only wallet"));
    }

    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Derive legacy Orchard viewing key from stored Orchard spending key
    if let Some(orchard_extsk_bytes) = secret.orchard_extsk.as_ref() {
        let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let orchard_ivk_bytes = orchard_fvk.to_ivk_bytes();
        Ok(hex::encode(orchard_ivk_bytes))
    } else {
        Err(anyhow!("Orchard keys not available for this wallet"))
    }
}

/// Import viewing keys (watch-only wallet).
///
/// Supports Sapling viewing keys (zxviews...) and Orchard extended viewing keys (bech32).
/// If both are provided, creates a watch-only wallet that can view both Sapling and Orchard transactions.
pub fn import_ivk(
    name: String,
    sapling_ivk: Option<String>,
    orchard_ivk: Option<String>,
    birthday: u32,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;
    // Validate viewing keys by attempting to create wallet
    let _wallet = Wallet::from_ivks(sapling_ivk.as_deref(), orchard_ivk.as_deref())?;

    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: true,
        birthday_height: birthday,
        network_type: Some("mainnet".to_string()), // Default to mainnet, can be updated when endpoint is set
    };

    // Clone values before moving meta
    let account_name = meta.name.clone();
    let account_created_at = meta.created_at;

    WALLETS.write().push(meta.clone());
    *ACTIVE_WALLET.write() = Some(wallet_id.clone());

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, &meta)?;
    set_active_wallet_registry(&registry_db, Some(&wallet_id))?;
    touch_wallet_last_used(&registry_db, &wallet_id)?;

    // Store viewing keys in encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Create account for this wallet
    let account = Account {
        id: None,
        name: account_name,
        created_at: account_created_at,
    };
    let account_id = repo.insert_account(&account)?;

    // Store viewing keys in wallet_secret (watch-only wallets don't have mnemonic)
    let mut dfvk_bytes: Option<Vec<u8>> = None;
    if let Some(ref value) = sapling_ivk {
        let dfvk = ExtendedFullViewingKey::from_xfvk_bech32_any(value)
            .map_err(|_| anyhow!("Invalid Sapling viewing key (xFVK)"))?;
        dfvk_bytes = Some(dfvk.to_bytes());
    }

    let mut orchard_fvk_bytes: Option<Vec<u8>> = None;
    if let Some(ref value) = orchard_ivk {
        let fvk = OrchardExtendedFullViewingKey::from_bech32_any(value)
            .map_err(|_| anyhow!("Invalid Orchard viewing key"))?;
        orchard_fvk_bytes = Some(fvk.to_bytes());
    }

    let dfvk_bytes_for_key = dfvk_bytes.clone();
    let orchard_fvk_bytes_for_key = orchard_fvk_bytes.clone();

    let secret = WalletSecret {
        wallet_id: wallet_id.clone(),
        account_id,
        extsk: Vec::new(), // Empty for watch-only
        dfvk: dfvk_bytes,
        orchard_extsk: None, // Empty for watch-only
        sapling_ivk: None,
        orchard_ivk: orchard_fvk_bytes,
        encrypted_mnemonic: None, // Watch-only wallets don't have mnemonic
        created_at: account_created_at,
    };

    let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
    repo.upsert_wallet_secret(&encrypted_secret)?;

    let account_key = AccountKey {
        id: None,
        account_id,
        key_type: KeyType::ImportView,
        key_scope: KeyScope::Account,
        label: None,
        birthday_height: birthday as i64,
        created_at: account_created_at,
        spendable: false,
        sapling_extsk: None,
        sapling_dfvk: dfvk_bytes_for_key,
        orchard_extsk: None,
        orchard_fvk: orchard_fvk_bytes_for_key,
        encrypted_mnemonic: None,
    };
    let encrypted_key = repo.encrypt_account_key_fields(&account_key)?;
    let _ = repo.upsert_account_key(&encrypted_key)?;

    Ok(wallet_id)
}

// ============================================================================
// Key Management
// ============================================================================

/// List key groups for the active wallet account.
pub fn list_key_groups(wallet_id: WalletId) -> Result<Vec<KeyGroupInfo>> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let keys = repo.get_account_keys(secret.account_id)?;

    let mut items: Vec<KeyGroupInfo> = keys
        .into_iter()
        .filter_map(|key| {
            let id = key.id?;
            let has_sapling = key.sapling_extsk.is_some() || key.sapling_dfvk.is_some();
            let has_orchard = key.orchard_extsk.is_some() || key.orchard_fvk.is_some();
            Some(KeyGroupInfo {
                id,
                label: key.label,
                key_type: key_type_to_info(key.key_type),
                spendable: key.spendable,
                has_sapling,
                has_orchard,
                birthday_height: key.birthday_height,
                created_at: key.created_at,
            })
        })
        .collect();

    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(items)
}

/// Export viewing/spending keys for a specific key group.
pub fn export_key_group_keys(wallet_id: WalletId, key_id: i64) -> Result<KeyExportInfo> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;
    if key.account_id != secret.account_id {
        return Err(anyhow!("Key group does not belong to this wallet"));
    }

    let network_type = address_prefix_network_type(&wallet_id)?;

    let sapling_viewing_key = if let Some(ref bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
        Some(extsk.to_xfvk_bech32_for_network(network_type))
    } else if let Some(ref bytes) = key.sapling_dfvk {
        encode_sapling_xfvk_from_bytes(bytes, network_type)
    } else {
        None
    };

    let sapling_spending_key = if let Some(ref bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
        Some(encode_extended_spending_key(
            sapling_extsk_hrp_for_network(network_type),
            extsk.inner(),
        ))
    } else {
        None
    };

    let orchard_viewing_key = if let Some(ref bytes) = key.orchard_fvk {
        let fvk = OrchardExtendedFullViewingKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid Orchard viewing key bytes: {}", e))?;
        Some(
            fvk.to_bech32_for_network(network_type)
                .map_err(|e| anyhow!("Failed to encode Orchard viewing key: {}", e))?,
        )
    } else {
        None
    };

    let orchard_spending_key = if let Some(ref bytes) = key.orchard_extsk {
        let extsk = OrchardExtendedSpendingKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
        Some(encode_orchard_extsk(&extsk, network_type)?)
    } else {
        None
    };

    Ok(KeyExportInfo {
        key_id,
        sapling_viewing_key,
        orchard_viewing_key,
        sapling_spending_key,
        orchard_spending_key,
    })
}

/// List addresses for a specific key group.
pub fn list_addresses_for_key(wallet_id: WalletId, key_id: i64) -> Result<Vec<KeyAddressInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let mut addresses = repo.get_addresses_by_key(secret.account_id, key_id)?;
    addresses.retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);

    let infos = addresses
        .into_iter()
        .map(|addr| KeyAddressInfo {
            key_id,
            address: addr.address,
            diversifier_index: addr.diversifier_index,
            label: addr.label,
            created_at: addr.created_at,
            color_tag: address_book_color_to_ffi(addr.color_tag),
        })
        .collect();

    Ok(infos)
}

/// Generate a new address for a specific key group.
pub fn generate_address_for_key(
    wallet_id: WalletId,
    key_id: i64,
    use_orchard: bool,
) -> Result<String> {
    if use_orchard && !should_generate_orchard(&wallet_id)? {
        return Err(anyhow!("Orchard is not active for this wallet"));
    }
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;

    let account_id = key.account_id;
    let next_index = repo.get_next_diversifier_index(account_id, key_id)?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    let (addr_string, address_type) = if use_orchard {
        let fvk_bytes = key
            .orchard_fvk
            .as_ref()
            .ok_or_else(|| anyhow!("Orchard viewing key not available"))?;
        let fvk = OrchardExtendedFullViewingKey::from_bytes(fvk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard viewing key bytes: {}", e))?;
        let addr = fvk
            .address_at(next_index)
            .encode_for_network(network_type)?;
        (addr, AddressType::Orchard)
    } else {
        let dfvk_bytes = key
            .sapling_dfvk
            .as_ref()
            .ok_or_else(|| anyhow!("Sapling viewing key not available"))?;
        let dfvk = ExtendedFullViewingKey::from_bytes(dfvk_bytes)
            .ok_or_else(|| anyhow!("Invalid Sapling viewing key bytes"))?;
        let addr = dfvk
            .derive_address(next_index)
            .encode_for_network(network_type);
        (addr, AddressType::Sapling)
    };

    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id,
        diversifier_index: next_index,
        address: addr_string.clone(),
        address_type,
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };

    repo.upsert_address(&address)?;
    Ok(addr_string)
}

/// Import a spending key into an existing wallet.
pub fn import_spending_key(
    wallet_id: WalletId,
    sapling_key: Option<String>,
    orchard_key: Option<String>,
    label: Option<String>,
    birthday_height: u32,
) -> Result<i64> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    if sapling_key.is_none() && orchard_key.is_none() {
        return Err(anyhow!("Provide a Sapling or Orchard spending key"));
    }

    let wallet_network = wallet_network_type(&wallet_id)?;
    let mut sapling_extsk = None;
    let mut sapling_dfvk = None;
    let mut orchard_extsk = None;
    let mut orchard_fvk = None;
    let mut network_from_key: Option<NetworkType> = None;

    if let Some(value) = sapling_key.as_ref() {
        let (extsk, network) = ExtendedSpendingKey::from_bech32_any(value)
            .map_err(|e| anyhow!("Invalid Sapling spending key: {}", e))?;
        if network != wallet_network {
            return Err(anyhow!(
                "Sapling spending key network does not match wallet"
            ));
        }
        network_from_key = Some(network);
        sapling_dfvk = Some(extsk.to_extended_fvk().to_bytes());
        sapling_extsk = Some(extsk.to_bytes());
    }

    if let Some(value) = orchard_key.as_ref() {
        let (extsk, network) = OrchardExtendedSpendingKey::from_bech32_any(value)
            .map_err(|e| anyhow!("Invalid Orchard spending key: {}", e))?;
        if network != wallet_network {
            return Err(anyhow!(
                "Orchard spending key network does not match wallet"
            ));
        }
        if let Some(existing) = network_from_key {
            if existing != network {
                return Err(anyhow!(
                    "Sapling and Orchard keys are for different networks"
                ));
            }
        }
        orchard_fvk = Some(extsk.to_extended_fvk().to_bytes());
        orchard_extsk = Some(extsk.to_bytes());
    }

    let key = AccountKey {
        id: None,
        account_id: secret.account_id,
        key_type: KeyType::ImportSpend,
        key_scope: KeyScope::Account,
        label,
        birthday_height: birthday_height as i64,
        created_at: chrono::Utc::now().timestamp(),
        spendable: true,
        sapling_extsk,
        sapling_dfvk,
        orchard_extsk,
        orchard_fvk,
        encrypted_mnemonic: None,
    };

    let encrypted = repo.encrypt_account_key_fields(&key)?;
    repo.upsert_account_key(&encrypted)
        .map_err(|e| anyhow!(e.to_string()))
}

/// Export mnemonic seed (DANGEROUS - requires authentication)
///
/// This is a high-security operation that requires:
/// 1. Passphrase verification (Argon2id)
/// 2. Biometric confirmation (if available)
/// 3. Screenshot blocking is enabled
///
/// Use `export_seed_with_passphrase` for the gated flow.
///
/// Note: Only works for wallets created/restored from seed.
/// Wallets imported from private key or watch-only wallets cannot export seed.
pub fn export_seed(wallet_id: WalletId) -> Result<String> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }

    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Check if mnemonic is stored (wallet was created/restored from seed)
    let mnemonic_bytes = secret.encrypted_mnemonic.ok_or_else(|| {
        anyhow!("Seed not available. This wallet was imported from private key or is watch-only.")
    })?;

    // Decrypt mnemonic (database encryption handles decryption)
    let mnemonic = String::from_utf8(mnemonic_bytes)
        .map_err(|e| anyhow!("Failed to decode mnemonic: {}", e))?;

    tracing::info!("Seed exported for wallet {}", wallet_id);
    Ok(mnemonic)
}

// ============================================================================
// Send (Send-to-Many with per-output memos)
// ============================================================================

use pirate_core::{
    FeeCalculator, FeePolicy, NoteSelector, SelectionStrategy, DEFAULT_FEE, MAX_FEE,
    MAX_MEMO_LENGTH, MIN_FEE,
};

/// Maximum number of outputs per transaction
pub const MAX_OUTPUTS_PER_TX: usize = 50;
const AUTO_CONSOLIDATION_THRESHOLD: usize = 30;
const AUTO_CONSOLIDATION_MAX_EXTRA_NOTES: usize = 20;

fn normalize_filter_ids(ids: Option<Vec<i64>>) -> Option<Vec<i64>> {
    let values = ids?;
    let mut unique = HashSet::new();
    let mut normalized = Vec::new();
    for id in values {
        if unique.insert(id) {
            normalized.push(id);
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn validate_spendable_key(repo: &Repository, account_id: i64, key_id: i64) -> Result<()> {
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;
    if key.account_id != account_id {
        return Err(anyhow!("Key group does not belong to this wallet"));
    }
    if !key.spendable {
        return Err(anyhow!("Key group is not spendable"));
    }
    Ok(())
}

fn resolve_spend_key_id(
    repo: &Repository,
    account_id: i64,
    key_ids_filter: Option<&[i64]>,
    address_ids_filter: Option<&[i64]>,
) -> Result<Option<i64>> {
    let mut selected_key_id: Option<i64> = None;

    if let Some(ids) = key_ids_filter {
        if !ids.is_empty() {
            let unique: HashSet<i64> = ids.iter().copied().collect();
            if unique.len() > 1 {
                return Err(anyhow!(
                    "Multiple key groups are not supported in a single transaction"
                ));
            }
            let key_id = *unique.iter().next().unwrap();
            validate_spendable_key(repo, account_id, key_id)?;
            selected_key_id = Some(key_id);
        }
    }

    if let Some(address_ids) = address_ids_filter {
        if !address_ids.is_empty() {
            let addresses = repo.get_all_addresses(account_id)?;
            let mut address_key_ids = HashSet::new();
            for address_id in address_ids {
                let addr = addresses
                    .iter()
                    .find(|addr| addr.id == Some(*address_id))
                    .ok_or_else(|| anyhow!("Address {} not found", address_id))?;
                let key_id = addr
                    .key_id
                    .ok_or_else(|| anyhow!("Address {} is missing key id", address_id))?;
                address_key_ids.insert(key_id);
            }
            if address_key_ids.len() > 1 {
                return Err(anyhow!("Selected addresses span multiple key groups"));
            }
            if let Some(address_key_id) = address_key_ids.iter().next().copied() {
                validate_spendable_key(repo, account_id, address_key_id)?;
                if let Some(existing) = selected_key_id {
                    if existing != address_key_id {
                        return Err(anyhow!(
                            "Selected key group does not match selected addresses"
                        ));
                    }
                } else {
                    selected_key_id = Some(address_key_id);
                }
            }
        }
    }

    Ok(selected_key_id)
}

/// Build transaction with note selection, fee calculation, and change
///
/// Validates:
/// - All addresses are valid Sapling (zs1...)
/// - All amounts are non-zero
/// - All memos are valid UTF-8 and <= 512 bytes
/// - Sufficient funds available
///
/// Returns PendingTx with fee, change, and input information
fn build_tx_internal(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    tracing::info!(
        "Building transaction for wallet {} with {} outputs",
        wallet_id,
        outputs.len()
    );

    // Validate output count
    if outputs.is_empty() {
        return Err(anyhow!("At least one output is required"));
    }
    if outputs.len() > MAX_OUTPUTS_PER_TX {
        return Err(anyhow!(
            "Too many outputs: {} (maximum {})",
            outputs.len(),
            MAX_OUTPUTS_PER_TX
        ));
    }

    // Validate each output
    let mut has_memo = false;
    let mut total_amount = 0u64;

    for (i, output) in outputs.iter().enumerate() {
        // Validate output
        output
            .validate()
            .map_err(|e| anyhow!("Output {}: {}", i + 1, e))?;

        // Detect and validate address type (Sapling or Orchard)
        let is_orchard = output.addr.starts_with("pirate1")
            || output.addr.starts_with("pirate-test1")
            || output.addr.starts_with("pirate-regtest1");
        let is_sapling = output.addr.starts_with("zs1")
            || output.addr.starts_with("ztestsapling1")
            || output.addr.starts_with("zregtestsapling1");

        if !is_orchard && !is_sapling {
            return Err(anyhow!(
                "Invalid address at output {}: must be Sapling (zs1...) or Orchard (pirate1...) address",
                i + 1
            ));
        }

        // Decode address to validate (try both types)
        if is_orchard {
            OrchardPaymentAddress::decode_any_network(&output.addr)
                .map_err(|e| anyhow!("Invalid Orchard address at output {}: {}", i + 1, e))?;
        } else {
            PaymentAddress::decode_any_network(&output.addr)
                .map_err(|e| anyhow!("Invalid Sapling address at output {}: {}", i + 1, e))?;
        }

        // Validate memo if present
        if let Some(ref memo_text) = output.memo {
            let memo_bytes = memo_text.len();
            if memo_bytes > MAX_MEMO_LENGTH {
                return Err(anyhow!(
                    "Memo at output {} is too long: {} bytes (maximum {})",
                    i + 1,
                    memo_bytes,
                    MAX_MEMO_LENGTH
                ));
            }

            // Validate UTF-8 (Rust strings are already UTF-8, but check for control chars)
            if memo_text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
            {
                return Err(anyhow!(
                    "Memo at output {} contains invalid control characters",
                    i + 1
                ));
            }

            has_memo = true;
        }

        // Sum amounts
        total_amount = total_amount
            .checked_add(output.amount)
            .ok_or_else(|| anyhow!("Amount overflow"))?;
    }

    // Calculate fee (fixed for Pirate, or override)
    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(2, outputs.len(), has_memo)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);

    // Validate fee
    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);

    // Load selectable notes and perform note selection
    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let _resolved_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;

    let selectable_notes = repo.get_unspent_selectable_notes_filtered(
        secret.account_id,
        key_ids_filter.clone(),
        address_ids_filter.clone(),
    )?;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();
    let eligible_note_count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    let auto_consolidate = auto_consolidation_enabled(&wallet_id).unwrap_or(false)
        && key_ids_filter.is_none()
        && address_ids_filter.is_none()
        && eligible_note_count >= AUTO_CONSOLIDATION_THRESHOLD;
    let auto_consolidation_extra_limit = if auto_consolidate {
        AUTO_CONSOLIDATION_MAX_EXTRA_NOTES
    } else {
        0
    };
    let key_ids_count = key_ids_filter.as_ref().map_or(0, |ids| ids.len());
    let address_ids_count = address_ids_filter.as_ref().map_or(0, |ids| ids.len());
    let key_id_log = if key_ids_count == 1 {
        key_ids_filter.as_ref().unwrap()[0]
    } else {
        -1
    };
    let address_id_log = if address_ids_count == 1 {
        address_ids_filter.as_ref().unwrap()[0]
    } else {
        -1
    };
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_build_tx","timestamp":{},"location":"api.rs:1920","message":"build_tx notes","data":{{"wallet_id":"{}","account_id":{},"key_id":{},"address_id":{},"key_ids_count":{},"address_ids_count":{},"selectable_notes":{},"available_balance":{},"total_amount":{},"fee":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                wallet_id,
                secret.account_id,
                key_id_log,
                address_id_log,
                key_ids_count,
                address_ids_count,
                selectable_notes.len(),
                available_balance,
                total_amount,
                fee
            );
        }
    }
    // #endregion

    // Check if we have enough funds
    let required_total = total_amount
        .checked_add(fee)
        .ok_or_else(|| anyhow!("Amount + fee overflow"))?;
    if required_total > available_balance {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis, have {} arrrtoshis",
            required_total,
            available_balance
        ));
    }

    let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
    let selection = if auto_consolidation_extra_limit > 0 {
        selector
            .select_notes_with_consolidation(
                selectable_notes,
                total_amount,
                fee,
                auto_consolidation_extra_limit,
            )
            .map_err(|e| anyhow!("Note selection failed: {}", e))?
    } else {
        selector
            .select_notes(selectable_notes, total_amount, fee)
            .map_err(|e| anyhow!("Note selection failed: {}", e))?
    };
    let change = selection.change;

    // Get current height from sync state for expiry
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height as u32;
    let expiry_height = current_height.saturating_add(40); // ~40 blocks (~40 minutes)

    let pending = PendingTx {
        id: uuid::Uuid::new_v4().to_string(),
        outputs,
        total_amount,
        fee,
        change,
        input_total: selection.total_value,
        num_inputs: selection.notes.len() as u32,
        expiry_height,
        created_at: chrono::Utc::now().timestamp(),
    };

    tracing::info!(
        "Built pending tx {}: {} outputs, {} fee, {} change",
        pending.id,
        pending.outputs.len(),
        pending.fee,
        pending.change
    );

    Ok(pending)
}

/// Build transaction with note selection, fee calculation, and change.
pub fn build_tx(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    build_tx_internal(wallet_id, outputs, fee_opt, None, None)
}

/// Build transaction using notes from a specific key group.
pub fn build_tx_for_key(
    wallet_id: WalletId,
    key_id: i64,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    build_tx_internal(wallet_id, outputs, fee_opt, Some(vec![key_id]), None)
}

/// Build transaction using selected key groups or addresses.
pub fn build_tx_filtered(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    build_tx_internal(
        wallet_id,
        outputs,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )
}

/// Build a consolidation transaction for a key group.
pub fn build_consolidation_tx(
    wallet_id: WalletId,
    key_id: i64,
    target_address: String,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let selectable_notes =
        repo.get_unspent_selectable_notes_filtered(secret.account_id, Some(vec![key_id]), None)?;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();

    if available_balance == 0 {
        return Err(anyhow!("No spendable notes available for consolidation"));
    }

    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(1, 1, false)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);
    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    if available_balance <= fee {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis for fee, have {} arrrtoshis",
            fee,
            available_balance
        ));
    }

    let outputs = vec![Output {
        addr: target_address,
        amount: available_balance - fee,
        memo: None,
    }];

    build_tx_internal(wallet_id, outputs, Some(fee), Some(vec![key_id]), None)
}

/// Build a sweep transaction from selected key groups or addresses.
/// Sends the full available balance minus fee to the target address.
pub fn build_sweep_tx(
    wallet_id: WalletId,
    target_address: String,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);
    let _resolved_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;

    let selectable_notes = repo.get_unspent_selectable_notes_filtered(
        secret.account_id,
        key_ids_filter.clone(),
        address_ids_filter.clone(),
    )?;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();

    if available_balance == 0 {
        return Err(anyhow!("No spendable notes available for sweep"));
    }

    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(1, 1, false)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);
    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    if available_balance <= fee {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis for fee, have {} arrrtoshis",
            fee,
            available_balance
        ));
    }

    let outputs = vec![Output {
        addr: target_address,
        amount: available_balance - fee,
        memo: None,
    }];

    build_tx_internal(
        wallet_id,
        outputs,
        Some(fee),
        key_ids_filter,
        address_ids_filter,
    )
}

/// Sign pending transaction
///
/// Loads wallet from secure storage, performs note selection,
/// generates Sapling proofs, and signs the transaction.
fn sign_tx_internal(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    tracing::info!(
        "Signing transaction {} for wallet {}",
        pending.id,
        wallet_id
    );
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_sign_tx_start","timestamp":{},"location":"api.rs:2019","message":"sign_tx start","data":{{"wallet_id":"{}","pending_id":"{}","outputs":{},"total_amount":{},"fee":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts,
            wallet_id,
            pending.id,
            pending.outputs.len(),
            pending.total_amount,
            pending.fee
        );
    }
    // #endregion

    let log_step = |step: &str, detail: &str| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_sign_tx_step","timestamp":{},"location":"api.rs:2035","message":"sign_tx step","data":{{"wallet_id":"{}","step":"{}","detail":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                ts, wallet_id, step, detail
            );
        }
    };

    // Open encrypted wallet DB and load wallet secret + notes
    log_step("open_db_start", "");
    let (_db, repo) = open_wallet_db_for(&wallet_id).map_err(|e| {
        log_step("open_db_error", &format!("{:?}", e));
        e
    })?;
    log_step("open_db_ok", "");
    log_step("load_wallet_secret_start", "");
    let secret = repo.get_wallet_secret(&wallet_id)?.ok_or_else(|| {
        log_step("load_wallet_secret_error", "missing");
        anyhow!("No wallet secret found for {}", wallet_id)
    })?;
    log_step("load_wallet_secret_ok", "");

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);
    let signing_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;

    let (sapling_extsk_bytes, orchard_extsk_bytes, change_key_id) =
        if let Some(key_id) = signing_key_id {
            let key = repo
                .get_account_key_by_id(key_id)?
                .ok_or_else(|| anyhow!("Key group not found"))?;
            if key.account_id != secret.account_id {
                return Err(anyhow!("Key group does not belong to this wallet"));
            }
            if !key.spendable {
                return Err(anyhow!("Key group is not spendable"));
            }
            let sapling_bytes = key
                .sapling_extsk
                .clone()
                .ok_or_else(|| anyhow!("Sapling spending key missing for key group"))?;
            (sapling_bytes, key.orchard_extsk.clone(), key_id)
        } else {
            (
                secret.extsk.clone(),
                secret.orchard_extsk.clone(),
                ensure_primary_account_key(&repo, &wallet_id, &secret)?,
            )
        };

    let extsk = ExtendedSpendingKey::from_bytes(&sapling_extsk_bytes).map_err(|e| {
        log_step("extsk_parse_error", &format!("{:?}", e));
        anyhow!("Invalid spending key bytes: {}", e)
    })?;
    log_step("extsk_parse_ok", "");

    // Load Orchard spending key if available
    let orchard_extsk_opt = orchard_extsk_bytes
        .as_ref()
        .and_then(|bytes| OrchardExtendedSpendingKey::from_bytes(bytes).ok());

    // Load selectable notes for this account
    log_step("load_selectable_notes_start", "");
    let mut selectable_notes = repo
        .get_unspent_selectable_notes_filtered(
            secret.account_id,
            key_ids_filter.clone(),
            address_ids_filter.clone(),
        )
        .map_err(|e| {
            log_step("load_selectable_notes_error", &format!("{:?}", e));
            e
        })?;
    log_step(
        "load_selectable_notes_ok",
        &format!("notes={}", selectable_notes.len()),
    );
    if signing_key_id.is_some()
        && orchard_extsk_opt.is_none()
        && selectable_notes
            .iter()
            .any(|note| note.note_type == pirate_core::selection::NoteType::Orchard)
    {
        log_step("orchard_extsk_missing", "");
        return Err(anyhow!("Orchard spending key missing for this key group"));
    }

    // Compute Orchard witnesses for notes that need them
    // Access sync engine to get frontier for witness computation
    {
        let sessions = SYNC_SESSIONS.read();
        if let Some(session_arc) = sessions.get(&wallet_id) {
            // Try to get sync engine without blocking
            if let Ok(session) = session_arc.try_lock() {
                if let Some(ref sync) = session.sync {
                    if let Ok(engine) = sync.try_lock() {
                        // Compute witnesses for Orchard notes
                        for note in &mut selectable_notes {
                            if note.note_type == pirate_core::selection::NoteType::Orchard {
                                if let Some(position) = note.orchard_position {
                                    // Compute witness asynchronously
                                    let witness_result = futures::executor::block_on(
                                        engine.get_orchard_witness(position),
                                    );

                                    match witness_result {
                                        Ok(Some(merkle_path)) => {
                                            // Reference: builder_ffi.rs (use .into() to convert)
                                            // incrementalmerkletree::MerklePath to orchard::tree::MerklePath
                                            note.orchard_merkle_path = Some(merkle_path.into());
                                            tracing::debug!(
                                                "Computed Orchard witness for position {}",
                                                position
                                            );
                                        }
                                        Ok(None) => {
                                            tracing::warn!("No witness available for Orchard note at position {}", position);
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to compute Orchard witness for position {}: {}", position, e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // If we have Orchard notes, prefer an anchor derived from those notes'
    // merkle paths to avoid AnchorMismatch when the chain tip moves forward.
    let mut orchard_anchor_from_notes: Option<orchard::tree::Anchor> = None;
    let mut orchard_anchor_hex = String::new();
    for note in &mut selectable_notes {
        if note.note_type != pirate_core::selection::NoteType::Orchard {
            continue;
        }
        let merkle_path = match note.orchard_merkle_path.as_ref() {
            Some(path) => path,
            None => continue,
        };
        if note.commitment.len() != 32 {
            continue;
        }
        let mut cmx_bytes = [0u8; 32];
        cmx_bytes.copy_from_slice(&note.commitment[..32]);
        let cmx = Option::from(ExtractedNoteCommitment::from_bytes(&cmx_bytes));
        let cmx = match cmx {
            Some(value) => value,
            None => continue,
        };
        let anchor = merkle_path.root(cmx);
        orchard_anchor_from_notes = Some(anchor);
        orchard_anchor_hex = hex::encode(anchor.to_bytes());
        note.orchard_anchor = Some(anchor);
    }

    if orchard_anchor_from_notes.is_some() {
        log_step("orchard_anchor_from_notes_ok", &orchard_anchor_hex);
        // Filter Orchard notes to those that match the chosen anchor.
        selectable_notes.retain(|note| {
            if note.note_type != pirate_core::selection::NoteType::Orchard {
                return true;
            }
            match (&note.orchard_anchor, &orchard_anchor_from_notes) {
                (Some(a), Some(chosen)) => a.to_bytes() == chosen.to_bytes(),
                _ => false,
            }
        });
        log_step(
            "orchard_anchor_from_notes_filtered",
            &format!("notes={}", selectable_notes.len()),
        );
    }

    let eligible_note_count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    let auto_consolidation_extra_limit = if auto_consolidation_enabled(&wallet_id).unwrap_or(false)
        && key_ids_filter.is_none()
        && address_ids_filter.is_none()
        && eligible_note_count >= AUTO_CONSOLIDATION_THRESHOLD
    {
        AUTO_CONSOLIDATION_MAX_EXTRA_NOTES
    } else {
        0
    };

    let wallet_meta = get_wallet_meta(&wallet_id)?;
    let network_type_str = wallet_meta.network_type.as_deref().unwrap_or("mainnet");
    let network_type = match network_type_str {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };

    // Build outputs from PendingTx (detect address type)
    let mut builder = pirate_core::shielded_builder::ShieldedBuilder::with_network(network_type);
    builder.with_fee_per_action(pending.fee);
    if auto_consolidation_extra_limit > 0 {
        builder.with_auto_consolidation_extra_limit(auto_consolidation_extra_limit);
    }
    let mut has_orchard_output = false;

    for out in &pending.outputs {
        // Detect address type
        let is_orchard = out.addr.starts_with("pirate1")
            || out.addr.starts_with("pirate-test1")
            || out.addr.starts_with("pirate-regtest1");

        let memo = out
            .memo
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| pirate_core::memo::Memo::from_text_truncated(s.clone()));

        if is_orchard {
            has_orchard_output = true;
            let addr = OrchardPaymentAddress::decode_any_network(&out.addr)
                .map_err(|e| anyhow!("Invalid Orchard address {}: {}", out.addr, e))?;
            builder.add_orchard_output(addr.inner, out.amount, memo)?;
        } else {
            let addr = PaymentAddress::decode_any_network(&out.addr)
                .map_err(|e| anyhow!("Invalid Sapling address {}: {}", out.addr, e))?;
            builder.add_sapling_output(addr, out.amount, memo)?;
        }
    }

    let mut note_refs: Vec<&pirate_core::selection::SelectableNote> =
        selectable_notes.iter().collect();
    note_refs.sort_by(|a, b| a.value.cmp(&b.value));
    let required_total = pending
        .total_amount
        .checked_add(pending.fee)
        .ok_or_else(|| anyhow!("Amount + fee overflow"))?;
    let mut total_selected = 0u64;
    let mut extra_selected = 0usize;
    let mut has_orchard_spends = false;
    for note in note_refs {
        if total_selected < required_total {
            total_selected = total_selected
                .checked_add(note.value)
                .ok_or_else(|| anyhow!("Value overflow"))?;
            if note.note_type == pirate_core::selection::NoteType::Orchard {
                has_orchard_spends = true;
            }
            continue;
        }

        if auto_consolidation_extra_limit == 0 || extra_selected >= auto_consolidation_extra_limit {
            break;
        }

        if note.auto_consolidation_eligible {
            total_selected = total_selected
                .checked_add(note.value)
                .ok_or_else(|| anyhow!("Value overflow"))?;
            extra_selected += 1;
            if note.note_type == pirate_core::selection::NoteType::Orchard {
                has_orchard_spends = true;
            }
        }
    }
    let use_orchard_change = has_orchard_output || has_orchard_spends;

    // Target height from sync_state
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(_db);
    let sync_state = sync_storage.load_sync_state().map_err(|e| {
        log_step("load_sync_state_error", &format!("{:?}", e));
        e
    })?;
    let target_height = sync_state.local_height as u32;
    log_step(
        "load_sync_state_ok",
        &format!("target_height={}", target_height),
    );

    // Fetch Orchard anchor from frontier first, then lightwalletd if needed.
    let orchard_anchor_opt = if has_orchard_output {
        log_step("orchard_anchor_fetch_start", "");
        let mut anchor_opt: Option<orchard::tree::Anchor> = None;

        if let Some(anchor) = orchard_anchor_from_notes {
            anchor_opt = Some(anchor);
            log_step("orchard_anchor_from_notes_used", "");
        }

        let sessions = SYNC_SESSIONS.read();
        if let Some(session_arc) = sessions.get(&wallet_id) {
            if let Ok(session) = session_arc.try_lock() {
                if let Some(ref sync) = session.sync {
                    if let Ok(engine) = sync.try_lock() {
                        if let Some(anchor_bytes) =
                            futures::executor::block_on(engine.get_orchard_anchor())
                        {
                            anchor_opt = orchard::tree::Anchor::from_bytes(anchor_bytes).into();
                            if anchor_opt.is_some() {
                                log_step("orchard_anchor_frontier_ok", "");
                            } else {
                                log_step("orchard_anchor_frontier_none", "");
                            }
                        } else {
                            log_step("orchard_anchor_frontier_none", "");
                        }
                    }
                }
            }
        }
        if anchor_opt.is_none() {
            log_step("orchard_anchor_frontier_missing", "");
        }

        if anchor_opt.is_none() {
            let snapshot_anchor = FrontierStorage::new(_db)
                .load_last_snapshot()
                .ok()
                .flatten()
                .and_then(|(_height, bytes)| decode_frontier_snapshot(&bytes).ok())
                .and_then(|(_sapling, orchard)| {
                    if orchard.is_empty() {
                        None
                    } else {
                        let hex_str = hex::encode(orchard);
                        orchard_anchor_from_frontier_hex(&hex_str).ok().flatten()
                    }
                });
            if snapshot_anchor.is_some() {
                anchor_opt = snapshot_anchor;
                log_step("orchard_anchor_snapshot_ok", "");
            } else {
                log_step("orchard_anchor_snapshot_none", "");
            }
        }

        if anchor_opt.is_none() {
            let endpoint = get_lightd_endpoint(wallet_id.clone())?;
            let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();

            let client_config = LightClientConfig {
                endpoint,
                transport,
                socks5_url,
                tls: TlsConfig::default(),
                retry: RetryConfig::default(),
                connect_timeout: std::time::Duration::from_secs(30),
                request_timeout: std::time::Duration::from_secs(60),
                allow_direct_fallback,
            };
            let client = LightClient::with_config(client_config);

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow!("Failed to build runtime: {}", e))?;

            let anchor_result = rt.block_on(async move {
                let fetch_height = target_height as u64;
                let fetch = async move {
                    client.connect().await.ok()?;
                    let tree_state = match client.get_bridge_tree_state(fetch_height).await {
                        Ok(ts) => ts,
                        Err(_) => client.get_tree_state(fetch_height).await.ok()?,
                    };

                    if tree_state.orchard_tree.is_empty() {
                        return None;
                    }

                    orchard_anchor_from_frontier_hex(&tree_state.orchard_tree)
                        .ok()
                        .flatten()
                };

                match tokio::time::timeout(std::time::Duration::from_secs(20), fetch).await {
                    Ok(result) => Ok(result),
                    Err(_) => Err(anyhow!("timeout")),
                }
            });

            match anchor_result {
                Ok(anchor) => {
                    anchor_opt = anchor;
                    let detail = if anchor_opt.is_some() {
                        format!("some@{}", target_height)
                    } else {
                        format!("none@{}", target_height)
                    };
                    log_step("orchard_anchor_fetch_ok", &detail);
                }
                Err(e) => {
                    log_step("orchard_anchor_fetch_error", &format!("{}", e));
                }
            }
        } else {
            log_step("orchard_anchor_fetch_ok", "some");
        }

        if anchor_opt.is_none() {
            log_step("orchard_anchor_fetch_error", "missing");
            return Err(anyhow!("Failed to fetch Orchard anchor"));
        }

        anchor_opt
    } else {
        None
    };

    // Get next diversifier index for internal (change) output
    let next_diversifier_index = repo
        .get_next_diversifier_index_for_scope(
            secret.account_id,
            change_key_id,
            pirate_storage_sqlite::AddressScope::Internal,
        )
        .map_err(|e| {
            log_step("next_diversifier_error", &format!("{:?}", e));
            e
        })?;
    log_step(
        "next_diversifier_ok",
        &format!("{}", next_diversifier_index),
    );

    if pending.change > 10_000 {
        let (change_addr, address_type) = if use_orchard_change {
            let orchard_extsk = orchard_extsk_opt
                .as_ref()
                .ok_or_else(|| anyhow!("Orchard spending key required for Orchard change"))?;
            let orchard_fvk = orchard_extsk.to_extended_fvk();
            let addr = orchard_fvk
                .address_at_internal(next_diversifier_index)
                .encode_for_network(network_type)?;
            (addr, AddressType::Orchard)
        } else {
            let addr = extsk
                .to_internal_fvk()
                .derive_address(next_diversifier_index)
                .encode_for_network(network_type);
            (addr, AddressType::Sapling)
        };

        let address = pirate_storage_sqlite::Address {
            id: None,
            key_id: Some(change_key_id),
            account_id: secret.account_id,
            diversifier_index: next_diversifier_index,
            address: change_addr,
            address_type,
            label: None,
            created_at: chrono::Utc::now().timestamp(),
            color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
            address_scope: pirate_storage_sqlite::AddressScope::Internal,
        };
        let _ = repo.upsert_address(&address);
    }

    log_step("build_and_sign_start", "");
    let (build_tx, build_rx) = std::sync::mpsc::channel();
    let wallet_id_for_log = wallet_id.clone();
    let build_timeout = std::time::Duration::from_secs(120);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            futures::executor::block_on(builder.build_and_sign(
                &extsk,
                orchard_extsk_opt.as_ref(),
                selectable_notes,
                target_height,
                orchard_anchor_opt,
                next_diversifier_index,
            ))
            .map_err(|e| anyhow!("Build/sign failed: {}", e))
        }));
        let send_result: anyhow::Result<_> = match result {
            Ok(build_result) => build_result,
            Err(panic_payload) => {
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                Err(anyhow!("build_and_sign panicked: {}", panic_msg))
            }
        };
        let _ = build_tx.send(send_result);
    });

    let signed_core = match build_rx.recv_timeout(build_timeout) {
        Ok(Ok(core)) => core,
        Ok(Err(e)) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign failed","data":{{"wallet_id":"{}","error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log, e
                );
            }
            return Err(anyhow!("Build/sign failed: {}", e));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign timeout","data":{{"wallet_id":"{}","timeout_secs":120}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log
                );
            }
            return Err(anyhow!("Build/sign timed out after 120s"));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign failed","data":{{"wallet_id":"{}","error":"channel disconnected"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log
                );
            }
            return Err(anyhow!("Build/sign failed: channel disconnected"));
        }
    };

    tracing::info!(
        "Signed transaction {}: {} bytes",
        signed_core.txid,
        signed_core.size
    );
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_sign_tx_ok","timestamp":{},"location":"api.rs:2222","message":"sign_tx ok","data":{{"wallet_id":"{}","txid":"{}","size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, wallet_id, signed_core.txid, signed_core.size
        );
    }
    // #endregion

    Ok(SignedTx {
        txid: signed_core.txid.to_string(),
        raw: signed_core.raw_tx,
        size: signed_core.size,
    })
}

/// Sign pending transaction (all spendable notes in the wallet)
pub fn sign_tx(wallet_id: WalletId, pending: PendingTx) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    sign_tx_internal(wallet_id, pending, None, None)
}

/// Sign pending transaction using notes from a specific key group
pub fn sign_tx_for_key(wallet_id: WalletId, pending: PendingTx, key_id: i64) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    sign_tx_internal(wallet_id, pending, Some(vec![key_id]), None)
}

/// Sign pending transaction using selected key groups or addresses.
pub fn sign_tx_filtered(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    sign_tx_internal(wallet_id, pending, key_ids_filter, address_ids_filter)
}

/// Broadcast signed transaction to the network
///
/// Sends transaction via lightwalletd gRPC SendTransaction.
/// Returns TxId on success, or error with details.
pub async fn broadcast_tx(signed: SignedTx) -> Result<TxId> {
    ensure_not_decoy("Broadcast transaction")?;
    run_on_runtime(move || broadcast_tx_inner(signed)).await
}

async fn broadcast_tx_inner(signed: SignedTx) -> Result<TxId> {
    tracing::info!("Broadcasting transaction {}", signed.txid);
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_broadcast_start","timestamp":{},"location":"api.rs:2233","message":"broadcast start","data":{{"txid":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, signed.txid
        );
    }
    // #endregion

    // Get active wallet for endpoint
    let wallet_id = get_active_wallet()?.ok_or_else(|| anyhow!("No active wallet"))?;

    // Get lightwalletd endpoint configuration
    let endpoint_config = get_lightd_endpoint_config(wallet_id)?;
    let endpoint_url = endpoint_config.url();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    let client_config = LightClientConfig {
        endpoint: endpoint_url.clone(),
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    // Create lightwalletd client and broadcast
    let client = pirate_sync_lightd::LightClient::with_config(client_config);
    if let Err(e) = client.connect().await {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_broadcast_connect_error","timestamp":{},"location":"api.rs:2212","message":"broadcast connect failed","data":{{"endpoint":"{}","error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                ts, endpoint_url, e
            );
        }
        return Err(anyhow!("Failed to connect to {}: {}", endpoint_url, e));
    }

    let txid_hex = client
        .broadcast(signed.raw.clone())
        .await
        .map_err(|e| {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_broadcast_error","timestamp":{},"location":"api.rs:2226","message":"broadcast failed","data":{{"txid":"{}","endpoint":"{}","error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts,
                    signed.txid,
                    endpoint_url,
                    e
                );
            }
            anyhow!("Broadcast failed: {}", e)
        })?;

    tracing::info!(
        "Broadcast to {} succeeded: {} ({} bytes)",
        endpoint_url,
        txid_hex,
        signed.size
    );
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_broadcast_ok","timestamp":{},"location":"api.rs:2263","message":"broadcast ok","data":{{"txid":"{}","endpoint":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, signed.txid, endpoint_url
        );
    }
    // #endregion

    Ok(signed.txid)
}

/// Estimate fee for transaction without building it
pub fn estimate_fee(num_outputs: usize, has_memo: bool, fee_policy: Option<String>) -> Result<u64> {
    let calculator = FeeCalculator::new();
    let estimated_inputs = num_outputs.div_ceil(2);

    let base_fee = calculator
        .calculate_fee(estimated_inputs, num_outputs, has_memo)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;

    // Apply fee policy
    let policy = match fee_policy.as_deref() {
        Some("low") => FeePolicy::Low,
        Some("high") => FeePolicy::High,
        Some("standard") | None => FeePolicy::Standard,
        Some(custom) => {
            let fee: u64 = custom
                .parse()
                .map_err(|_| anyhow!("Invalid fee: {}", custom))?;
            FeePolicy::Custom(fee)
        }
    };

    let fee = policy.apply(base_fee);
    Ok(fee.clamp(MIN_FEE, MAX_FEE))
}

/// Get fee information
pub fn get_fee_info() -> Result<FeeInfo> {
    Ok(FeeInfo {
        default_fee: DEFAULT_FEE,
        min_fee: MIN_FEE,
        max_fee: MAX_FEE,
        fee_per_output: 0,
        memo_fee_multiplier: 1.0,
    })
}

/// Fee information for UI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeeInfo {
    /// Default fee (fixed)
    pub default_fee: u64,
    /// Minimum allowed fee
    pub min_fee: u64,
    /// Maximum allowed fee
    pub max_fee: u64,
    /// Additional fee per output (fixed fee uses 0)
    pub fee_per_output: u64,
    /// Fee multiplier when memo is included (fixed fee uses 1.0)
    pub memo_fee_multiplier: f64,
}

// ============================================================================
// Sync
// ============================================================================
use pirate_sync_lightd::{PerfCounters, SyncConfig, SyncEngine, SyncProgress};
use tokio::sync::Mutex;

fn map_stage(stage: pirate_sync_lightd::SyncStage) -> crate::models::SyncStage {
    match stage {
        pirate_sync_lightd::SyncStage::Headers => crate::models::SyncStage::Headers,
        pirate_sync_lightd::SyncStage::Notes => crate::models::SyncStage::Notes,
        pirate_sync_lightd::SyncStage::Witness => crate::models::SyncStage::Witness,
        pirate_sync_lightd::SyncStage::Verify => crate::models::SyncStage::Verify,
        pirate_sync_lightd::SyncStage::Complete => crate::models::SyncStage::Verify, // Complete maps to Verify for FFI compatibility
    }
}

fn decode_frontier_snapshot(bytes: &[u8]) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    const FRONTIER_SNAPSHOT_MAGIC: [u8; 4] = *b"PFRT";
    const FRONTIER_SNAPSHOT_VERSION: u8 = 1;

    if bytes.len() < FRONTIER_SNAPSHOT_MAGIC.len() + 1
        || !bytes.starts_with(&FRONTIER_SNAPSHOT_MAGIC)
    {
        return Ok((bytes.to_vec(), Vec::new()));
    }

    let version = bytes[FRONTIER_SNAPSHOT_MAGIC.len()];
    if version != FRONTIER_SNAPSHOT_VERSION {
        return Err(anyhow::anyhow!(
            "Unsupported frontier snapshot version: {}",
            version
        ));
    }

    let mut offset = FRONTIER_SNAPSHOT_MAGIC.len() + 1;
    if bytes.len() < offset + 4 {
        return Err(anyhow::anyhow!("Frontier snapshot truncated"));
    }
    let sapling_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    if bytes.len() < offset + sapling_len + 4 {
        return Err(anyhow::anyhow!("Frontier snapshot truncated"));
    }
    let sapling_bytes = bytes[offset..offset + sapling_len].to_vec();
    offset += sapling_len;
    let orchard_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    if bytes.len() < offset + orchard_len {
        return Err(anyhow::anyhow!("Frontier snapshot truncated"));
    }
    let orchard_bytes = bytes[offset..offset + orchard_len].to_vec();
    Ok((sapling_bytes, orchard_bytes))
}

fn orchard_anchor_from_frontier_hex(
    frontier_hex: &str,
) -> anyhow::Result<Option<orchard::tree::Anchor>> {
    if frontier_hex.is_empty() {
        return Ok(None);
    }
    let bytes = match hex::decode(frontier_hex) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };

    let frontier = if let Ok(f) = read_frontier_v1::<MerkleHashOrchard, _>(&bytes[..]) {
        f
    } else {
        read_frontier_v0::<MerkleHashOrchard, _>(&bytes[..])?
    };

    let mut orchard_frontier = OrchardFrontier::new();
    orchard_frontier.init_from_frontier(frontier);
    let root_bytes = orchard_frontier.root();
    Ok(root_bytes.and_then(|b| orchard::tree::Anchor::from_bytes(b).into()))
}

lazy_static::lazy_static! {
    /// Active sync sessions per wallet
    //
    // IMPORTANT: `SyncEngine` is not `Send + Sync` (it holds a rusqlite-backed storage sink),
    // so we store sessions in a `parking_lot::RwLock` and never move them across threads.
    // FRB calls are handled on a single thread by default.
    static ref SYNC_SESSIONS: Arc<RwLock<HashMap<WalletId, Arc<tokio::sync::Mutex<SyncSession>>>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

/// Sync session state
/// Internal only - not exposed to FFI
#[flutter_rust_bridge::frb(ignore)]
struct SyncSession {
    /// The sync engine
    sync: Option<Arc<tokio::sync::Mutex<SyncEngine>>>,
    /// Cancellation flag shared with the engine
    cancelled: Option<Arc<tokio::sync::RwLock<bool>>>,
    /// Shared progress tracker (readable without locking the engine)
    progress: Option<Arc<tokio::sync::RwLock<SyncProgress>>>,
    /// Shared performance counters
    perf: Option<Arc<PerfCounters>>,
    /// Last known status (for when sync is idle)
    last_status: SyncStatus,
    /// Whether sync is currently running
    is_running: bool,
    /// Last time we updated the target height from the server
    last_target_height_update: Option<std::time::Instant>,
}

impl Default for SyncSession {
    fn default() -> Self {
        Self {
            sync: None,
            cancelled: None,
            progress: None,
            perf: None,
            last_status: SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            },
            is_running: false,
            last_target_height_update: None,
        }
    }
}

/// Start sync for a wallet
pub async fn start_sync(wallet_id: WalletId, mode: SyncMode) -> Result<()> {
    ensure_not_decoy("Sync")?;
    tracing::info!("Starting sync for wallet {} in mode {:?}", wallet_id, mode);
    log_orchard_address_samples(&wallet_id);
    {
        let sessions = SYNC_SESSIONS.read();
        if let Some(session_arc) = sessions.get(&wallet_id) {
            if let Ok(session) = session_arc.try_lock() {
                if session.is_running {
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(debug_log_path())
                    {
                        use std::io::Write;
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_start_sync_skip_running","timestamp":{},"location":"api.rs:start_sync","message":"start_sync skipped; already running","data":{{"wallet_id":"{}","mode":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                            ts, wallet_id, mode
                        );
                    }
                    return Ok(());
                }
            }
        }
    }
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:2306","message":"start_sync wallet","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            id, ts, wallet_id
        );
    }
    // #endregion

    // Get wallet birthday height
    let wallet = get_wallet_meta(&wallet_id)?;
    let birthday_height = wallet.birthday_height;
    let start_height = {
        let resume_height_opt = open_wallet_db_for(&wallet_id).ok().and_then(|(db, _repo)| {
            let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
            sync_storage
                .load_sync_state()
                .ok()
                .map(|state| state.local_height as u32)
        });
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = uuid::Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>();
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:2319","message":"start_sync resume_height","data":{{"wallet_id":"{}","resume_height":"{:?}","birthday_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                id, ts, wallet_id, resume_height_opt, birthday_height
            );
        }
        // #endregion
        match resume_height_opt {
            Some(resume_height) if resume_height > 0 => resume_height,
            _ => birthday_height,
        }
    };

    // Get endpoint configuration (not just URL)
    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();

    // Extract tunnel mode before async
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();

    // Parse endpoint URL to determine TLS settings (same logic as test_node)
    let normalized_url = endpoint_url.trim().to_string();
    let tls_enabled = if normalized_url.starts_with("http://") {
        false // Explicitly disable TLS for http:// URLs
    } else if normalized_url.starts_with("https://") {
        true // Explicitly enable TLS for https:// URLs
    } else {
        endpoint_config.use_tls // Use config value if no protocol specified
    };

    // Extract hostname for TLS SNI
    let host = if let Some(stripped) = normalized_url.strip_prefix("https://") {
        stripped.split(':').next().unwrap_or("").to_string()
    } else if let Some(stripped) = normalized_url.strip_prefix("http://") {
        stripped.split(':').next().unwrap_or("").to_string()
    } else {
        endpoint_config.host.clone()
    };

    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            // If connecting via IP, use the hostname for SNI to match the certificate
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    tracing::info!(
        "start_sync: Using endpoint {} (TLS: {}, transport: {:?})",
        endpoint_url,
        tls_enabled,
        transport
    );

    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:1964","message":"start_sync config","data":{{"endpoint":"{}","tls_enabled":{},"transport":"{:?}","host":"{}","tls_server_name":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            id, ts, endpoint_url, tls_enabled, transport, host, tls_server_name
        );
    }
    // #endregion

    // Create LightClient config with proper TLS settings
    let client_config = LightClientConfig {
        endpoint: endpoint_url.clone(),
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    let network_type = wallet_network_type(&wallet_id)?;
    let is_mobile = cfg!(target_os = "android") || cfg!(target_os = "ios");
    let (
        max_parallel_decrypt,
        max_batch_memory_bytes,
        target_batch_bytes,
        min_batch_bytes,
        max_batch_bytes,
    ) = if is_mobile {
        (8, Some(100_000_000), 8_000_000, 2_000_000, 16_000_000)
    } else {
        (32, Some(500_000_000), 32_000_000, 4_000_000, 64_000_000)
    };

    // Create sync config
    // Adaptive batch sizing will handle spam blocks automatically
    let config = SyncConfig {
        checkpoint_interval: 10_000,
        batch_size: match mode {
            SyncMode::Compact => 2_000, // Faster sync with larger batches (used when server recommendations disabled)
            SyncMode::Deep => 1_000,    // Smaller batches for deep scan
        },
        min_batch_size: 100,                    // Minimum for spam blocks
        max_batch_size: 2_000, // Maximum batch size to prevent OOM (also caps server recommendations)
        use_server_batch_recommendations: true, // Use server's ~4MB chunk recommendations (typically ~199 blocks)
        mini_checkpoint_every: 5,               // Mini-checkpoint every 5 batches
        max_parallel_decrypt,
        lazy_memo_decode: true, // Default to lazy memo decoding
        defer_full_tx_fetch: true,
        target_batch_bytes,
        min_batch_bytes,
        max_batch_bytes,
        heavy_block_threshold_bytes: 500_000, // 500KB per block = heavy/spam
        max_batch_memory_bytes,
    };

    // Create sync engine with wallet context and proper client config
    // We need to modify SyncEngine to accept a LightClientConfig instead of just a URL
    // For now, create the client manually and pass it to a modified sync engine
    let client = LightClient::with_config(client_config);
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let sync = SyncEngine::with_client_and_config(client, start_height, config)
        .with_wallet(wallet_id.clone(), db_key, master_key, network_type)
        .map_err(|e| anyhow!("Failed to initialize sync engine: {}", e))?;
    let sync = Arc::new(Mutex::new(sync));
    let (progress, perf, cancel_flag) = {
        let engine = sync.clone().lock_owned().await;
        (
            engine.progress(),
            engine.perf_counters(),
            engine.cancel_flag(),
        )
    };

    // Store session
    {
        let mut sessions = SYNC_SESSIONS.write();
        let session = Arc::new(tokio::sync::Mutex::new(SyncSession {
            sync: Some(Arc::clone(&sync)),
            cancelled: Some(cancel_flag),
            progress: Some(progress),
            perf: Some(perf),
            last_status: SyncStatus {
                local_height: start_height as u64,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            },
            is_running: true,
            last_target_height_update: None,
        }));
        sessions.insert(wallet_id.clone(), session.clone());
    }

    // Start sync in background
    let wallet_id_for_task = wallet_id.clone();
    tokio::spawn(async move {
        // Clone the session arc while holding the lock, then drop the lock unconditionally
        let session_arc_opt = {
            let sessions = SYNC_SESSIONS.read();
            sessions.get(&wallet_id_for_task).cloned()
        }; // sessions guard dropped here before any await points

        if let Some(session_arc) = session_arc_opt {
            let sync_opt = { session_arc.lock().await.sync.clone() };

            if let Some(sync) = sync_opt {
                let wallet_id_for_log = wallet_id_for_task.clone();
                let result = run_sync_engine_task(sync.clone(), move |engine| {
                    Box::pin(async move {
                        tracing::info!(
                            "Starting sync_from_birthday for wallet {}",
                            wallet_id_for_log
                        );
                        let result = engine
                            .sync_from_birthday()
                            .await
                            .map_err(anyhow::Error::from);
                        if let Err(ref e) = result {
                            tracing::error!("Sync error in engine: {:?}", e);
                        }
                        result
                    })
                })
                .await;

                // Snapshot status after sync attempt.
                let (progress_arc, perf_snapshot) = {
                    let engine = sync.clone().lock_owned().await;
                    (engine.progress(), engine.perf_counters().snapshot())
                };
                let status_opt = {
                    let progress = progress_arc.read().await;
                    let status = SyncStatus {
                        local_height: progress.current_height(),
                        target_height: progress.target_height(),
                        percent: progress.percentage(),
                        eta: progress.eta_seconds(),
                        stage: map_stage(progress.stage()),
                        last_checkpoint: progress.last_checkpoint(),
                        blocks_per_second: perf_snapshot.blocks_per_second,
                        notes_decrypted: perf_snapshot.notes_decrypted,
                        last_batch_ms: perf_snapshot.avg_batch_ms,
                    };
                    tracing::debug!(
                        "Sync status snapshot: local={}, target={}, stage={:?}, percent={:.2}%",
                        status.local_height,
                        status.target_height,
                        status.stage,
                        status.percent
                    );
                    Some(status)
                };

                let mut session = session_arc.lock().await;
                if let Some(status) = status_opt {
                    session.last_status = status;
                }
                match &result {
                    Ok(()) => {
                        // Sync caught up - but it's still running in the background monitoring for new blocks
                        // Don't set is_running = false, sync continues indefinitely
                        tracing::info!(
                            "Sync caught up for wallet {} (still monitoring for new blocks)",
                            wallet_id_for_task
                        );
                        if let Ok(registry_db) = open_wallet_registry() {
                            if let Err(e) =
                                touch_wallet_last_synced(&registry_db, &wallet_id_for_task)
                            {
                                tracing::warn!(
                                    "Failed to update last_synced_at for {}: {}",
                                    wallet_id_for_task,
                                    e
                                );
                            }
                        }
                        // Keep is_running = true since sync is still monitoring
                    }
                    Err(e) => {
                        tracing::error!("Sync failed for wallet {}: {:?}", wallet_id_for_task, e);
                        tracing::error!("Sync error details: {}", e);
                        // Only set is_running = false on actual error
                        session.is_running = false;
                    }
                }
                // Don't set is_running = false here - sync continues monitoring even after catching up
            } else {
                session_arc.lock().await.is_running = false;
            }
        }
    });

    Ok(())
}

/// Get sync status for a wallet with full performance metrics
pub fn sync_status(wallet_id: WalletId) -> Result<SyncStatus> {
    if is_decoy_mode_active() {
        return Ok(SyncStatus {
            local_height: 0,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: SyncStage::Headers,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        });
    }
    let wallet_id_for_panic = wallet_id.clone();
    let result = std::panic::catch_unwind(|| sync_status_inner(wallet_id));
    match result {
        Ok(inner) => inner,
        Err(_) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status_panic","timestamp":{},"location":"api.rs:2557","message":"sync_status panic","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                    ts, wallet_id_for_panic
                );
            }
            Ok(SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            })
        }
    }
}

fn sync_status_inner(wallet_id: WalletId) -> Result<SyncStatus> {
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_sync_status_call","timestamp":{},"location":"api.rs:2557","message":"sync_status call","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                ts, wallet_id
            );
        }
    }
    // #endregion
    let session_arc = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };

    let session_arc = match session_arc {
        Some(session) => session,
        None => {
            // #region agent log
            {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status_session_none","timestamp":{},"location":"api.rs:2568","message":"sync_status no session in map","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        ts, wallet_id
                    );
                }
            }
            // #endregion
            if let Ok((db, _repo)) = open_wallet_db_for(&wallet_id) {
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
                if let Ok(state) = sync_storage.load_sync_state() {
                    let percent = if state.target_height > 0 {
                        (state.local_height as f64 / state.target_height as f64) * 100.0
                    } else {
                        0.0
                    };
                    // #region agent log
                    {
                        use std::io::Write;
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(debug_log_path())
                        {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_sync_status_state","timestamp":{},"location":"api.rs:2585","message":"sync_status returning from sync_state","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                                ts, wallet_id, state.local_height, state.target_height, percent
                            );
                        }
                    }
                    // #endregion
                    return Ok(SyncStatus {
                        local_height: state.local_height,
                        target_height: state.target_height,
                        percent,
                        eta: None,
                        stage: crate::models::SyncStage::Verify,
                        last_checkpoint: Some(state.last_checkpoint_height),
                        blocks_per_second: 0.0,
                        notes_decrypted: 0,
                        last_batch_ms: 0,
                    });
                }
            }
            // #region agent log
            {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status_no_session","timestamp":{},"location":"api.rs:2590","message":"sync_status no session","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        ts, wallet_id
                    );
                }
            }
            // #endregion
            // Return default status if no session
            // #region agent log
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status_default","timestamp":{},"location":"api.rs:2200","message":"sync_status returning default zeros","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"G"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    wallet_id
                );
            }
            // #endregion
            return Ok(SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            });
        }
    };

    let (progress_handle, perf_handle, sync_handle, last_status, last_target_update) = {
        if let Ok(session) = session_arc.try_lock() {
            (
                session.progress.clone(),
                session.perf.clone(),
                session.sync.clone(),
                session.last_status.clone(),
                session.last_target_height_update,
            )
        } else {
            // #region agent log
            {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status_lock_busy","timestamp":{},"location":"api.rs:2626","message":"sync_status session lock busy","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        ts, wallet_id
                    );
                }
            }
            // #endregion
            if let Ok((db, _repo)) = open_wallet_db_for(&wallet_id) {
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
                if let Ok(state) = sync_storage.load_sync_state() {
                    let percent = if state.target_height > 0 {
                        (state.local_height as f64 / state.target_height as f64) * 100.0
                    } else {
                        0.0
                    };
                    // #region agent log
                    {
                        use std::io::Write;
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(debug_log_path())
                        {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_sync_status_state_busy","timestamp":{},"location":"api.rs:2652","message":"sync_status returning from sync_state (lock busy)","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                                ts, wallet_id, state.local_height, state.target_height, percent
                            );
                        }
                    }
                    // #endregion
                    return Ok(SyncStatus {
                        local_height: state.local_height,
                        target_height: state.target_height,
                        percent,
                        eta: None,
                        stage: crate::models::SyncStage::Verify,
                        last_checkpoint: Some(state.last_checkpoint_height),
                        blocks_per_second: 0.0,
                        notes_decrypted: 0,
                        last_batch_ms: 0,
                    });
                }
            }
            return Ok(SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            });
        }
    };

    if let Some(progress) = progress_handle {
        if let Ok(progress) = progress.try_read() {
            let perf_snapshot = perf_handle.as_ref().map(|perf| perf.snapshot());
            let should_update = last_target_update
                .map(|last| last.elapsed().as_secs() >= 10)
                .unwrap_or(true);
            if should_update {
                if let Some(sync) = sync_handle.as_ref() {
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        let sync_clone = Arc::clone(sync);
                        let session_arc_clone = Arc::clone(&session_arc);
                        handle.spawn(async move {
                            let result = run_sync_engine_task(sync_clone, |engine| {
                                Box::pin(async move {
                                    engine
                                        .update_target_height()
                                        .await
                                        .map_err(anyhow::Error::from)
                                })
                            })
                            .await;
                            if result.is_ok() {
                                let mut session = session_arc_clone.lock().await;
                                session.last_target_height_update = Some(std::time::Instant::now());
                            }
                        });
                    }
                }
            }
            let status = SyncStatus {
                local_height: progress.current_height(),
                target_height: progress.target_height(),
                percent: progress.percentage(),
                eta: progress.eta_seconds(),
                stage: map_stage(progress.stage()),
                last_checkpoint: progress.last_checkpoint(),
                blocks_per_second: perf_snapshot
                    .as_ref()
                    .map_or(0.0, |perf| perf.blocks_per_second),
                notes_decrypted: perf_snapshot
                    .as_ref()
                    .map_or(0, |perf| perf.notes_decrypted),
                last_batch_ms: perf_snapshot.as_ref().map_or(0, |perf| perf.avg_batch_ms),
            };

            if let Ok(mut session) = session_arc.try_lock() {
                session.last_status = status.clone();
            }

            // #region agent log
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status","timestamp":{},"location":"api.rs:2166","message":"sync_status returning","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    wallet_id,
                    status.local_height,
                    status.target_height,
                    status.percent,
                    status.stage
                );
            }
            // #endregion

            return Ok(status);
        }
    }

    if let Some(sync) = sync_handle {
        if let Ok(engine) = sync.try_lock() {
            if let Ok(progress) = engine.progress().try_read() {
                let perf = engine.perf_counters().snapshot();
                let target_height = progress.target_height();

                // Update target height from server periodically (every 10 seconds)
                let should_update = last_target_update
                    .map(|last| last.elapsed().as_secs() >= 10)
                    .unwrap_or(true);

                if should_update {
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        let sync_clone = Arc::clone(&sync);
                        let session_arc_clone = Arc::clone(&session_arc);
                        handle.spawn(async move {
                            let result = run_sync_engine_task(sync_clone, |engine| {
                                Box::pin(async move {
                                    engine
                                        .update_target_height()
                                        .await
                                        .map_err(anyhow::Error::from)
                                })
                            })
                            .await;
                            if result.is_ok() {
                                let mut session = session_arc_clone.lock().await;
                                session.last_target_height_update = Some(std::time::Instant::now());
                            }
                        });
                    }
                }

                let status = SyncStatus {
                    local_height: progress.current_height(),
                    target_height,
                    percent: progress.percentage(),
                    eta: progress.eta_seconds(),
                    stage: map_stage(progress.stage()),
                    last_checkpoint: progress.last_checkpoint(),
                    blocks_per_second: perf.blocks_per_second,
                    notes_decrypted: perf.notes_decrypted,
                    last_batch_ms: perf.avg_batch_ms,
                };

                if let Ok(mut session) = session_arc.try_lock() {
                    session.last_status = status.clone();
                }

                // #region agent log
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status","timestamp":{},"location":"api.rs:2166","message":"sync_status returning","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis(),
                        wallet_id,
                        status.local_height,
                        status.target_height,
                        status.percent,
                        status.stage
                    );
                }
                // #endregion

                return Ok(status);
            }
        }
    }

    // Fallback to last known status
    // #region agent log
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        let _ = writeln!(
            file,
            r#"{{"id":"log_sync_status_fallback","timestamp":{},"location":"api.rs:2192","message":"sync_status using fallback last_status","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"F"}}"#,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            wallet_id,
            last_status.local_height,
            last_status.target_height,
            last_status.percent,
            last_status.stage
        );
    }
    // #endregion
    Ok(last_status)
}

/// Get last checkpoint info for diagnostics
pub fn get_last_checkpoint(wallet_id: WalletId) -> Result<Option<CheckpointInfo>> {
    if is_decoy_mode_active() {
        return Ok(None);
    }
    let sessions = SYNC_SESSIONS.read();

    // Try to get checkpoint height from sync session
    let checkpoint_height_opt = if let Some(session_arc) = sessions.get(&wallet_id) {
        if let Ok(session) = session_arc.try_lock() {
            session.last_status.last_checkpoint.map(|h| h as u32)
        } else {
            None
        }
    } else {
        None
    };
    drop(sessions);

    // Load actual checkpoint from database using CheckpointManager
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    use pirate_storage_sqlite::CheckpointManager;
    let manager = CheckpointManager::new(db.conn());

    // If we have a height from sync session, try to get checkpoint at that height
    // Otherwise, get the latest checkpoint
    let checkpoint = if let Some(height) = checkpoint_height_opt {
        manager
            .get_at_height(height)?
            .or_else(|| manager.get_latest().ok().flatten())
    } else {
        manager.get_latest()?
    };

    if let Some(checkpoint) = checkpoint {
        Ok(Some(CheckpointInfo {
            height: checkpoint.height,
            timestamp: checkpoint.timestamp,
        }))
    } else {
        Ok(None)
    }
}

/// Checkpoint information for diagnostics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointInfo {
    /// Checkpoint block height
    pub height: u32,
    /// Unix timestamp when checkpoint was created
    pub timestamp: i64,
}

async fn wait_for_sync_stop(wallet_id: &WalletId, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let running = {
            let sessions = SYNC_SESSIONS.read();
            if let Some(session_arc) = sessions.get(wallet_id) {
                if let Ok(session) = session_arc.try_lock() {
                    session.is_running
                } else {
                    true
                }
            } else {
                false
            }
        };

        if !running {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Rescan wallet from specific height
pub async fn rescan(wallet_id: WalletId, from_height: u32) -> Result<()> {
    ensure_not_decoy("Rescan")?;
    tracing::info!(
        "Rescanning wallet {} from height {}",
        wallet_id,
        from_height
    );
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_start","timestamp":{},"location":"api.rs:3050","message":"rescan start","data":{{"wallet_id":"{}","from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id, from_height
            );
        }
    }
    // #endregion

    // Validate from_height
    if from_height == 0 {
        return Err(anyhow!("Invalid rescan height: must be > 0"));
    }

    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3058","message":"rescan step","data":{{"wallet_id":"{}","step":"cancel_sync_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id
            );
        }
    }
    // #endregion

    // Stop any existing sync session to force a clean rescan.
    let was_syncing = is_sync_running(wallet_id.clone()).unwrap_or(false);
    if was_syncing {
        let cancel_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            cancel_sync(wallet_id.clone()),
        )
        .await;
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let step = if cancel_result.is_ok() {
                    "cancel_sync_done"
                } else {
                    "cancel_sync_timeout"
                };
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3076","message":"rescan step","data":{{"wallet_id":"{}","step":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, step
                );
            }
        }
        // #endregion

        let wait_ok = wait_for_sync_stop(&wallet_id, std::time::Duration::from_secs(10)).await;
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let step = if wait_ok {
                    "cancel_sync_wait_done"
                } else {
                    "cancel_sync_wait_timeout"
                };
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3090","message":"rescan step","data":{{"wallet_id":"{}","step":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, step
                );
            }
        }
        // #endregion
    } else {
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3090","message":"rescan step","data":{{"wallet_id":"{}","step":"cancel_sync_skipped"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
    }

    if let Some(session_arc) = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    } {
        if let Ok(mut session) = session_arc.try_lock() {
            session.is_running = false;
            session.last_status = SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            };
            session.last_target_height_update = None;
        }
    }
    {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(&wallet_id);
    }
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3105","message":"rescan step","data":{{"wallet_id":"{}","step":"session_removed"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id
            );
        }
    }
    // #endregion

    let truncate_height = from_height.saturating_sub(1) as u64;
    {
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3119","message":"rescan step","data":{{"wallet_id":"{}","step":"get_passphrase_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
        let passphrase = match app_passphrase() {
            Ok(passphrase) => passphrase,
            Err(e) => {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_passphrase_error","timestamp":{},"location":"api.rs:3070","message":"rescan passphrase error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id, e
                    );
                }
                return Err(e);
            }
        };
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3146","message":"rescan step","data":{{"wallet_id":"{}","step":"get_passphrase_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3159","message":"rescan step","data":{{"wallet_id":"{}","step":"open_db_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
        let (mut db, _key, _master_key) =
            open_wallet_db_with_passphrase(&wallet_id, &passphrase).map_err(|e| {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_open_db_error","timestamp":{},"location":"api.rs:3085","message":"rescan open db error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts,
                        wallet_id,
                        e
                    );
                }
                e
            })?;
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3181","message":"rescan step","data":{{"wallet_id":"{}","step":"open_db_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3194","message":"rescan step","data":{{"wallet_id":"{}","step":"truncate_start","truncate_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, truncate_height
                );
            }
        }
        // #endregion
        pirate_storage_sqlite::truncate_above_height(&mut db, truncate_height).map_err(|e| {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_truncate_error","timestamp":{},"location":"api.rs:3098","message":"rescan truncate error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts,
                    wallet_id,
                    e
                );
            }
            e
        })?;
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3219","message":"rescan step","data":{{"wallet_id":"{}","step":"truncate_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3234","message":"rescan step","data":{{"wallet_id":"{}","step":"reset_state_start","reset_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts,
                    wallet_id,
                    from_height.saturating_sub(1)
                );
            }
        }
        // #endregion
        let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
        sync_storage
            .reset_sync_state(from_height.saturating_sub(1) as u64)
            .map_err(|e| {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_reset_error","timestamp":{},"location":"api.rs:3112","message":"rescan reset error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts,
                        wallet_id,
                        e
                    );
                }
                e
            })?;
        // #region agent log
        {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3254","message":"rescan step","data":{{"wallet_id":"{}","step":"reset_state_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            }
        }
        // #endregion
    }
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_reset","timestamp":{},"location":"api.rs:3078","message":"rescan reset ok","data":{{"wallet_id":"{}","truncate_height":{},"reset_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts,
                wallet_id,
                truncate_height,
                from_height.saturating_sub(1)
            );
        }
    }
    // #endregion

    // Get endpoint configuration (not just URL)
    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();

    // Extract tunnel mode before async
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();

    // Parse endpoint URL to determine TLS settings (same logic as test_node)
    let normalized_url = endpoint_url.trim().to_string();
    let tls_enabled = if normalized_url.starts_with("http://") {
        false // Explicitly disable TLS for http:// URLs
    } else if normalized_url.starts_with("https://") {
        true // Explicitly enable TLS for https:// URLs
    } else {
        endpoint_config.use_tls // Use config value if no protocol specified
    };

    // Extract hostname for TLS SNI
    let host = if let Some(stripped) = normalized_url.strip_prefix("https://") {
        stripped.split(':').next().unwrap_or("").to_string()
    } else if let Some(stripped) = normalized_url.strip_prefix("http://") {
        stripped.split(':').next().unwrap_or("").to_string()
    } else {
        endpoint_config.host.clone()
    };

    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            // If connecting via IP, use the hostname for SNI to match the certificate
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    // Create LightClient config with proper TLS settings
    let client_config = LightClientConfig {
        endpoint: endpoint_url.clone(),
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    tracing::info!(
        "rescan: Using endpoint {} (TLS: {}, transport: {:?})",
        endpoint_url,
        tls_enabled,
        transport
    );

    let network_type = wallet_network_type(&wallet_id)?;
    let is_mobile = cfg!(target_os = "android") || cfg!(target_os = "ios");
    let (
        max_parallel_decrypt,
        max_batch_memory_bytes,
        target_batch_bytes,
        min_batch_bytes,
        max_batch_bytes,
    ) = if is_mobile {
        (8, Some(100_000_000), 8_000_000, 2_000_000, 16_000_000)
    } else {
        (32, Some(500_000_000), 32_000_000, 4_000_000, 64_000_000)
    };

    // Create sync config for rescan
    let config = SyncConfig {
        checkpoint_interval: 10_000,
        batch_size: 2_000,
        min_batch_size: 100,
        max_batch_size: 2_000,
        use_server_batch_recommendations: true,
        mini_checkpoint_every: 5,
        max_parallel_decrypt,
        lazy_memo_decode: true,
        defer_full_tx_fetch: true,
        target_batch_bytes,
        min_batch_bytes,
        max_batch_bytes,
        heavy_block_threshold_bytes: 500_000,
        max_batch_memory_bytes,
    };

    // Create sync engine with wallet context and proper client config
    let client = LightClient::with_config(client_config);
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let sync = SyncEngine::with_client_and_config(client, from_height, config)
        .with_wallet(wallet_id.clone(), db_key, master_key, network_type)
        .map_err(|e| anyhow!("Failed to initialize sync engine: {}", e))?;
    let sync = Arc::new(Mutex::new(sync));
    let (progress, perf, cancel_flag) = {
        let engine = sync.clone().lock_owned().await;
        (
            engine.progress(),
            engine.perf_counters(),
            engine.cancel_flag(),
        )
    };

    // Store session
    {
        let mut sessions = SYNC_SESSIONS.write();
        let session = Arc::new(tokio::sync::Mutex::new(SyncSession {
            sync: Some(Arc::clone(&sync)),
            cancelled: Some(cancel_flag),
            progress: Some(progress),
            perf: Some(perf),
            last_status: SyncStatus {
                local_height: from_height as u64,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            },
            is_running: true,
            last_target_height_update: None,
        }));
        sessions.insert(wallet_id.clone(), session);
    }
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_session","timestamp":{},"location":"api.rs:3142","message":"rescan session created","data":{{"wallet_id":"{}","from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id, from_height
            );
        }
    }
    // #endregion

    // Start rescan in background
    let wallet_id_for_task = wallet_id.clone();
    tokio::spawn(async move {
        // Clone the session arc while holding the lock, then drop the lock unconditionally
        let session_arc_opt = {
            let sessions = SYNC_SESSIONS.read();
            sessions.get(&wallet_id_for_task).cloned()
        }; // sessions guard dropped here before any await points

        if let Some(session_arc) = session_arc_opt {
            let sync_opt = { session_arc.lock().await.sync.clone() };

            if let Some(sync) = sync_opt {
                let result = run_sync_engine_task(sync.clone(), move |engine| {
                    Box::pin(async move {
                        engine
                            .sync_range(from_height as u64, None)
                            .await
                            .map_err(anyhow::Error::from)
                    })
                })
                .await;

                let (progress_arc, perf_snapshot) = {
                    let engine = sync.clone().lock_owned().await;
                    (engine.progress(), engine.perf_counters().snapshot())
                };
                let status_opt = {
                    let progress = progress_arc.read().await;
                    Some(SyncStatus {
                        local_height: progress.current_height(),
                        target_height: progress.target_height(),
                        percent: progress.percentage(),
                        eta: progress.eta_seconds(),
                        stage: map_stage(progress.stage()),
                        last_checkpoint: progress.last_checkpoint(),
                        blocks_per_second: perf_snapshot.blocks_per_second,
                        notes_decrypted: perf_snapshot.notes_decrypted,
                        last_batch_ms: perf_snapshot.avg_batch_ms,
                    })
                };

                let mut session = session_arc.lock().await;
                if let Some(status) = status_opt {
                    session.last_status = status;
                }
                match result {
                    Ok(()) => tracing::info!("Rescan completed for wallet {}", wallet_id_for_task),
                    Err(e) => {
                        tracing::error!("Rescan failed for wallet {}: {:?}", wallet_id_for_task, e)
                    }
                }
                session.is_running = false;
            } else {
                session_arc.lock().await.is_running = false;
            }
        }
    });

    Ok(())
}

/// Cancel ongoing sync for a wallet
pub async fn cancel_sync(wallet_id: WalletId) -> Result<()> {
    ensure_not_decoy("Cancel sync")?;
    // Clone the session arc while holding the lock, then drop the lock unconditionally
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    }; // sessions guard dropped here before any await points

    if let Some(session_arc) = session_arc_opt {
        let cancel_opt = { session_arc.lock().await.cancelled.clone() };
        if let Some(cancelled) = cancel_opt {
            *cancelled.write().await = true;
            tracing::info!("Sync cancelled for wallet {}", wallet_id);
            // #region agent log
            {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_cancel_sync","timestamp":{},"location":"api.rs:3679","message":"cancel sync","data":{{"wallet_id":"{}","path":"cancel_flag"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id
                    );
                }
            }
            // #endregion
        } else {
            // Fall back to locking the engine if we don't have a cancel flag.
            let sync_opt = { session_arc.lock().await.sync.clone() };
            if let Some(sync) = sync_opt {
                let result = run_sync_engine_task(sync.clone(), |engine| {
                    Box::pin(async move {
                        engine.cancel().await;
                        Ok(())
                    })
                })
                .await;
                if result.is_ok() {
                    tracing::info!("Sync cancelled for wallet {}", wallet_id);
                } else if let Err(e) = result {
                    tracing::warn!("Failed to cancel sync for wallet {}: {}", wallet_id, e);
                }
                // #region agent log
                {
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(debug_log_path())
                    {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_cancel_sync","timestamp":{},"location":"api.rs:3696","message":"cancel sync","data":{{"wallet_id":"{}","path":"engine"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                            ts, wallet_id
                        );
                    }
                }
                // #endregion
            }
        }
        if let Ok(mut session) = session_arc.try_lock() {
            session.is_running = false;
        }
    }

    Ok(())
}

/// Check if sync is running for a wallet
pub fn is_sync_running(wallet_id: WalletId) -> Result<bool> {
    if is_decoy_mode_active() {
        return Ok(false);
    }
    let sessions = SYNC_SESSIONS.read();

    if let Some(session_arc) = sessions.get(&wallet_id) {
        if let Ok(session) = session_arc.try_lock() {
            return Ok(session.is_running);
        }
    }

    Ok(false)
}

// ============================================================================
// Background Sync
// ============================================================================

use pirate_sync_lightd::{BackgroundSyncConfig, BackgroundSyncMode, BackgroundSyncOrchestrator};
use tokio::sync::Mutex as TokioMutex;

const WARM_WALLET_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;
const BG_SYNC_CURSOR_KEY: &str = "bg_rr_cursor";

/// Start background sync for a wallet
///
/// This should be called from iOS BGAppRefreshTask or Android WorkManager.
/// The sync will run with time limits and battery constraints.
///
/// Note: This creates a new SyncEngine instance for background sync to avoid
/// conflicts with foreground sync. The background sync will use the same
/// wallet database and storage.
pub async fn start_background_sync(
    wallet_id: WalletId,
    mode: Option<String>,
) -> Result<crate::models::BackgroundSyncResult> {
    run_on_runtime(move || start_background_sync_inner(wallet_id, mode)).await
}

async fn start_background_sync_inner(
    wallet_id: WalletId,
    mode: Option<String>,
) -> Result<crate::models::BackgroundSyncResult> {
    tracing::info!(
        "Starting background sync for wallet {} with mode {:?}",
        wallet_id,
        mode
    );

    // Extract all needed data before async operations
    let (birthday_height, endpoint_config) = {
        let wallet = get_wallet_meta(&wallet_id)?;
        let birthday_height = wallet.birthday_height;
        let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
        (birthday_height, endpoint_config)
    };

    let endpoint_url = endpoint_config.url();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    let client_config = LightClientConfig {
        endpoint: endpoint_url,
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    let network_type = wallet_network_type(&wallet_id)?;
    // Create sync engine for background sync
    let config = SyncConfig::default();
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let client = LightClient::with_config(client_config);
    let sync_engine = SyncEngine::with_client_and_config(client, birthday_height, config)
        .with_wallet(wallet_id.clone(), db_key, master_key, network_type)
        .map_err(|e| anyhow!("Failed to initialize background sync engine: {}", e))?;

    // Create orchestrator
    let bg_config = BackgroundSyncConfig::default();
    let orchestrator =
        BackgroundSyncOrchestrator::new(Arc::new(TokioMutex::new(sync_engine)), bg_config);

    // Determine sync mode
    let sync_mode = match mode.as_deref() {
        Some("deep") => BackgroundSyncMode::Deep,
        Some("compact") | None => BackgroundSyncMode::Compact,
        _ => BackgroundSyncMode::Compact,
    };

    // Execute background sync
    let result = orchestrator
        .execute_sync(sync_mode)
        .await
        .map_err(|e| anyhow!("Background sync failed: {}", e))?;

    if let Ok(registry_db) = open_wallet_registry() {
        if let Err(e) = touch_wallet_last_synced(&registry_db, &wallet_id) {
            tracing::warn!("Failed to update last_synced_at for {}: {}", wallet_id, e);
        }
    }

    // Convert to FFI model
    Ok(crate::models::BackgroundSyncResult {
        mode: match result.mode {
            BackgroundSyncMode::Compact => "compact".to_string(),
            BackgroundSyncMode::Deep => "deep".to_string(),
        },
        blocks_synced: result.blocks_synced,
        start_height: result.start_height,
        end_height: result.end_height,
        duration_secs: result.duration_secs,
        errors: result.errors,
        new_balance: result.new_balance,
        new_transactions: result.new_transactions,
    })
}

/// Start background sync using round-robin scheduling with warm-wallet priority.
///
/// Chooses the next wallet to sync based on recent usage and rotates fairly
/// across wallets over successive runs.
pub async fn start_background_sync_round_robin(
    mode: Option<String>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    run_on_runtime(move || start_background_sync_round_robin_inner(mode)).await
}

async fn start_background_sync_round_robin_inner(
    mode: Option<String>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    ensure_wallet_registry_loaded()?;

    let registry_db = open_wallet_registry()?;
    let mut candidates = load_wallet_registry_activity(&registry_db)?;
    if candidates.is_empty() {
        return Err(anyhow!("No wallets available for background sync"));
    }

    candidates.retain(|candidate| match is_sync_running(candidate.id.clone()) {
        Ok(is_running) => !is_running,
        Err(_) => true,
    });

    if candidates.is_empty() {
        return Err(anyhow!("All wallets are currently syncing"));
    }

    let now = chrono::Utc::now().timestamp();
    let mut warm = Vec::new();
    let mut cool = Vec::new();
    for candidate in candidates {
        let is_warm = candidate
            .last_used_at
            .map(|ts| now - ts <= WARM_WALLET_WINDOW_SECS)
            .unwrap_or(false);
        if is_warm {
            warm.push(candidate);
        } else {
            cool.push(candidate);
        }
    }

    warm.sort_by_key(|entry| std::cmp::Reverse(entry.last_used_at.unwrap_or(0)));
    cool.sort_by_key(|entry| entry.last_synced_at.unwrap_or(0));

    let mut ordered = Vec::with_capacity(warm.len() + cool.len());
    ordered.extend(warm);
    ordered.extend(cool);

    if ordered.is_empty() {
        return Err(anyhow!("No eligible wallets for background sync"));
    }

    let cursor = get_registry_setting(&registry_db, BG_SYNC_CURSOR_KEY)?;
    if let Some(cursor_id) = cursor.as_deref() {
        if let Some(pos) = ordered.iter().position(|entry| entry.id == cursor_id) {
            let len = ordered.len();
            ordered.rotate_left((pos + 1) % len);
        }
    }

    let next_wallet_id = ordered[0].id.clone();
    let result = start_background_sync_inner(next_wallet_id.clone(), mode).await?;

    if let Err(e) = set_registry_setting(&registry_db, BG_SYNC_CURSOR_KEY, Some(&next_wallet_id)) {
        tracing::warn!(
            "Failed to update background sync cursor for {}: {}",
            next_wallet_id,
            e
        );
    }

    Ok(crate::models::WalletBackgroundSyncResult {
        wallet_id: next_wallet_id,
        mode: result.mode,
        blocks_synced: result.blocks_synced,
        start_height: result.start_height,
        end_height: result.end_height,
        duration_secs: result.duration_secs,
        errors: result.errors,
        new_balance: result.new_balance,
        new_transactions: result.new_transactions,
    })
}

/// Check if background sync is needed for a wallet
pub async fn is_background_sync_needed(wallet_id: WalletId) -> Result<bool> {
    // Extract session Arc before async operations
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).map(Arc::clone)
    };

    if let Some(session_arc) = session_arc_opt {
        let sync_opt = { session_arc.lock().await.sync.clone() };
        if let Some(sync) = sync_opt {
            let progress_arc = {
                let engine = sync.clone().lock_owned().await;
                engine.progress()
            };
            let progress = progress_arc.read().await;
            Ok(progress.current_height() < progress.target_height())
        } else {
            Ok(false)
        }
    } else {
        // No sync session, check storage for sync state
        let passphrase = app_passphrase()?;
        let (db, _key, _master_key) = open_wallet_db_with_passphrase(&wallet_id, &passphrase)?;
        let sync_state = pirate_storage_sqlite::SyncStateStorage::new(&db);

        match sync_state.load_sync_state() {
            Ok(state) => Ok(state.local_height < state.target_height),
            Err(_) => Ok(false),
        }
    }
}

/// Get recommended background sync mode based on time since last sync
pub fn get_recommended_background_sync_mode(
    _wallet_id: WalletId,
    minutes_since_last: u32,
) -> Result<String> {
    // Use default config to determine mode
    let config = BackgroundSyncConfig::default();
    let temp_orchestrator = BackgroundSyncOrchestrator::new(
        Arc::new(TokioMutex::new(SyncEngine::new("dummy".to_string(), 0))),
        config,
    );

    let mode = temp_orchestrator.recommend_sync_mode(minutes_since_last);
    Ok(match mode {
        BackgroundSyncMode::Compact => "compact".to_string(),
        BackgroundSyncMode::Deep => "deep".to_string(),
    })
}

// ============================================================================
// Nodes & Endpoints
// ============================================================================

/// Default lightwalletd endpoint (Pirate Chain official)
pub const DEFAULT_LIGHTD_HOST: &str = "64.23.167.130";
pub const DEFAULT_LIGHTD_PORT: u16 = 9067;
pub const DEFAULT_LIGHTD_USE_TLS: bool = false;

lazy_static::lazy_static! {
    /// Persisted endpoint per wallet (in production, stored encrypted)
    static ref LIGHTD_ENDPOINTS: Arc<RwLock<std::collections::HashMap<WalletId, LightdEndpoint>>> =
        Arc::new(RwLock::new(std::collections::HashMap::new()));
}

/// Lightwalletd endpoint configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LightdEndpoint {
    /// Server host
    pub host: String,
    /// Server port
    pub port: u16,
    /// Whether TLS is enabled
    pub use_tls: bool,
    /// Optional TLS certificate pin (SPKI hash, base64)
    pub tls_pin: Option<String>,
    /// User label
    pub label: Option<String>,
}

impl Default for LightdEndpoint {
    fn default() -> Self {
        Self {
            host: DEFAULT_LIGHTD_HOST.to_string(),
            port: DEFAULT_LIGHTD_PORT,
            use_tls: DEFAULT_LIGHTD_USE_TLS,
            tls_pin: None,
            label: Some("Pirate Chain Official".to_string()),
        }
    }
}

impl LightdEndpoint {
    /// Full URL for gRPC connection
    pub fn url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }

    /// Display string (host:port)
    pub fn display_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Set lightwalletd endpoint
pub fn set_lightd_endpoint(
    wallet_id: WalletId,
    url: String,
    tls_pin_opt: Option<String>,
) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    // Parse URL to extract host/port
    let (host, port, use_tls) = parse_endpoint_url(&url)?;
    // Detect network type from endpoint
    let network_type = detect_network_from_endpoint(&host, port);

    let endpoint = LightdEndpoint {
        host,
        port,
        use_tls,
        tls_pin: tls_pin_opt.clone(),
        label: None,
    };

    let endpoint_url = endpoint.url();

    tracing::info!(
        "Set lightd endpoint for wallet {}: {} (network: {:?})",
        wallet_id,
        endpoint.url(),
        network_type
    );

    LIGHTD_ENDPOINTS
        .write()
        .insert(wallet_id.clone(), endpoint.clone());

    // Update wallet network type
    let mut wallets = WALLETS.write();
    if let Some(wallet) = wallets.iter_mut().find(|w| w.id == wallet_id) {
        let old_network_type = match wallet.network_type.as_deref().unwrap_or("mainnet") {
            "testnet" => NetworkType::Testnet,
            "regtest" => NetworkType::Regtest,
            _ => NetworkType::Mainnet,
        };
        let new_network_type = network_type;
        wallet.network_type = Some(format!("{:?}", new_network_type).to_lowercase());
        let registry_db = open_wallet_registry()?;
        persist_wallet_meta(&registry_db, wallet)?;
        tracing::info!(
            "Updated wallet {} network type to {:?}",
            wallet_id,
            new_network_type
        );

        {
            let ts = chrono::Utc::now().timestamp_millis();
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_set_lightd_endpoint","timestamp":{},"location":"api.rs:set_lightd_endpoint","message":"set_lightd_endpoint","data":{{"wallet_id":"{}","endpoint":"{}","old_network":"{:?}","new_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, endpoint_url, old_network_type, new_network_type
                );
            }
        }

        if old_network_type != new_network_type {
            if let Ok((_db, repo)) = open_wallet_db_for(&wallet_id) {
                if let Err(err) = repo.clear_chain_state() {
                    tracing::warn!(
                        "Failed to clear chain state for wallet {} after network change: {:?}",
                        wallet_id,
                        err
                    );
                }
            }
        }

        if old_network_type != new_network_type {
            if let Err(err) =
                rederive_wallet_keys_for_network(&wallet_id, old_network_type, new_network_type)
            {
                tracing::warn!(
                    "Failed to re-derive keys for wallet {}: {:?}",
                    wallet_id,
                    err
                );
            }
        } else {
            let ts = chrono::Utc::now().timestamp_millis();
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:set_lightd_endpoint","message":"rederive skipped (same network)","data":{{"wallet_id":"{}","network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, new_network_type
                );
            }
        }

        // Persist endpoint per wallet so it survives restarts.
        let endpoint_key = format!("lightd_endpoint_{}", wallet_id);
        let pin_key = format!("lightd_tls_pin_{}", wallet_id);
        set_registry_setting(&registry_db, &endpoint_key, Some(&endpoint_url))?;
        set_registry_setting(&registry_db, &pin_key, tls_pin_opt.as_deref())?;
    }

    Ok(())
}

/// Get lightwalletd endpoint
pub fn get_lightd_endpoint(wallet_id: WalletId) -> Result<String> {
    let endpoints = LIGHTD_ENDPOINTS.read();
    let endpoint = endpoints.get(&wallet_id).cloned().unwrap_or_default();

    Ok(endpoint.url())
}

/// Get full endpoint configuration
pub fn get_lightd_endpoint_config(wallet_id: WalletId) -> Result<LightdEndpoint> {
    let endpoints = LIGHTD_ENDPOINTS.read();
    Ok(endpoints.get(&wallet_id).cloned().unwrap_or_default())
}

/// Detect network type from endpoint URL
///
/// Detects network based on hostname and port:
/// - `lightd1.piratechain.com:9067` → Mainnet (Sapling only)
/// - `64.23.167.130:9067` → Mainnet (Orchard-ready, but not activated)
/// - `64.23.167.130:8067` → Testnet (Orchard activated at block 61)
fn detect_network_from_endpoint(host: &str, port: u16) -> NetworkType {
    // Testnet uses port 8067
    if port == 8067 {
        return NetworkType::Testnet;
    }

    // Mainnet uses port 9067
    // lightd1.piratechain.com is mainnet
    if host == "lightd1.piratechain.com" || host.contains("piratechain.com") {
        return NetworkType::Mainnet;
    }

    // 64.23.167.130:9067 is mainnet (orchard-ready server)
    if host == "64.23.167.130" && port == 9067 {
        return NetworkType::Mainnet;
    }

    // Default to mainnet for unknown endpoints
    NetworkType::Mainnet
}

fn infer_key_network_type_from_addresses(
    mnemonic: &str,
    account_id: i64,
    repo: &Repository,
    endpoint: &LightdEndpoint,
) -> Result<Option<(NetworkType, usize, usize)>> {
    let addresses = repo.get_all_addresses(account_id)?;
    let address_count = addresses.len();
    if addresses.is_empty() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_address_count","timestamp":{},"location":"api.rs:infer_key_network_type_from_addresses","message":"no stored addresses","data":{{"account_id":{},"count":0}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, account_id
            );
        }
        return Ok(None);
    }

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(mnemonic, "")?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;
    let candidates = [
        NetworkType::Mainnet,
        NetworkType::Testnet,
        NetworkType::Regtest,
    ];

    let mut best_network = None;
    let mut best_matches = 0usize;
    let mut match_counts = Vec::new();

    for candidate in candidates {
        let candidate_network = Network::from_type(candidate);
        let sapling_extsk = ExtendedSpendingKey::from_mnemonic_with_account(
            mnemonic,
            "",
            candidate_network.network_type,
            0,
        )?;
        let sapling_fvk = sapling_extsk.to_extended_fvk();
        let orchard_extsk = orchard_master.derive_account(candidate_network.coin_type, 0)?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let prefix_network = address_prefix_network_type_for_endpoint(endpoint, candidate);

        let mut matches = 0usize;
        for addr in &addresses {
            let derived = match addr.address_type {
                AddressType::Orchard => {
                    let orchard_addr = orchard_fvk.address_at(addr.diversifier_index);
                    orchard_addr.encode_for_network(prefix_network)?
                }
                AddressType::Sapling => {
                    let payment_addr = sapling_fvk.derive_address(addr.diversifier_index);
                    payment_addr.encode_for_network(prefix_network)
                }
            };
            if derived == addr.address {
                matches += 1;
            }
        }

        match_counts.push((candidate, matches));
        if matches > best_matches {
            best_matches = matches;
            best_network = Some(candidate);
        }
    }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        let ts = chrono::Utc::now().timestamp_millis();
        let mut summary = String::new();
        for (idx, (candidate, matches)) in match_counts.iter().enumerate() {
            if idx > 0 {
                summary.push(',');
            }
            summary.push_str(&format!(
                r#"{{"network":"{:?}","matches":{}}}"#,
                candidate, matches
            ));
        }
        let sample = addresses.first().map(|addr| {
            let prefix_len = addr.address.chars().take(8).count();
            let sample = addr.address.chars().take(prefix_len).collect::<String>();
            (sample, addr.address_type)
        });
        if let Some((sample_prefix, sample_type)) = sample {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_address_match","timestamp":{},"location":"api.rs:infer_key_network_type_from_addresses","message":"address match summary","data":{{"account_id":{},"count":{},"sample_prefix":"{}","sample_type":"{:?}","matches":[{}]}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, account_id, address_count, sample_prefix, sample_type, summary
            );
        }
    }

    if best_matches == 0 {
        return Ok(None);
    }

    Ok(best_network.map(|network| (network, best_matches, addresses.len())))
}

fn rederive_wallet_keys_for_network(
    wallet_id: &WalletId,
    old_network_type: NetworkType,
    new_network_type: NetworkType,
) -> Result<()> {
    {
        let ts = chrono::Utc::now().timestamp_millis();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_start","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive start","data":{{"wallet_id":"{}","old_network":"{:?}","new_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, old_network_type, new_network_type
            );
        }
    }

    let (_db, repo) = open_wallet_db_for(wallet_id)?;
    let mut secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let mnemonic_bytes = match secret.encrypted_mnemonic.as_ref() {
        Some(bytes) => bytes,
        None => {
            tracing::warn!(
                "Wallet {} has no mnemonic stored; skipping key re-derive",
                wallet_id
            );
            let ts = chrono::Utc::now().timestamp_millis();
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive skipped (no mnemonic)","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id
                );
            }
            return Ok(());
        }
    };

    let mnemonic = String::from_utf8(mnemonic_bytes.clone())
        .map_err(|_| anyhow!("Stored mnemonic is not valid UTF-8"))?;

    let old_network = Network::from_type(old_network_type);
    let current_extsk = ExtendedSpendingKey::from_mnemonic_with_account(
        &mnemonic,
        "",
        old_network.network_type,
        0,
    )?;

    let mut matches_any = current_extsk.to_bytes() == secret.extsk;
    if !matches_any {
        let candidates = [
            NetworkType::Mainnet,
            NetworkType::Testnet,
            NetworkType::Regtest,
        ];
        for candidate in candidates {
            if candidate == old_network_type {
                continue;
            }
            let candidate_net = Network::from_type(candidate);
            let candidate_extsk = ExtendedSpendingKey::from_mnemonic_with_account(
                &mnemonic,
                "",
                candidate_net.network_type,
                0,
            )?;
            if candidate_extsk.to_bytes() == secret.extsk {
                matches_any = true;
                break;
            }
        }
    }

    if !matches_any {
        tracing::warn!(
            "Wallet {} appears to use a non-empty BIP-39 passphrase; skipping key re-derive",
            wallet_id
        );
        let ts = chrono::Utc::now().timestamp_millis();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive skipped (passphrase mismatch)","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id
            );
        }
        return Ok(());
    }

    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    let inferred_network =
        infer_key_network_type_from_addresses(&mnemonic, secret.account_id, &repo, &endpoint)?;
    let key_network_type = if let Some((network_type, matched, total)) = inferred_network {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_infer","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive inferred key network","data":{{"wallet_id":"{}","inferred_network":"{:?}","matched":{},"total":{},"endpoint_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, network_type, matched, total, new_network_type
            );
        }
        network_type
    } else {
        let prefix_network = address_prefix_network_type_for_endpoint(&endpoint, new_network_type);
        if prefix_network != new_network_type {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                let ts = chrono::Utc::now().timestamp_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_prefix_fallback","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive using prefix network fallback","data":{{"wallet_id":"{}","endpoint_network":"{:?}","prefix_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, new_network_type, prefix_network
                );
            }
        }
        prefix_network
    };

    let new_network = Network::from_type(key_network_type);
    let new_extsk = ExtendedSpendingKey::from_mnemonic_with_account(
        &mnemonic,
        "",
        new_network.network_type,
        0,
    )?;
    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(&mnemonic, "")?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;
    let orchard_extsk = orchard_master.derive_account(new_network.coin_type, 0)?;

    secret.extsk = new_extsk.to_bytes();
    secret.dfvk = Some(new_extsk.to_extended_fvk().to_bytes());
    secret.orchard_extsk = Some(orchard_extsk.to_bytes());
    secret.sapling_ivk = None;
    secret.orchard_ivk = None;

    let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
    repo.upsert_wallet_secret(&encrypted_secret)?;
    repo.clear_chain_state()?;

    tracing::info!(
        "Re-derived wallet {} keys for network {:?} and cleared chain state",
        wallet_id,
        key_network_type
    );
    {
        let ts = chrono::Utc::now().timestamp_millis();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_ok","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive ok","data":{{"wallet_id":"{}","network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, key_network_type
            );
        }
    }

    Ok(())
}

/// Parse endpoint URL into components
fn parse_endpoint_url(url: &str) -> Result<(String, u16, bool)> {
    let mut normalized = url.trim().to_string();
    let mut use_tls = DEFAULT_LIGHTD_USE_TLS;

    // Handle scheme
    if normalized.starts_with("https://") {
        normalized = normalized[8..].to_string();
        use_tls = true;
    } else if normalized.starts_with("http://") {
        normalized = normalized[7..].to_string();
        use_tls = false;
    }

    // Remove trailing slash
    if normalized.ends_with('/') {
        normalized.pop();
    }

    // Parse host:port
    let parts: Vec<&str> = normalized.split(':').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(anyhow!("Invalid endpoint URL format"));
    }

    let host = parts[0].to_string();
    if host.is_empty() {
        return Err(anyhow!("Empty host"));
    }

    let port = if parts.len() == 2 {
        parts[1]
            .parse::<u16>()
            .map_err(|_| anyhow!("Invalid port number"))?
    } else {
        DEFAULT_LIGHTD_PORT
    };

    Ok((host, port, use_tls))
}

// ============================================================================
// Network Tunnel
// ============================================================================

/// Set network tunnel mode
pub fn set_tunnel(mode: TunnelMode) -> Result<()> {
    tracing::info!("Setting tunnel mode: {:?}", mode);
    *TUNNEL_MODE.write() = mode.clone();
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let (mode_label, socks5_label) = match &mode {
            TunnelMode::Tor => ("tor", "none".to_string()),
            TunnelMode::I2p => ("i2p", "none".to_string()),
            TunnelMode::Direct => ("direct", "none".to_string()),
            TunnelMode::Socks5 { url } => ("socks5", redact_socks5_url(url)),
        };
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_set","timestamp":{},"location":"api.rs:{}", "message":"set_tunnel","data":{{"mode":"{}","socks5":"{}"}}}}"#,
            ts,
            line!(),
            mode_label,
            socks5_label
        );
    }
    // #endregion
    if let Ok(registry_db) = open_wallet_registry() {
        if let Err(e) = persist_registry_tunnel_mode(&registry_db, &mode) {
            tracing::warn!("Failed to persist tunnel mode: {}", e);
            *PENDING_TUNNEL_MODE.write() = Some(mode.clone());
        }
    } else {
        *PENDING_TUNNEL_MODE.write() = Some(mode.clone());
    }
    spawn_bootstrap_transport(mode);
    Ok(())
}

/// Get current tunnel mode
pub fn get_tunnel() -> Result<TunnelMode> {
    Ok(TUNNEL_MODE.read().clone())
}

/// Bootstrap tunnel transport early (Tor/I2P/SOCKS5) without unlocking wallets.
pub async fn bootstrap_tunnel(mode: TunnelMode) -> Result<()> {
    let (transport, socks5_url, _) = tunnel_transport_config_for(&mode);
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let (mode_label, socks5_label) = match &mode {
            TunnelMode::Tor => ("tor", "none".to_string()),
            TunnelMode::I2p => ("i2p", "none".to_string()),
            TunnelMode::Direct => ("direct", "none".to_string()),
            TunnelMode::Socks5 { url } => ("socks5", redact_socks5_url(url)),
        };
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_bootstrap","timestamp":{},"location":"api.rs:{}", "message":"bootstrap_tunnel","data":{{"mode":"{}","socks5":"{}"}}}}"#,
            ts,
            line!(),
            mode_label,
            socks5_label
        );
    }
    // #endregion
    pirate_sync_lightd::bootstrap_transport(transport, socks5_url)
        .await
        .map_err(|e| anyhow!("Failed to bootstrap transport: {}", e))?;
    Ok(())
}

/// Shutdown any active transport manager (Tor/I2P/SOCKS5).
pub async fn shutdown_transport() -> Result<()> {
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_shutdown","timestamp":{},"location":"api.rs:{}", "message":"shutdown_transport","data":{{}}}}"#,
            ts,
            line!()
        );
    }
    // #endregion
    pirate_sync_lightd::shutdown_transport().await;
    Ok(())
}

/// Configure Tor bridge settings (Snowflake/obfs4/custom) for censorship circumvention.
pub async fn set_tor_bridge_settings(
    use_bridges: bool,
    fallback_to_bridges: bool,
    transport: String,
    bridge_lines: Vec<String>,
    transport_path: Option<String>,
) -> Result<()> {
    pirate_sync_lightd::client::set_tor_bridge_settings(
        use_bridges,
        fallback_to_bridges,
        transport.clone(),
        bridge_lines.clone(),
        transport_path.clone(),
    )?;

    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tor_bridge_settings","timestamp":{},"location":"api.rs:{}", "message":"set_tor_bridge_settings","data":{{"use_bridges":{},"fallback_to_bridges":{},"transport":"{}","bridge_lines":{},"transport_path_set":{}}}}}"#,
            ts,
            line!(),
            use_bridges,
            fallback_to_bridges,
            escape_json(&transport),
            bridge_lines.len(),
            transport_path
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false)
        );
    }
    // #endregion

    let current = TUNNEL_MODE.read().clone();
    if matches!(current, TunnelMode::Tor) {
        pirate_sync_lightd::bootstrap_transport(TransportMode::Tor, None)
            .await
            .map_err(|e| anyhow!("Failed to bootstrap transport: {}", e))?;
    }

    Ok(())
}

/// Get current Tor bootstrap status for UI.
pub async fn get_tor_status() -> Result<String> {
    let status = pirate_sync_lightd::tor_status().await;
    let payload = match status {
        Some(pirate_sync_lightd::TorStatus::Ready) => "{\"status\":\"ready\"}".to_string(),
        Some(pirate_sync_lightd::TorStatus::Bootstrapping { progress, blocked }) => {
            if let Some(blocked) = blocked {
                format!(
                    "{{\"status\":\"bootstrapping\",\"progress\":{},\"blocked\":\"{}\"}}",
                    progress,
                    escape_json(&blocked)
                )
            } else {
                format!("{{\"status\":\"bootstrapping\",\"progress\":{}}}", progress)
            }
        }
        Some(pirate_sync_lightd::TorStatus::Error(message)) => {
            format!(
                "{{\"status\":\"error\",\"error\":\"{}\"}}",
                escape_json(&message)
            )
        }
        Some(pirate_sync_lightd::TorStatus::NotStarted) | None => {
            "{\"status\":\"not_started\"}".to_string()
        }
    };
    Ok(payload)
}

/// Rotate Tor exit circuits for new streams and reconnect sync channels.
pub async fn rotate_tor_exit() -> Result<()> {
    pirate_sync_lightd::rotate_tor_exit()
        .await
        .map_err(|e| anyhow!("Failed to rotate Tor exit: {}", e))?;

    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tor_exit_rotate","timestamp":{},"location":"api.rs:{}", "message":"tor_exit_rotate","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts,
            line!()
        );
    }
    // #endregion

    let sessions: Vec<(WalletId, Arc<tokio::sync::Mutex<SyncSession>>)> = {
        let sessions = SYNC_SESSIONS.read();
        sessions
            .iter()
            .map(|(wallet_id, session)| (wallet_id.clone(), Arc::clone(session)))
            .collect()
    };

    for (wallet_id, session_arc) in sessions {
        let sync_opt = { session_arc.lock().await.sync.clone() };
        if let Some(sync) = sync_opt {
            let wallet_id_for_log = wallet_id.clone();
            let result = run_sync_engine_task(sync.clone(), move |engine| {
                Box::pin(async move {
                    engine.disconnect().await;
                    Ok(())
                })
            })
            .await;
            if let Err(e) = result {
                tracing::warn!(
                    "Failed to disconnect sync engine for {} after Tor exit rotation: {}",
                    wallet_id_for_log,
                    e
                );
            }
        }
    }

    Ok(())
}

// ============================================================================
// Balance & Transactions
// ============================================================================

/// Get wallet balance
///
/// Calculates balance from unspent notes in the database.
/// - spendable: Confirmed unspent notes (with 10+ confirmations)
/// - pending: Unconfirmed unspent notes
/// - total: spendable + pending
pub fn get_balance(wallet_id: WalletId) -> Result<Balance> {
    if is_decoy_mode_active() {
        return Ok(Balance {
            total: 0,
            spendable: 0,
            pending: 0,
        });
    }
    tracing::info!("Getting balance for wallet {}", wallet_id);

    // Open encrypted wallet DB
    let (db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    // Get current height from sync state
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height;

    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4186","message":"get_balance start","data":{{"wallet_id":"{}","account_id":{},"current_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, secret.account_id, current_height
            );
        }
    }
    // #endregion

    // Standard confirmation depth for Pirate Chain (10 blocks, same as Zcash)
    const MIN_DEPTH: u64 = 10;

    // #region agent log
    {
        let unspent = repo.get_unspent_notes(secret.account_id)?;
        let (count, sum_value, min_h, max_h) = if unspent.is_empty() {
            (0usize, 0i64, None, None)
        } else {
            let mut sum = 0i64;
            let mut min_height = i64::MAX;
            let mut max_height = i64::MIN;
            for n in &unspent {
                sum = sum.saturating_add(n.value);
                min_height = min_height.min(n.height);
                max_height = max_height.max(n.height);
            }
            (unspent.len(), sum, Some(min_height), Some(max_height))
        };
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4196","message":"get_balance unspent","data":{{"wallet_id":"{}","unspent_count":{},"unspent_sum":{},"min_height":{},"max_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                wallet_id,
                count,
                sum_value,
                min_h
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                max_h
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            );
        }
    }
    // #endregion

    // Calculate balance from unspent notes
    let (spendable, pending, total) =
        repo.calculate_balance(secret.account_id, current_height, MIN_DEPTH)?;

    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4204","message":"get_balance result","data":{{"wallet_id":"{}","total":{},"spendable":{},"pending":{},"min_depth":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, total, spendable, pending, MIN_DEPTH
            );
        }
    }
    // #endregion

    tracing::debug!(
        "Balance for wallet {}: total={}, spendable={}, pending={} (height={})",
        wallet_id,
        total,
        spendable,
        pending,
        current_height
    );

    Ok(Balance {
        total,
        spendable,
        pending,
    })
}

/// List transactions
///
/// Returns transaction history from the database, aggregated by transaction ID.
/// Transactions are sorted by height descending (newest first).
pub fn list_transactions(wallet_id: WalletId, limit: Option<u32>) -> Result<Vec<TxInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    tracing::info!(
        "Listing transactions for wallet {} (limit: {:?})",
        wallet_id,
        limit
    );

    // Open encrypted wallet DB
    let (db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    let spendable =
        !secret.extsk.is_empty() || secret.orchard_extsk.as_ref().is_some_and(|k| !k.is_empty());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = format!("{:08x}", ts);
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:list_transactions","message":"list_transactions flags","data":{{"wallet_id":"{}","spendable":{},"extsk_len":{},"orchard_extsk_len":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            id,
            ts,
            wallet_id,
            spendable,
            secret.extsk.len(),
            secret.orchard_extsk.as_ref().map(|k| k.len()).unwrap_or(0)
        );
    }

    // Get current height from sync state
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height;

    // Standard confirmation depth for Pirate Chain (10 blocks)
    const MIN_DEPTH: u64 = 10;

    // Get transactions from database
    let split_transfers = spendable;
    let tx_records = repo.get_transactions_with_options(
        secret.account_id,
        limit,
        current_height,
        MIN_DEPTH,
        split_transfers,
    )?;

    // Convert to TxInfo format
    let transactions: Vec<TxInfo> = tx_records
        .into_iter()
        .map(|tx| {
            // Determine confirmed status
            let confirmed = if tx.height > 0 {
                let confirmations = current_height.saturating_sub(tx.height as u64);
                confirmations >= MIN_DEPTH
            } else {
                false
            };

            // Decode memo from bytes to string (if present)
            let memo_str = tx.memo.and_then(|memo_bytes| {
                // Memo is typically UTF-8, but may have null padding
                // Remove null bytes and try to decode
                let trimmed: Vec<u8> = memo_bytes.into_iter().take_while(|&b| b != 0).collect();

                // Try UTF-8 decode
                String::from_utf8(trimmed).ok().filter(|s| !s.is_empty())
            });

            TxInfo {
                txid: tx.txid,
                height: if tx.height > 0 {
                    Some(tx.height as u32)
                } else {
                    None
                },
                timestamp: tx.timestamp,
                amount: tx.amount,
                fee: tx.fee,
                memo: memo_str,
                confirmed,
            }
        })
        .collect();

    tracing::debug!(
        "Found {} transactions for wallet {}",
        transactions.len(),
        wallet_id
    );

    Ok(transactions)
}

/// Fetch and decrypt memo for a specific transaction (lazy memo decoding)
///
/// This function implements lazy memo decoding:
/// 1. Checks if memo already exists in database
/// 2. If exists, validates it by re-decrypting to ensure it's correct
/// 3. If missing or corrupted, fetches full transaction and decrypts memo
/// 4. Stores memo in database for future use
///
/// # Arguments
/// * `wallet_id` - Wallet ID
/// * `txid` - Transaction ID (hex string)
/// * `output_index` - Optional output index (if None, returns first memo found)
///
/// # Returns
/// Decoded memo string, or None if no memo exists or decryption fails
pub async fn fetch_transaction_memo(
    wallet_id: WalletId,
    txid: String,
    output_index: Option<u32>,
) -> Result<Option<String>> {
    run_on_runtime(move || fetch_transaction_memo_inner(wallet_id, txid, output_index)).await
}

async fn fetch_transaction_memo_inner(
    wallet_id: WalletId,
    txid: String,
    output_index: Option<u32>,
) -> Result<Option<String>> {
    tracing::info!(
        "Fetching memo for transaction {} (output_index: {:?})",
        txid,
        output_index
    );

    // Extract all data from DB in a block scope to ensure repo is dropped before async
    let (
        endpoint_config,
        account_id,
        ivk_by_output,
        output_indices,
        notes_with_memos,
        txid_array,
        txid_bytes,
    ) = {
        // Open encrypted wallet DB
        let (_db, repo) = open_wallet_db_for(&wallet_id)?;

        // Get wallet secret to find account_id
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

        // Parse txid from hex
        let txid_bytes = hex::decode(&txid).map_err(|e| anyhow!("Invalid txid hex: {}", e))?;

        if txid_bytes.len() != 32 {
            return Err(anyhow!(
                "Invalid txid length: {} (expected 32 bytes)",
                txid_bytes.len()
            ));
        }

        let mut txid_array = [0u8; 32];
        txid_array.copy_from_slice(&txid_bytes[..32]);

        let default_ivk_bytes = if !secret.extsk.is_empty() {
            ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map(|extsk| extsk.to_extended_fvk().to_ivk().to_sapling_ivk_bytes())
                .ok()
        } else if let Some(ref dfvk_bytes) = secret.dfvk {
            ExtendedFullViewingKey::from_bytes(dfvk_bytes)
                .map(|dfvk| dfvk.to_ivk().to_sapling_ivk_bytes())
        } else if let Some(ref ivk_bytes) = secret.sapling_ivk {
            if ivk_bytes.len() == 32 {
                let mut ivk = [0u8; 32];
                ivk.copy_from_slice(&ivk_bytes[..32]);
                Some(ivk)
            } else {
                None
            }
        } else {
            None
        };

        let notes = repo.get_notes_by_txid(secret.account_id, &txid_bytes)?;
        let mut ivk_by_key: HashMap<i64, [u8; 32]> = HashMap::new();
        let mut ivk_by_output: HashMap<i64, [u8; 32]> = HashMap::new();
        let mut output_indices: Vec<i64> = Vec::new();

        for note in &notes {
            if note.note_type != pirate_storage_sqlite::models::NoteType::Sapling {
                continue;
            }
            output_indices.push(note.output_index);
            let ivk_bytes = if let Some(key_id) = note.key_id {
                if let Some(cached) = ivk_by_key.get(&key_id) {
                    Some(*cached)
                } else {
                    let key = repo
                        .get_account_key_by_id(key_id)?
                        .ok_or_else(|| anyhow!("Key group not found"))?;
                    let ivk = if let Some(ref bytes) = key.sapling_extsk {
                        let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
                        extsk.to_extended_fvk().to_ivk().to_sapling_ivk_bytes()
                    } else if let Some(ref bytes) = key.sapling_dfvk {
                        let dfvk = ExtendedFullViewingKey::from_bytes(bytes)
                            .ok_or_else(|| anyhow!("Invalid Sapling viewing key bytes"))?;
                        dfvk.to_ivk().to_sapling_ivk_bytes()
                    } else {
                        continue;
                    };
                    ivk_by_key.insert(key_id, ivk);
                    Some(ivk)
                }
            } else {
                default_ivk_bytes
            };

            if let Some(ivk_bytes) = ivk_bytes {
                ivk_by_output.insert(note.output_index, ivk_bytes);
            }
        }

        if let Some(idx) = output_index {
            output_indices = vec![idx as i64];
            let idx_i64 = idx as i64;
            if let Some(default_ivk) = default_ivk_bytes {
                ivk_by_output.entry(idx_i64).or_insert(default_ivk);
            }
        }

        // Extract all needed data before async operations
        let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
        let account_id = secret.account_id;
        // Collect all notes with memos before async operations
        let mut notes_with_memos = Vec::new();
        for idx in &output_indices {
            if let Some(note) = repo.get_note_by_txid_and_index(account_id, &txid_bytes, *idx)? {
                if let Some(ref stored_memo) = note.memo {
                    let commitment = if note.commitment.len() == 32 {
                        let mut cmu = [0u8; 32];
                        cmu.copy_from_slice(&note.commitment[..32]);
                        Some(cmu)
                    } else {
                        None
                    };
                    if let Some(ivk_bytes) = ivk_by_output.get(idx).copied() {
                        notes_with_memos.push((*idx, stored_memo.clone(), commitment, ivk_bytes));
                    }
                }
            }
        }

        // Return all extracted data (repo and _db are dropped here)
        (
            endpoint,
            account_id,
            ivk_by_output,
            output_indices,
            notes_with_memos,
            txid_array,
            txid_bytes,
        )
    };

    let endpoint_url = endpoint_config.url();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    let client_config = LightClientConfig {
        endpoint: endpoint_url,
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    // Try validating stored memos
    for (idx, stored_memo, cmu_opt, ivk_bytes) in notes_with_memos {
        let client = pirate_sync_lightd::LightClient::with_config(client_config.clone());

        match client.connect().await {
            Ok(_) => {
                match client.get_transaction(&txid_array).await {
                    Ok(raw_tx_bytes) => {
                        if let Some(cmu) = cmu_opt {
                            match pirate_sync_lightd::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes(
                                &raw_tx_bytes,
                                idx as usize,
                                &ivk_bytes,
                                Some(&cmu),
                            ) {
                                Ok(Some(decrypted)) => {
                                    if decrypted.memo == stored_memo {
                                        let memo_str = pirate_sync_lightd::sapling::full_decrypt::decode_memo(&decrypted.memo);
                                        tracing::debug!("Memo validated from database for tx {} output {}", txid, idx);
                                        return Ok(memo_str);
                                    } else {
                                        // Memo is corrupted, update it (re-open DB)
                                        let (_db2, repo2) = open_wallet_db_for(&wallet_id)?;
                                        repo2.update_note_memo(account_id, &txid_bytes, idx, Some(&decrypted.memo))?;
                                        let memo_str = pirate_sync_lightd::sapling::full_decrypt::decode_memo(&decrypted.memo);
                                        return Ok(memo_str);
                                    }
                                }
                                _ => continue,
                            }
                        }
                    }
                    _ => {
                        // If validation fails, return stored memo anyway
                        let memo_str =
                            pirate_sync_lightd::sapling::full_decrypt::decode_memo(&stored_memo);
                        return Ok(memo_str);
                    }
                }
            }
            _ => {
                // If connection fails, return stored memo anyway
                let memo_str = pirate_sync_lightd::sapling::full_decrypt::decode_memo(&stored_memo);
                return Ok(memo_str);
            }
        }
    }

    // Memo not in database or validation failed, fetch and decrypt
    let client = pirate_sync_lightd::LightClient::with_config(client_config);
    client
        .connect()
        .await
        .map_err(|e| anyhow!("Failed to connect to lightwalletd: {}", e))?;

    let raw_tx_bytes = client
        .get_transaction(&txid_array)
        .await
        .map_err(|e| anyhow!("Failed to fetch transaction: {}", e))?;

    // Get commitment from note if available (re-open DB) and decrypt
    for idx in output_indices {
        let ivk_bytes = match ivk_by_output.get(&idx) {
            Some(bytes) => bytes,
            None => continue,
        };
        let cmu_opt = {
            let (_db2, repo2) = open_wallet_db_for(&wallet_id)?;
            if let Some(note) = repo2.get_note_by_txid_and_index(account_id, &txid_bytes, idx)? {
                if note.commitment.len() == 32 {
                    let mut cmu = [0u8; 32];
                    cmu.copy_from_slice(&note.commitment[..32]);
                    Some(cmu)
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Decrypt memo
        match pirate_sync_lightd::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes(
            &raw_tx_bytes,
            idx as usize,
            ivk_bytes,
            cmu_opt.as_ref(),
        ) {
            Ok(Some(decrypted)) => {
                // Store memo in database (re-open DB)
                let (_db3, repo3) = open_wallet_db_for(&wallet_id)?;
                repo3.update_note_memo(account_id, &txid_bytes, idx, Some(&decrypted.memo))?;

                // Decode and return
                let memo_str =
                    pirate_sync_lightd::sapling::full_decrypt::decode_memo(&decrypted.memo);
                tracing::info!("Fetched and stored memo for tx {} output {}", txid, idx);
                return Ok(memo_str);
            }
            Ok(None) => {
                // No memo for this output, try next
                continue;
            }
            Err(e) => {
                tracing::warn!("Failed to decrypt memo for output {}: {}", idx, e);
                continue;
            }
        }
    }

    // No memo found for any output
    Ok(None)
}

// ============================================================================
// Utilities
// ============================================================================

/// Generate new mnemonic (utility function for testing/development)
///
/// **Note**: New wallets always use 24-word seeds. This function is provided
/// for testing/utilities. For wallet creation, use `create_wallet()` which
/// always generates 24-word seeds.
///
/// # Arguments
/// * `word_count` - Number of words in mnemonic (12, 18, or 24). Defaults to 24 if None.
///
/// # Returns
/// BIP39 mnemonic phrase with the specified number of words
pub fn generate_mnemonic(word_count: Option<u32>) -> Result<String> {
    // Validate word count (must be 12, 18, or 24)
    if let Some(count) = word_count {
        if count != 12 && count != 18 && count != 24 {
            return Err(anyhow!("Invalid word count: must be 12, 18, or 24"));
        }
    }

    Ok(ExtendedSpendingKey::generate_mnemonic(word_count))
}

/// Validate mnemonic
pub fn validate_mnemonic(mnemonic: String) -> Result<bool> {
    match ExtendedSpendingKey::from_mnemonic(&mnemonic, "") {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Get network info
pub fn get_network_info() -> Result<NetworkInfo> {
    let net = pirate_params::Network::mainnet();

    Ok(NetworkInfo {
        name: net.name.to_string(),
        coin_type: net.coin_type,
        rpc_port: net.rpc_port,
        default_birthday: net.default_birthday_height,
    })
}

/// Format amount (arrrtoshis to ARRR)
pub fn format_amount(arrrtoshis: u64) -> Result<String> {
    let arrr = arrrtoshis as f64 / 100_000_000.0;
    Ok(format!("{:.8}", arrr))
}

/// Parse amount (ARRR to arrrtoshis)
pub fn parse_amount(arrr: String) -> Result<u64> {
    let value: f64 = arrr.parse().map_err(|_| anyhow!("Invalid amount"))?;
    Ok((value * 100_000_000.0) as u64)
}

// ============================================================================
// Security Features
// ============================================================================

use pirate_storage_sqlite::{
    seed_warnings, DecoyVaultManager, ExportFlowState, IvkImportRequest, PanicPin,
    SeedExportManager, VaultMode, WatchOnlyBanner, WatchOnlyCapabilities, WatchOnlyManager,
};

lazy_static::lazy_static! {
    /// Global decoy vault manager
    static ref DECOY_VAULT: Arc<RwLock<DecoyVaultManager>> =
        Arc::new(RwLock::new(DecoyVaultManager::new()));

    /// Global seed export manager
    static ref SEED_EXPORT: Arc<RwLock<SeedExportManager>> =
        Arc::new(RwLock::new(SeedExportManager::new()));

    /// Global watch-only manager
    static ref WATCH_ONLY: Arc<RwLock<WatchOnlyManager>> =
        Arc::new(RwLock::new(WatchOnlyManager::new()));
}

fn is_decoy_mode_active() -> bool {
    let vault = DECOY_VAULT.read();
    vault.is_decoy_mode()
}

fn decoy_wallet_meta() -> WalletMeta {
    let vault = DECOY_VAULT.read();
    let network = Network::mainnet();
    WalletMeta {
        id: DECOY_WALLET_ID.to_string(),
        name: vault.decoy_name(),
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height: network.default_birthday_height,
        network_type: Some(network.name.to_string()),
    }
}

fn ensure_decoy_wallet_state() {
    let meta = decoy_wallet_meta();
    *WALLETS.write() = vec![meta.clone()];
    *ACTIVE_WALLET.write() = Some(meta.id);
}

fn reverse_passphrase(passphrase: &str) -> String {
    passphrase.chars().rev().collect()
}

fn ensure_not_decoy(operation: &str) -> Result<()> {
    if is_decoy_mode_active() {
        return Err(anyhow!("{} is unavailable in decoy mode", operation));
    }
    Ok(())
}

fn validate_custom_duress_passphrase(passphrase: &str) -> Result<()> {
    const SYMBOLS: &str = "!@#$%^&*(),.?\":{}|<>";

    AppPassphrase::validate(passphrase)?;

    if !passphrase.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(anyhow!("Duress passphrase must include a lowercase letter"));
    }
    if !passphrase.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(anyhow!(
            "Duress passphrase must include an uppercase letter"
        ));
    }
    if !passphrase.chars().any(|c| c.is_ascii_digit()) {
        return Err(anyhow!("Duress passphrase must include a number"));
    }
    if !passphrase.chars().any(|c| SYMBOLS.contains(c)) {
        return Err(anyhow!(
            "Duress passphrase must include a symbol (e.g. !@#$)"
        ));
    }

    Ok(())
}

fn refresh_duress_reverse_hash(registry_db: &Database, new_passphrase: &str) -> Result<()> {
    let use_reverse = get_registry_setting(registry_db, REGISTRY_DURESS_USE_REVERSE_KEY)?
        .map(|value| value == "true")
        .unwrap_or(false);

    if !use_reverse {
        return Ok(());
    }

    if new_passphrase.chars().eq(new_passphrase.chars().rev()) {
        set_registry_setting(registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None)?;
        set_registry_setting(registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None)?;
        return Ok(());
    }

    let duress_passphrase = reverse_passphrase(new_passphrase);
    let duress_hash = AppPassphrase::hash(&duress_passphrase)
        .map_err(|e| anyhow!("Failed to hash duress passphrase: {}", e))?;
    set_registry_setting(
        registry_db,
        REGISTRY_DURESS_PASSPHRASE_HASH_KEY,
        Some(duress_hash.hash_string()),
    )?;
    Ok(())
}

// ============================================================================
// Panic PIN / Decoy Vault
// ============================================================================

/// Set panic PIN for decoy vault
pub fn set_panic_pin(pin: String) -> Result<()> {
    // Validate PIN format
    if pin.len() < 4 || pin.len() > 8 {
        return Err(anyhow!("PIN must be 4-8 digits"));
    }

    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow!("PIN must contain only digits"));
    }

    // Hash PIN with Argon2id
    let panic_pin = PanicPin::hash(&pin).map_err(|e| anyhow!("Failed to hash PIN: {}", e))?;

    let salt = pirate_storage_sqlite::generate_salt().to_vec();

    // Enable decoy vault
    let vault = DECOY_VAULT.read();
    vault
        .enable(panic_pin.hash_string().to_string(), salt)
        .map_err(|e| anyhow!("Failed to enable decoy vault: {}", e))?;

    tracing::info!("Panic PIN configured and decoy vault enabled");
    Ok(())
}

/// Check if panic PIN is configured
pub fn has_panic_pin() -> Result<bool> {
    let vault = DECOY_VAULT.read();
    Ok(vault.config().enabled)
}

/// Verify panic PIN (returns true if PIN matches and activates decoy mode)
pub fn verify_panic_pin(pin: String) -> Result<bool> {
    let vault = DECOY_VAULT.read();

    let is_panic = vault
        .verify_panic_pin(&pin)
        .map_err(|e| anyhow!("Failed to verify PIN: {}", e))?;

    if is_panic {
        vault
            .activate_decoy()
            .map_err(|e| anyhow!("Failed to activate decoy: {}", e))?;
        tracing::warn!("Decoy vault activated via panic PIN");
    }

    Ok(is_panic)
}

/// Check if currently in decoy mode
pub fn is_decoy_mode() -> Result<bool> {
    let vault = DECOY_VAULT.read();
    Ok(vault.is_decoy_mode())
}

/// Get current vault mode
pub fn get_vault_mode() -> Result<String> {
    let vault = DECOY_VAULT.read();
    Ok(match vault.mode() {
        VaultMode::Real => "real".to_string(),
        VaultMode::Decoy => "decoy".to_string(),
    })
}

/// Clear panic PIN and disable decoy vault
pub fn clear_panic_pin() -> Result<()> {
    let vault = DECOY_VAULT.read();
    vault
        .disable()
        .map_err(|e| anyhow!("Failed to disable decoy vault: {}", e))?;

    tracing::info!("Panic PIN cleared and decoy vault disabled");
    Ok(())
}

/// Set duress passphrase for decoy vault
/// Returns the Argon2id hash for secure storage on the client side.
pub fn set_duress_passphrase(custom_passphrase: Option<String>) -> Result<String> {
    let app_passphrase =
        passphrase_store::get_passphrase().map_err(|e| anyhow!("App is locked: {}", e))?;
    let app_passphrase = app_passphrase.as_str();

    let custom_trimmed = custom_passphrase
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let use_reverse = custom_trimmed.is_none();
    let duress_passphrase = if let Some(value) = custom_trimmed {
        validate_custom_duress_passphrase(&value)?;
        value
    } else {
        if app_passphrase.chars().eq(app_passphrase.chars().rev()) {
            return Err(anyhow!(
                "Passphrase reads the same forwards and backwards; set a custom duress passphrase"
            ));
        }
        reverse_passphrase(app_passphrase)
    };

    if duress_passphrase == app_passphrase {
        return Err(anyhow!(
            "Duress passphrase must be different from your app passphrase"
        ));
    }

    let duress_hash = AppPassphrase::hash(&duress_passphrase)
        .map_err(|e| anyhow!("Failed to hash duress passphrase: {}", e))?;

    let registry_db = open_wallet_registry()?;
    set_registry_setting(
        &registry_db,
        REGISTRY_DURESS_PASSPHRASE_HASH_KEY,
        Some(duress_hash.hash_string()),
    )?;
    set_registry_setting(
        &registry_db,
        REGISTRY_DURESS_USE_REVERSE_KEY,
        Some(if use_reverse { "true" } else { "false" }),
    )?;

    let vault = DECOY_VAULT.read();
    let salt = generate_salt().to_vec();
    vault
        .enable(duress_hash.hash_string().to_string(), salt)
        .map_err(|e| anyhow!("Failed to enable decoy vault: {}", e))?;

    tracing::info!("Duress passphrase configured");
    Ok(duress_hash.hash_string().to_string())
}

/// Check if a duress passphrase is configured
pub fn has_duress_passphrase() -> Result<bool> {
    if !wallet_registry_path()?.exists() {
        return Ok(false);
    }
    let registry_db = open_wallet_registry()?;
    Ok(get_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY)?.is_some())
}

/// Get the stored duress passphrase hash (for client-side secure storage sync)
pub fn get_duress_passphrase_hash() -> Result<Option<String>> {
    if !wallet_registry_path()?.exists() {
        return Ok(None);
    }
    let registry_db = open_wallet_registry()?;
    get_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY)
}

/// Clear duress passphrase configuration
pub fn clear_duress_passphrase() -> Result<()> {
    if wallet_registry_path()?.exists() {
        let registry_db = open_wallet_registry()?;
        set_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None)?;
        set_registry_setting(&registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None)?;
    }

    let vault = DECOY_VAULT.read();
    vault
        .disable()
        .map_err(|e| anyhow!("Failed to disable decoy vault: {}", e))?;
    tracing::info!("Duress passphrase cleared");
    Ok(())
}

/// Verify duress passphrase (activates decoy mode if correct)
pub fn verify_duress_passphrase(passphrase: String, hash: String) -> Result<bool> {
    let verifier = AppPassphrase::from_hash(hash.clone());
    let is_match = verifier
        .verify(&passphrase)
        .map_err(|e| anyhow!("Failed to verify duress passphrase: {}", e))?;

    if is_match {
        let vault = DECOY_VAULT.read();
        if !vault.config().enabled {
            let salt = generate_salt().to_vec();
            let _ = vault.enable(hash, salt);
        }
        vault
            .activate_decoy()
            .map_err(|e| anyhow!("Failed to activate decoy: {}", e))?;
        ensure_decoy_wallet_state();
        tracing::warn!("Decoy vault activated via duress passphrase");
    }

    Ok(is_match)
}

/// Set decoy wallet name
pub fn set_decoy_wallet_name(name: String) -> Result<()> {
    let vault = DECOY_VAULT.read();
    vault.set_decoy_name(name);
    Ok(())
}

/// Exit decoy mode (requires real passphrase re-authentication)
pub fn exit_decoy_mode() -> Result<()> {
    let vault = DECOY_VAULT.read();
    vault.deactivate_decoy();
    tracing::info!("Exited decoy mode");
    Ok(())
}

// ============================================================================
// Seed Export (Gated Flow)
// ============================================================================

/// Start seed export flow (step 1: show warning)
pub fn start_seed_export(wallet_id: WalletId) -> Result<String> {
    ensure_not_decoy("Seed export")?;
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }
    let manager = SEED_EXPORT.write();
    let state = manager
        .start_export(wallet_id)
        .map_err(|e| anyhow!("Failed to start export: {}", e))?;

    Ok(format!("{:?}", state))
}

/// Acknowledge seed export warning (step 2)
pub fn acknowledge_seed_warning() -> Result<String> {
    ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .acknowledge_warning()
        .map_err(|e| anyhow!("Failed to acknowledge: {}", e))?;

    Ok(format!("{:?}", state))
}

/// Complete biometric step (step 3)
pub fn complete_seed_biometric(success: bool) -> Result<String> {
    ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .complete_biometric(success)
        .map_err(|e| anyhow!("Failed to complete biometric: {}", e))?;

    Ok(format!("{:?}", state))
}

/// Skip biometric (when not available)
pub fn skip_seed_biometric() -> Result<String> {
    ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .skip_biometric()
        .map_err(|e| anyhow!("Failed to skip biometric: {}", e))?;

    Ok(format!("{:?}", state))
}

/// Verify passphrase and get seed (step 4 - final)
///
/// This is the final step of the gated seed export flow.
/// Verifies passphrase against stored Argon2id hash before returning the seed.
///
/// Note: Only works for wallets created/restored from seed.
/// Wallets imported from private key or watch-only wallets cannot export seed.
pub fn export_seed_with_passphrase(wallet_id: WalletId, passphrase: String) -> Result<Vec<String>> {
    ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.read();

    // Verify flow state
    if manager.state() != ExportFlowState::AwaitingPassphrase {
        return Err(anyhow!(
            "Invalid export flow state. Complete previous steps first."
        ));
    }
    drop(manager);

    // Get wallet and verify
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }

    let registry_db = open_wallet_registry()?;
    let passphrase_hash = get_registry_setting(&registry_db, REGISTRY_APP_PASSPHRASE_KEY)?
        .ok_or_else(|| anyhow!("Passphrase not configured"))?;
    {
        let manager = SEED_EXPORT.write();
        manager.set_passphrase_hash(passphrase_hash);
    }
    {
        let manager = SEED_EXPORT.read();
        let verified = manager.verify_passphrase(&passphrase)?;
        if !verified {
            return Err(anyhow!("Invalid passphrase"));
        }
    }

    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Check if mnemonic is stored (wallet was created/restored from seed)
    let mnemonic_bytes = secret.encrypted_mnemonic.ok_or_else(|| {
        anyhow!("Seed not available. This wallet was imported from private key or is watch-only.")
    })?;

    // Decrypt mnemonic (database encryption handles decryption)
    let mnemonic = String::from_utf8(mnemonic_bytes)
        .map_err(|e| anyhow!("Failed to decode mnemonic: {}", e))?;

    let words: Vec<String> = mnemonic.split_whitespace().map(String::from).collect();

    let result = {
        let manager = SEED_EXPORT.read();
        manager.complete_export_verified(words)?
    };

    tracing::info!(
        "Seed exported for wallet {} (gated flow completed)",
        wallet_id
    );

    Ok(result.words().to_vec())
}

/// Export seed using cached app passphrase (after biometric approval).
pub fn export_seed_with_cached_passphrase(wallet_id: WalletId) -> Result<Vec<String>> {
    ensure_not_decoy("Seed export")?;
    let passphrase = app_passphrase()?;
    export_seed_with_passphrase(wallet_id, passphrase)
}

/// Cancel seed export flow
pub fn cancel_seed_export() -> Result<()> {
    let manager = SEED_EXPORT.write();
    manager.cancel();
    Ok(())
}

/// Get current seed export flow state
pub fn get_seed_export_state() -> Result<String> {
    let manager = SEED_EXPORT.read();
    Ok(format!("{:?}", manager.state()))
}

/// Check if screenshots are blocked during export
pub fn are_seed_screenshots_blocked() -> Result<bool> {
    let manager = SEED_EXPORT.read();
    Ok(manager.are_screenshots_blocked())
}

/// Get clipboard auto-clear remaining seconds
pub fn get_seed_clipboard_remaining() -> Result<Option<u64>> {
    let manager = SEED_EXPORT.read();
    Ok(manager.clipboard_remaining_seconds())
}

/// Get seed export warning messages
pub fn get_seed_export_warnings() -> Result<SeedExportWarnings> {
    Ok(SeedExportWarnings {
        primary: seed_warnings::PRIMARY_WARNING.to_string(),
        secondary: seed_warnings::SECONDARY_WARNING.to_string(),
        backup_instructions: seed_warnings::BACKUP_INSTRUCTIONS.to_string(),
        clipboard_warning: seed_warnings::CLIPBOARD_WARNING.to_string(),
    })
}

/// Seed export warning messages for UI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeedExportWarnings {
    pub primary: String,
    pub secondary: String,
    pub backup_instructions: String,
    pub clipboard_warning: String,
}

// ============================================================================
// Watch-Only / Viewing Key Export/Import
// ============================================================================

/// Export Sapling viewing key from full wallet (for creating watch-only on another device)
pub fn export_ivk_secure(wallet_id: WalletId) -> Result<String> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export viewing key from watch-only wallet"));
    }
    // Load wallet secret from encrypted storage and extract viewing key (same logic as export_ivk)
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Derive xFVK from stored spending key
    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
    let network_type_str = wallet.network_type.as_deref().unwrap_or("mainnet");
    let network_type = match network_type_str {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };
    let ivk = extsk.to_xfvk_bech32_for_network(network_type);

    let manager = WATCH_ONLY.read();
    let result = manager
        .export_ivk(&wallet_id, ivk)
        .map_err(|e| anyhow!("Failed to export viewing key: {}", e))?;

    tracing::info!("Viewing key exported for wallet {}", wallet_id);

    Ok(result.ivk().to_string())
}

/// Import viewing key to create watch-only wallet
pub fn import_ivk_as_watch_only(
    name: String,
    ivk: String,
    birthday_height: u32,
) -> Result<WalletId> {
    // Validate import request
    let request = IvkImportRequest::new(name.clone(), ivk.clone(), birthday_height);
    let manager = WATCH_ONLY.read();
    manager
        .validate_import(&request)
        .map_err(|e| anyhow!("Invalid viewing key import: {}", e))?;

    let wallet_id = import_ivk(name, Some(ivk), None, birthday_height)?;
    tracing::info!("Watch-only wallet created: {}", wallet_id);
    Ok(wallet_id)
}

/// Get watch-only capabilities for a wallet
pub fn get_watch_only_capabilities(wallet_id: WalletId) -> Result<WatchOnlyCapabilitiesInfo> {
    let wallet = get_wallet_meta(&wallet_id)?;

    let caps = if wallet.watch_only {
        WatchOnlyCapabilities::watch_only()
    } else {
        WatchOnlyCapabilities::full_wallet()
    };

    Ok(WatchOnlyCapabilitiesInfo {
        can_view_incoming: caps.can_view_incoming,
        can_view_outgoing: caps.can_view_outgoing,
        can_spend: caps.can_spend,
        can_export_seed: caps.can_export_seed,
        can_generate_addresses: caps.can_generate_addresses,
        is_watch_only: wallet.watch_only,
    })
}

/// Watch-only capabilities for FFI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatchOnlyCapabilitiesInfo {
    pub can_view_incoming: bool,
    pub can_view_outgoing: bool,
    pub can_spend: bool,
    pub can_export_seed: bool,
    pub can_generate_addresses: bool,
    pub is_watch_only: bool,
}

/// Get watch-only banner info for a wallet
pub fn get_watch_only_banner(wallet_id: WalletId) -> Result<Option<WatchOnlyBannerInfo>> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if !wallet.watch_only {
        return Ok(None);
    }

    let banner = WatchOnlyBanner::incoming_only();

    Ok(Some(WatchOnlyBannerInfo {
        banner_type: format!("{:?}", banner.banner_type),
        title: banner.title,
        subtitle: banner.subtitle,
        icon: banner.icon,
    }))
}

/// Watch-only banner info for FFI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatchOnlyBannerInfo {
    pub banner_type: String,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
}

/// Check if viewing key clipboard should be cleared
pub fn get_ivk_clipboard_remaining() -> Result<Option<u64>> {
    let manager = WATCH_ONLY.read();
    Ok(manager.clipboard_remaining_seconds())
}

/// Get build information for verification
pub fn get_build_info() -> Result<BuildInfo> {
    // Determine target triple at compile time using cfg attributes
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    let target_triple = "x86_64-pc-windows-msvc";

    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    let target_triple = "x86_64-unknown-linux-gnu";

    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    let target_triple = "x86_64-apple-darwin";

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    let target_triple = "aarch64-apple-darwin";

    #[cfg(all(target_arch = "aarch64", target_os = "android"))]
    let target_triple = "aarch64-linux-android";

    #[cfg(all(target_arch = "aarch64", target_os = "ios"))]
    let target_triple = "aarch64-apple-ios";

    #[cfg(not(any(
        all(target_arch = "x86_64", target_os = "windows"),
        all(target_arch = "x86_64", target_os = "linux"),
        all(target_arch = "x86_64", target_os = "macos"),
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "aarch64", target_os = "android"),
        all(target_arch = "aarch64", target_os = "ios"),
    )))]
    let target_triple = "unknown";

    Ok(BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
        rust_version: option_env!("CARGO_PKG_RUST_VERSION")
            .unwrap_or("unknown")
            .to_string(),
        target_triple: target_triple.to_string(),
    })
}

/// Get sync logs for diagnostics
pub fn get_sync_logs(
    wallet_id: WalletId,
    limit: Option<u32>,
) -> Result<Vec<crate::models::SyncLogEntryFfi>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(200);

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let logs = repo.get_sync_logs(&wallet_id, limit)?;

    Ok(logs
        .into_iter()
        .map(
            |(timestamp, level, module, message)| crate::models::SyncLogEntryFfi {
                timestamp,
                level,
                module,
                message,
            },
        )
        .collect())
}

/// Get checkpoint details at specific height
pub fn get_checkpoint_details(_wallet_id: WalletId, height: u32) -> Result<Option<CheckpointInfo>> {
    let (db, _repo) = open_wallet_db_for(&_wallet_id)?;

    // Use CheckpointManager to get checkpoint at height
    use pirate_storage_sqlite::CheckpointManager;
    let manager = CheckpointManager::new(db.conn());

    match manager.get_at_height(height)? {
        Some(checkpoint) => Ok(Some(CheckpointInfo {
            height: checkpoint.height,
            timestamp: checkpoint.timestamp,
        })),
        None => Ok(None),
    }
}

/// Test connection to a lightwalletd endpoint
pub async fn test_node(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    run_on_runtime(move || test_node_inner(url, tls_pin)).await
}

async fn test_node_inner(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    use pirate_sync_lightd::client::{LightClient, LightClientConfig};
    use std::time::Instant;

    let start_time = Instant::now();

    // Extract all needed data before async context to ensure Send
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    tracing::info!(
        "test_node: Using transport mode: {:?}, endpoint: {}",
        transport,
        url
    );

    // Normalize endpoint URL - ensure it has the correct format for tonic
    // Tonic requires format: https://host:port or http://host:port
    let normalized_url = url.trim().to_string();

    // Determine if TLS is enabled
    // Explicitly check for http:// (no TLS) vs https:// (TLS) vs no protocol (default to TLS)
    let tls_enabled = if normalized_url.starts_with("http://") {
        false // Explicitly disable TLS for http:// URLs
    } else if normalized_url.starts_with("https://") {
        true // Explicitly enable TLS for https:// URLs
    } else {
        // No protocol specified - default to TLS (https) for security
        true
    };

    // Remove protocol if present to parse host:port
    let host_port = if let Some(stripped) = normalized_url.strip_prefix("https://") {
        stripped.to_string()
    } else if let Some(stripped) = normalized_url.strip_prefix("http://") {
        stripped.to_string()
    } else {
        normalized_url.clone()
    };

    // Remove trailing slash
    let host_port = host_port.trim_end_matches('/').to_string();

    // Parse host and port
    let (host, port) = if host_port.contains(':') {
        let parts: Vec<&str> = host_port.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid endpoint format: expected host:port"));
        }
        (
            parts[0].to_string(),
            parts[1]
                .parse::<u16>()
                .map_err(|_| anyhow!("Invalid port number in endpoint"))?,
        )
    } else {
        // No port specified, use default
        (host_port, 9067)
    };

    // Check if host is an IP address
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();

    // Reconstruct endpoint URL in correct format for tonic
    let scheme = if tls_enabled { "https" } else { "http" };
    let endpoint = format!("{}://{}:{}", scheme, host, port);

    tracing::info!(
        "test_node: Parsed endpoint URL: {} (TLS: {}, host: {}, port: {}, is_ip: {})",
        endpoint,
        tls_enabled,
        host,
        port,
        is_ip_address
    );

    // Create client config (all data is Send-safe now)
    // For TLS SNI: If connecting via IP address, we need to use the hostname from the certificate
    // The certificate is likely issued for lightd1.piratechain.com, not the IP
    // So we should use the hostname for SNI even when connecting via IP
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            // If connecting via IP, use the known hostname for SNI to match the certificate
            // This is required because the certificate is issued for the hostname, not the IP
            tracing::info!("test_node: Connecting via IP {}, using hostname 'lightd1.piratechain.com' for TLS SNI to match certificate", host);
            Some("lightd1.piratechain.com".to_string())
        } else {
            // Use hostname for SNI (required for proper TLS handshake with hostname-based certs)
            Some(host.clone())
        }
    } else {
        None
    };

    let actual_pin = if tls_enabled {
        let server_name = tls_server_name.clone().unwrap_or_else(|| host.clone());
        match pirate_sync_lightd::fetch_spki_pin(
            &host,
            port,
            Some(server_name),
            transport,
            socks5_url.clone(),
        )
        .await
        {
            Ok(pin) => Some(pin),
            Err(e) => {
                tracing::warn!("test_node: Failed to extract SPKI pin: {}", e);
                None
            }
        }
    } else {
        None
    };

    let tls_pin_matched = match (tls_pin.as_deref(), actual_pin.as_deref()) {
        (Some(expected), Some(actual)) => {
            let expected = expected.strip_prefix("sha256/").unwrap_or(expected);
            let actual = actual.strip_prefix("sha256/").unwrap_or(actual);
            Some(expected == actual)
        }
        (Some(_), None) => None,
        _ => None,
    };

    let config = LightClientConfig {
        endpoint: endpoint.clone(),
        transport,
        socks5_url,
        tls: pirate_sync_lightd::client::TlsConfig {
            enabled: tls_enabled,
            spki_pin: tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: Default::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    let client = LightClient::with_config(config);

    // Try to connect and get latest block
    tracing::info!(
        "test_node: Attempting to connect to {} (hostname: {})",
        endpoint,
        host
    );
    match client.connect().await {
        Ok(_) => {
            tracing::info!("test_node: Connection successful, fetching latest block...");
            match client.get_latest_block().await {
                Ok(height) => {
                    tracing::info!(
                        "test_node: Successfully retrieved latest block height: {}",
                        height
                    );
                    // Try to get server info if available
                    let (server_version, chain_name) = match client.get_lightd_info().await {
                        Ok(info) => {
                            tracing::info!(
                                "test_node: Server info - version: {}, chain: {}",
                                info.version,
                                info.chain_name
                            );
                            (Some(info.version), Some(info.chain_name))
                        }
                        Err(e) => {
                            tracing::warn!("test_node: Failed to get server info: {}", e);
                            (None, None)
                        }
                    };

                    let response_time = start_time.elapsed().as_millis() as u64;

                    Ok(crate::models::NodeTestResult {
                        success: true,
                        latest_block_height: Some(height),
                        transport_mode: format!("{:?}", transport),
                        tls_enabled,
                        tls_pin_matched,
                        expected_pin: tls_pin,
                        actual_pin,
                        error_message: None,
                        response_time_ms: response_time,
                        server_version,
                        chain_name,
                    })
                }
                Err(e) => {
                    let response_time = start_time.elapsed().as_millis() as u64;
                    // Clean up error message - remove duplicate "transport error" if present
                    let error_msg = format!("{}", e);
                    let cleaned_error = if error_msg.contains("transport error: transport error") {
                        error_msg.replace("transport error: transport error", "transport error")
                    } else {
                        error_msg
                    };

                    Ok(crate::models::NodeTestResult {
                        success: false,
                        latest_block_height: None,
                        transport_mode: format!("{:?}", transport),
                        tls_enabled,
                        tls_pin_matched,
                        expected_pin: tls_pin,
                        actual_pin,
                        error_message: Some(format!(
                            "Failed to get latest block: {}",
                            cleaned_error
                        )),
                        response_time_ms: response_time,
                        server_version: None,
                        chain_name: None,
                    })
                }
            }
        }
        Err(e) => {
            let response_time = start_time.elapsed().as_millis() as u64;
            tracing::error!(
                "test_node: Connection failed after {}ms: {}",
                response_time,
                e
            );

            // Clean up error message - remove duplicate "transport error" if present
            let error_msg = format!("{}", e);
            let cleaned_error = if error_msg.contains("transport error: transport error") {
                error_msg.replace("transport error: transport error", "transport error")
            } else if error_msg.starts_with("Transport error: ") {
                // Remove redundant "Transport error: " prefix if the inner error already says "transport error"
                let inner = error_msg
                    .strip_prefix("Transport error: ")
                    .unwrap_or(&error_msg);
                if inner.contains("transport error") {
                    inner.to_string()
                } else {
                    error_msg
                }
            } else {
                error_msg
            };

            // Provide more helpful error message
            let final_error = if cleaned_error.contains("dns")
                || cleaned_error.contains("name resolution")
                || cleaned_error.contains("failed to lookup")
                || cleaned_error.contains("Name or service not known")
            {
                format!("DNS resolution failed: {}. The hostname '{}' cannot be resolved to an IP address. This may be a DNS configuration issue on your network. Try using the IP address directly (e.g., https://64.23.167.130:9067) or check your DNS settings.", cleaned_error, host)
            } else if cleaned_error.contains("transport error") {
                format!("Connection failed: {}. The connection attempt failed before we could query the latest block height. This could be due to DNS resolution failure, TLS/certificate issues, or network connectivity problems. Check your network connection, DNS settings, and endpoint URL.", cleaned_error)
            } else {
                format!("Connection failed: {}. Latest block height not retrieved because connection failed.", cleaned_error)
            };

            Ok(crate::models::NodeTestResult {
                success: false,
                latest_block_height: None, // No block height retrieved because connection failed
                transport_mode: format!("{:?}", transport),
                tls_enabled,
                tls_pin_matched,
                expected_pin: tls_pin,
                actual_pin,
                error_message: Some(final_error),
                response_time_ms: response_time,
                server_version: None,
                chain_name: None,
            })
        }
    }
}
