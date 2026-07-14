//! Wallet management

use crate::keys::{
    ExtendedFullViewingKey, ExtendedSpendingKey, IncomingViewingKey,
    IronwoodExtendedFullViewingKey, PaymentAddress,
};
use crate::mnemonic::MnemonicLanguage;
use crate::notes::Note;
use crate::{Error, Result};
use orchard::keys::IncomingViewingKey as IronwoodIncomingViewingKey;
use pirate_params::Network;

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
    ironwood_viewing_key: Option<IronwoodExtendedFullViewingKey>,
    ironwood_incoming_ivk: Option<IronwoodIncomingViewingKey>, // Ironwood IVK
    notes: Vec<Note>,
}

impl Wallet {
    /// Create from mnemonic (full wallet)
    pub fn from_mnemonic(mnemonic: &str) -> Result<Self> {
        Self::from_mnemonic_in_language(mnemonic, None)
    }

    /// Create from mnemonic (full wallet) with an explicit or autodetected language.
    pub fn from_mnemonic_in_language(
        mnemonic: &str,
        language: Option<MnemonicLanguage>,
    ) -> Result<Self> {
        let network = Network::mainnet();
        let spending_key = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
            mnemonic,
            network.network_type,
            0,
            language,
        )?;
        let viewing_key = spending_key.to_extended_fvk();

        // Derive Ironwood keys from the same seed
        // Get seed bytes from mnemonic (same as used for Sapling)
        let seed_bytes = crate::keys::ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
            mnemonic, language,
        )?;
        let ironwood_master = crate::keys::IronwoodExtendedSpendingKey::master(&seed_bytes)?;
        let ironwood_extsk = ironwood_master.derive_account(network.coin_type, 0)?;
        let ironwood_viewing_key = ironwood_extsk.to_extended_fvk();

        Ok(Self {
            wallet_type: WalletType::Full,
            spending_key: Some(spending_key),
            viewing_key: Some(viewing_key),
            incoming_ivk: None,
            ironwood_viewing_key: Some(ironwood_viewing_key),
            ironwood_incoming_ivk: None,
            notes: Vec::new(),
        })
    }

    /// Create from viewing key (watch-only wallet).
    ///
    /// Accepts Sapling xFVK (zxviews...) or Ironwood extended viewing key.
    pub fn from_ivk(ivk: &str) -> Result<Self> {
        let mut sapling_viewing_key = None;
        let mut sapling_ivk = None;
        let mut ironwood_viewing_key = None;
        let mut ironwood_ivk = None;

        if let Ok((dfvk, ivk)) = parse_sapling_watch_key(ivk) {
            sapling_viewing_key = dfvk;
            sapling_ivk = Some(ivk);
        } else if let Ok((fvk, ivk)) = parse_ironwood_watch_key(ivk) {
            ironwood_viewing_key = fvk;
            ironwood_ivk = Some(ivk);
        }

        if sapling_ivk.is_none() && ironwood_ivk.is_none() {
            return Err(Error::InvalidKey(
                "Invalid viewing key format - must be Sapling xFVK or Ironwood extended viewing key"
                    .to_string(),
            ));
        }

        Ok(Self {
            wallet_type: WalletType::WatchOnly,
            spending_key: None,
            viewing_key: sapling_viewing_key,
            incoming_ivk: sapling_ivk,
            ironwood_viewing_key,
            ironwood_incoming_ivk: ironwood_ivk,
            notes: Vec::new(),
        })
    }

    /// Create from both Sapling and Ironwood viewing keys (watch-only wallet).
    pub fn from_viewing_keys(
        sapling_viewing_key_str: Option<&str>,
        ironwood_viewing_key_str: Option<&str>,
    ) -> Result<Self> {
        let mut sapling_viewing_key = None;
        let mut sapling = None;
        if let Some(value) = sapling_viewing_key_str {
            let (dfvk, ivk) = parse_sapling_watch_key(value)?;
            sapling_viewing_key = dfvk;
            sapling = Some(ivk);
        }

        let mut ironwood_viewing_key = None;
        let mut orchard = None;
        if let Some(value) = ironwood_viewing_key_str {
            let (fvk, ivk) = parse_ironwood_watch_key(value)?;
            ironwood_viewing_key = fvk;
            orchard = Some(ivk);
        }

        if sapling.is_none() && orchard.is_none() {
            return Err(Error::InvalidKey(
                "At least one viewing key (Sapling or Ironwood) must be provided".to_string(),
            ));
        }

        Ok(Self {
            wallet_type: WalletType::WatchOnly,
            spending_key: None,
            viewing_key: sapling_viewing_key,
            incoming_ivk: sapling,
            ironwood_viewing_key,
            ironwood_incoming_ivk: orchard,
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

    /// Get the wallet's Ironwood incoming viewing key (IVK), if this is a watch-only wallet.
    pub fn ironwood_incoming_ivk(&self) -> Option<&IronwoodIncomingViewingKey> {
        self.ironwood_incoming_ivk.as_ref()
    }

    /// Get the wallet's Ironwood viewing key, if available.
    pub fn ironwood_viewing_key(&self) -> Option<&IronwoodExtendedFullViewingKey> {
        self.ironwood_viewing_key.as_ref()
    }

    /// Check if wallet is watch-only
    pub fn is_watch_only(&self) -> bool {
        self.wallet_type == WalletType::WatchOnly
    }

    /// Export Sapling viewing key (xFVK)
    pub fn export_sapling_viewing_key(&self) -> String {
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

    /// Export Ironwood Extended Full Viewing Key as Bech32 (for watch-only wallets)
    ///
    /// Returns Bech32-encoded string with "pirate-extended-viewing-key" HRP.
    /// Uses the standard Ironwood viewing key Bech32 format.
    pub fn export_ironwood_viewing_key(&self) -> Option<String> {
        if let Some(fvk) = self.ironwood_viewing_key.as_ref() {
            fvk.to_bech32().ok()
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
        self.notes
            .iter()
            .filter(|n| !n.spent)
            .map(|n| n.value)
            .sum()
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

fn parse_ironwood_watch_key(
    value: &str,
) -> Result<(
    Option<IronwoodExtendedFullViewingKey>,
    IronwoodIncomingViewingKey,
)> {
    if let Ok(fvk) = IronwoodExtendedFullViewingKey::from_bech32_any(value) {
        let ivk_bytes = fvk.to_ivk_bytes();
        let ivk_ct = IronwoodIncomingViewingKey::from_bytes(&ivk_bytes);
        let ivk: Option<IronwoodIncomingViewingKey> = ivk_ct.into();
        let ivk = ivk.ok_or_else(|| Error::InvalidKey("Invalid Ironwood IVK bytes".to_string()))?;
        return Ok((Some(fvk), ivk));
    }

    Err(Error::InvalidKey(
        "Invalid Ironwood viewing key format (expected extended viewing key)".to_string(),
    ))
}
