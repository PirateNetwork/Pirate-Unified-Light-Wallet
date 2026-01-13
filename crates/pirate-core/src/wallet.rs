//! Wallet management

use crate::{Error, Result};
use crate::keys::{ExtendedSpendingKey, ExtendedFullViewingKey, IncomingViewingKey, PaymentAddress, OrchardExtendedFullViewingKey};
use crate::notes::Note;
use orchard::keys::IncomingViewingKey as OrchardIncomingViewingKey;
use pirate_params::Network;
use hex;

/// Wallet type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletType {
    /// Full wallet (can spend)
    Full,
    /// Watch-only (incoming view only, cannot spend)
    WatchOnly,
}

/// Wallet instance
pub struct Wallet {
    wallet_type: WalletType,
    spending_key: Option<ExtendedSpendingKey>,
    viewing_key: Option<ExtendedFullViewingKey>,
    incoming_ivk: Option<IncomingViewingKey>, // Sapling IVK
    orchard_viewing_key: Option<OrchardExtendedFullViewingKey>,
    orchard_incoming_ivk: Option<OrchardIncomingViewingKey>, // Orchard IVK
    notes: Vec<Note>,
}

impl Wallet {
    /// Create from mnemonic (full wallet)
    pub fn from_mnemonic(mnemonic: &str, passphrase: &str) -> Result<Self> {
        let network = Network::mainnet();
        let spending_key = ExtendedSpendingKey::from_mnemonic_with_account(
            mnemonic,
            passphrase,
            network.network_type,
            0,
        )?;
        let viewing_key = spending_key.to_extended_fvk();
        
        // Derive Orchard keys from the same seed
        // Get seed bytes from mnemonic (same as used for Sapling)
        let seed_bytes = crate::keys::ExtendedSpendingKey::seed_bytes_from_mnemonic(mnemonic, passphrase)?;
        let orchard_master = crate::keys::OrchardExtendedSpendingKey::master(&seed_bytes)?;
        let orchard_extsk = orchard_master.derive_account(network.coin_type, 0)?;
        let orchard_viewing_key = orchard_extsk.to_extended_fvk();
        
        Ok(Self {
            wallet_type: WalletType::Full,
            spending_key: Some(spending_key),
            viewing_key: Some(viewing_key),
            incoming_ivk: None,
            orchard_viewing_key: Some(orchard_viewing_key),
            orchard_incoming_ivk: None,
            notes: Vec::new(),
        })
    }

    /// Create from viewing key (watch-only wallet).
    ///
    /// Accepts Sapling xFVK (zxviews...) or Orchard extended viewing key.
    pub fn from_ivk(ivk: &str) -> Result<Self> {
        let mut sapling_viewing_key = None;
        let mut sapling_ivk = None;
        let mut orchard_viewing_key = None;
        let mut orchard_ivk = None;

        if let Ok((dfvk, ivk)) = parse_sapling_watch_key(ivk) {
            sapling_viewing_key = dfvk;
            sapling_ivk = Some(ivk);
        } else if let Ok((fvk, ivk)) = parse_orchard_watch_key(ivk) {
            orchard_viewing_key = fvk;
            orchard_ivk = Some(ivk);
        }

        if sapling_ivk.is_none() && orchard_ivk.is_none() {
            return Err(Error::InvalidKey(
                "Invalid viewing key format - must be Sapling xFVK or Orchard extended viewing key".to_string(),
            ));
        }

        Ok(Self {
            wallet_type: WalletType::WatchOnly,
            spending_key: None,
            viewing_key: sapling_viewing_key,
            incoming_ivk: sapling_ivk,
            orchard_viewing_key,
            orchard_incoming_ivk: orchard_ivk,
            notes: Vec::new(),
        })
    }
    
    /// Create from both Sapling and Orchard viewing keys (watch-only wallet).
    pub fn from_ivks(sapling_ivk: Option<&str>, orchard_ivk: Option<&str>) -> Result<Self> {
        let mut sapling_viewing_key = None;
        let mut sapling = None;
        if let Some(value) = sapling_ivk {
            let (dfvk, ivk) = parse_sapling_watch_key(value)?;
            sapling_viewing_key = dfvk;
            sapling = Some(ivk);
        }

        let mut orchard_viewing_key = None;
        let mut orchard = None;
        if let Some(value) = orchard_ivk {
            let (fvk, ivk) = parse_orchard_watch_key(value)?;
            orchard_viewing_key = fvk;
            orchard = Some(ivk);
        }
        
        if sapling.is_none() && orchard.is_none() {
            return Err(Error::InvalidKey("At least one viewing key (Sapling or Orchard) must be provided".to_string()));
        }

        Ok(Self {
            wallet_type: WalletType::WatchOnly,
            spending_key: None,
            viewing_key: sapling_viewing_key,
            incoming_ivk: sapling,
            orchard_viewing_key,
            orchard_incoming_ivk: orchard,
            notes: Vec::new(),
        })
    }

    /// Get wallet type
    pub fn wallet_type(&self) -> WalletType {
        self.wallet_type
    }

    /// Get the wallet's spending key, if this is a full wallet.
    pub fn spending_key(&self) -> Option<&ExtendedSpendingKey> {
        self.spending_key.as_ref()
    }

    /// Get the wallet's viewing key, if available.
    pub fn viewing_key(&self) -> Option<&ExtendedFullViewingKey> {
        self.viewing_key.as_ref()
    }

    /// Get the wallet's incoming viewing key (IVK), if this is a watch-only wallet.
    pub fn incoming_ivk(&self) -> Option<&IncomingViewingKey> {
        self.incoming_ivk.as_ref()
    }
    
    /// Get the wallet's Orchard incoming viewing key (IVK), if this is a watch-only wallet.
    pub fn orchard_incoming_ivk(&self) -> Option<&OrchardIncomingViewingKey> {
        self.orchard_incoming_ivk.as_ref()
    }
    
    /// Get the wallet's Orchard viewing key, if available.
    pub fn orchard_viewing_key(&self) -> Option<&OrchardExtendedFullViewingKey> {
        self.orchard_viewing_key.as_ref()
    }

    /// Check if wallet is watch-only
    pub fn is_watch_only(&self) -> bool {
        self.wallet_type == WalletType::WatchOnly
    }

    /// Export Sapling IVK
    pub fn export_ivk(&self) -> String {
        if let Some(ivk) = self.incoming_ivk.as_ref() {
            ivk.to_string()
        } else {
            // Full wallet
            self.viewing_key
                .as_ref()
                .expect("full wallet must have viewing key")
                .to_ivk_string()
        }
    }
    
    /// Export Orchard Extended Full Viewing Key as Bech32 (for watch-only wallets)
    /// 
    /// Returns Bech32-encoded string with "pirate-extended-viewing-key" HRP.
    /// Uses the standard Orchard viewing key Bech32 format.
    pub fn export_orchard_viewing_key(&self) -> Option<String> {
        if let Some(fvk) = self.orchard_viewing_key.as_ref() {
            fvk.to_bech32().ok()
        } else {
            None
        }
    }
    
    /// Export Orchard IVK (returns hex-encoded 64 bytes) - DEPRECATED
    /// 
    /// Use export_orchard_viewing_key() instead for watch-only wallets.
    /// This method is kept for backward compatibility.
    #[deprecated(note = "Use export_orchard_viewing_key() instead")]
    pub fn export_orchard_ivk(&self) -> Option<String> {
        if let Some(ivk) = self.orchard_incoming_ivk.as_ref() {
            Some(hex::encode(ivk.to_bytes()))
        } else if let Some(fvk) = self.orchard_viewing_key.as_ref() {
            // Full wallet - derive IVK from viewing key
            Some(hex::encode(fvk.to_ivk_bytes()))
        } else {
            None
        }
    }

    /// Get default address
    pub fn default_address(&self) -> Result<PaymentAddress> {
        match self.wallet_type {
            WalletType::Full => Ok(self
                .viewing_key
                .as_ref()
                .expect("full wallet must have viewing key")
                .derive_address(0)),
            WalletType::WatchOnly => Err(Error::InvalidKey(
                "Watch-only wallet (IVK) cannot derive receiving addresses; IVK supports incoming detection only"
                    .to_string(),
            )),
        }
    }

    /// Get balance
    pub fn balance(&self) -> u64 {
        self.notes.iter().filter(|n| !n.spent).map(|n| n.value).sum()
    }

    /// Add note
    pub fn add_note(&mut self, note: Note) {
        self.notes.push(note);
    }
}

fn parse_sapling_watch_key(
    value: &str,
) -> Result<(Option<ExtendedFullViewingKey>, IncomingViewingKey)> {
    if let Ok(dfvk) = ExtendedFullViewingKey::from_xfvk_bech32_any(value) {
        let ivk = dfvk.to_ivk();
        return Ok((Some(dfvk), ivk));
    }

    Err(Error::InvalidKey(
        "Invalid Sapling viewing key format (expected xFVK)".to_string(),
    ))
}

fn parse_orchard_watch_key(
    value: &str,
) -> Result<(Option<OrchardExtendedFullViewingKey>, OrchardIncomingViewingKey)> {
    if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bech32_any(value) {
        let ivk_bytes = fvk.to_ivk_bytes();
        let ivk_ct = OrchardIncomingViewingKey::from_bytes(&ivk_bytes);
        let ivk: Option<OrchardIncomingViewingKey> = ivk_ct.into();
        let ivk = ivk.ok_or_else(|| Error::InvalidKey("Invalid Orchard IVK bytes".to_string()))?;
        return Ok((Some(fvk), ivk));
    }

    Err(Error::InvalidKey(
        "Invalid Orchard viewing key format (expected extended viewing key)".to_string(),
    ))
}

