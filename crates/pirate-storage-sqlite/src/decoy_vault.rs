//! Decoy vault for panic PIN functionality
//!
//! When the user enters their panic PIN instead of the real PIN/passphrase,
//! the wallet shows a decoy view with empty balance and no transaction history.
//! This provides plausible deniability under duress.

#![allow(missing_docs)]

use crate::{Error, Result};
use rusqlite::params;
use std::sync::Arc;

/// Decoy vault state
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VaultMode {
    /// Normal wallet mode (real data)
    #[default]
    Real,
    /// Decoy mode (empty vault)
    Decoy,
}

/// Decoy vault configuration stored in DB
#[derive(Debug, Clone)]
pub struct DecoyVaultConfig {
    /// Whether decoy vault is enabled
    pub enabled: bool,
    /// Panic PIN hash (Argon2id)
    pub panic_pin_hash: Option<String>,
    /// Salt for panic PIN
    pub panic_pin_salt: Option<Vec<u8>>,
    /// Decoy wallet name shown when activated
    pub decoy_wallet_name: String,
    /// Created timestamp
    pub created_at: i64,
    /// Last activated timestamp (for logging suspicious access)
    pub last_activated: Option<i64>,
    /// Activation count (for forensics)
    pub activation_count: u32,
}

impl Default for DecoyVaultConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            panic_pin_hash: None,
            panic_pin_salt: None,
            decoy_wallet_name: "Wallet".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            last_activated: None,
            activation_count: 0,
        }
    }
}

/// Decoy vault manager
pub struct DecoyVaultManager {
    /// Current vault mode
    mode: std::sync::RwLock<VaultMode>,
    /// Configuration
    config: std::sync::RwLock<DecoyVaultConfig>,
}

impl DecoyVaultManager {
    /// Create new manager
    pub fn new() -> Self {
        Self {
            mode: std::sync::RwLock::new(VaultMode::Real),
            config: std::sync::RwLock::new(DecoyVaultConfig::default()),
        }
    }

    /// Create with existing config
    pub fn with_config(config: DecoyVaultConfig) -> Self {
        Self {
            mode: std::sync::RwLock::new(VaultMode::Real),
            config: std::sync::RwLock::new(config),
        }
    }

    /// Get current vault mode
    pub fn mode(&self) -> VaultMode {
        *self.mode.read().unwrap()
    }

    /// Check if in decoy mode
    pub fn is_decoy_mode(&self) -> bool {
        self.mode() == VaultMode::Decoy
    }

    /// Get config
    pub fn config(&self) -> DecoyVaultConfig {
        self.config.read().unwrap().clone()
    }

    /// Enable decoy vault with panic PIN
    pub fn enable(&self, panic_pin_hash: String, salt: Vec<u8>) -> Result<()> {
        let mut config = self.config.write().unwrap();
        config.enabled = true;
        config.panic_pin_hash = Some(panic_pin_hash);
        config.panic_pin_salt = Some(salt);
        config.created_at = chrono::Utc::now().timestamp();

        tracing::info!("Decoy vault enabled");
        Ok(())
    }

    /// Disable decoy vault
    pub fn disable(&self) -> Result<()> {
        let mut config = self.config.write().unwrap();
        config.enabled = false;
        config.panic_pin_hash = None;
        config.panic_pin_salt = None;

        // Reset to real mode
        *self.mode.write().unwrap() = VaultMode::Real;

        tracing::info!("Decoy vault disabled");
        Ok(())
    }

    /// Activate decoy mode (called when panic PIN is entered)
    pub fn activate_decoy(&self) -> Result<()> {
        let mut config = self.config.write().unwrap();
        if !config.enabled {
            return Err(Error::Security("Decoy vault not enabled".to_string()));
        }

        *self.mode.write().unwrap() = VaultMode::Decoy;
        config.last_activated = Some(chrono::Utc::now().timestamp());
        config.activation_count = config.activation_count.saturating_add(1);

        // Log activation (for user's later review if needed)
        tracing::warn!("Decoy vault activated (count: {})", config.activation_count);

        Ok(())
    }

    /// Deactivate decoy mode (return to real wallet)
    /// This requires re-authentication with the real passphrase
    pub fn deactivate_decoy(&self) {
        *self.mode.write().unwrap() = VaultMode::Real;
        tracing::info!("Decoy vault deactivated");
    }

    /// Check panic PIN against stored hash
    pub fn verify_panic_pin(&self, pin: &str) -> Result<bool> {
        let config = self.config.read().unwrap();

        if !config.enabled {
            return Ok(false);
        }

        let Some(ref stored_hash) = config.panic_pin_hash else {
            return Ok(false);
        };

        // Use PanicPin verifier
        let panic_pin = crate::security::PanicPin::from_hash(stored_hash.clone());
        panic_pin.verify(pin)
    }

    /// Set decoy wallet name
    pub fn set_decoy_name(&self, name: String) {
        self.config.write().unwrap().decoy_wallet_name = name;
    }

    /// Get decoy wallet name
    pub fn decoy_name(&self) -> String {
        self.config.read().unwrap().decoy_wallet_name.clone()
    }
}

impl Default for DecoyVaultManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Decoy balance (always zero)
#[derive(Debug, Clone, Default)]
pub struct DecoyBalance {
    pub total: u64,
    pub spendable: u64,
    pub pending: u64,
}

impl DecoyBalance {
    pub fn new() -> Self {
        Self {
            total: 0,
            spendable: 0,
            pending: 0,
        }
    }
}

/// Decoy transaction list (always empty)
#[derive(Debug, Clone, Default)]
pub struct DecoyTransactionList {
    pub transactions: Vec<()>, // Empty
}

impl DecoyTransactionList {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }
}

/// Decoy wallet metadata
#[derive(Debug, Clone)]
pub struct DecoyWalletMeta {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub watch_only: bool,
    pub birthday_height: u32,
}

impl DecoyWalletMeta {
    pub fn new(name: String) -> Self {
        Self {
            id: "decoy_wallet".to_string(),
            name,
            created_at: chrono::Utc::now().timestamp(),
            watch_only: false,
            birthday_height: 2_000_000,
        }
    }
}

/// Storage interface for decoy vault config
pub struct DecoyVaultStorage {
    db: Arc<std::sync::Mutex<crate::Database>>,
}

impl DecoyVaultStorage {
    /// Create new storage
    pub fn new(db: Arc<std::sync::Mutex<crate::Database>>) -> Self {
        Self { db }
    }

    /// Save config to database
    pub fn save_config(&self, config: &DecoyVaultConfig) -> Result<()> {
        let db = self.db.lock().unwrap();
        let conn = db.conn();

        conn.execute(
            "INSERT OR REPLACE INTO decoy_vault_config 
             (id, enabled, panic_pin_hash, panic_pin_salt, decoy_wallet_name, 
              created_at, last_activated, activation_count)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                config.enabled,
                config.panic_pin_hash,
                config.panic_pin_salt,
                config.decoy_wallet_name,
                config.created_at,
                config.last_activated,
                config.activation_count,
            ],
        )?;

        Ok(())
    }

    /// Load config from database
    pub fn load_config(&self) -> Result<Option<DecoyVaultConfig>> {
        let db = self.db.lock().unwrap();
        let conn = db.conn();

        let mut stmt = conn.prepare(
            "SELECT enabled, panic_pin_hash, panic_pin_salt, decoy_wallet_name,
                    created_at, last_activated, activation_count
             FROM decoy_vault_config WHERE id = 1",
        )?;

        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            Ok(Some(DecoyVaultConfig {
                enabled: row.get(0)?,
                panic_pin_hash: row.get(1)?,
                panic_pin_salt: row.get(2)?,
                decoy_wallet_name: row.get(3)?,
                created_at: row.get(4)?,
                last_activated: row.get(5)?,
                activation_count: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Delete config
    pub fn delete_config(&self) -> Result<()> {
        let db = self.db.lock().unwrap();
        let conn = db.conn();

        conn.execute("DELETE FROM decoy_vault_config WHERE id = 1", [])?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_mode_default() {
        let manager = DecoyVaultManager::new();
        assert_eq!(manager.mode(), VaultMode::Real);
        assert!(!manager.is_decoy_mode());
    }

    #[test]
    fn test_enable_disable() {
        let manager = DecoyVaultManager::new();

        // Enable
        manager
            .enable("test_hash".to_string(), vec![1, 2, 3, 4])
            .unwrap();
        assert!(manager.config().enabled);

        // Activate
        manager.activate_decoy().unwrap();
        assert!(manager.is_decoy_mode());
        assert_eq!(manager.config().activation_count, 1);

        // Deactivate
        manager.deactivate_decoy();
        assert!(!manager.is_decoy_mode());

        // Disable
        manager.disable().unwrap();
        assert!(!manager.config().enabled);
    }

    #[test]
    fn test_decoy_balance() {
        let balance = DecoyBalance::new();
        assert_eq!(balance.total, 0);
        assert_eq!(balance.spendable, 0);
        assert_eq!(balance.pending, 0);
    }

    #[test]
    fn test_decoy_wallet_name() {
        let manager = DecoyVaultManager::new();
        manager.set_decoy_name("My Empty Wallet".to_string());
        assert_eq!(manager.decoy_name(), "My Empty Wallet");
    }
}
