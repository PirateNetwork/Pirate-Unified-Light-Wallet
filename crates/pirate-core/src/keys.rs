//! Key derivation and management
//!
//! Implements BIP-32/BIP-39/BIP-44 HD wallet key derivation for Pirate Chain
//! using Sapling key types from `zcash_primitives`.

use crate::{Error, Result};
use bip39::{Language, Mnemonic};
use blake2s_simd::Params as Blake2sParams;
use blake2b_simd::Params as Blake2bParams;
use pirate_params::{Network, NetworkType};
use zcash_client_backend::encoding::{
    decode_extended_full_viewing_key, decode_extended_spending_key, decode_payment_address,
    encode_extended_full_viewing_key, encode_payment_address,
};
use zcash_client_backend::keys::sapling as sapling_keys;
use zcash_primitives::{
    constants,
    sapling::{
        self,
        keys::FullViewingKey,
        keys::{NullifierDerivingKey, OutgoingViewingKey, SaplingIvk},
    },
    zip32::{
        AccountId,
        DiversifiableFullViewingKey,
        DiversifierIndex,
        Scope,
        ExtendedFullViewingKey as SaplingExtendedFullViewingKey,
        ExtendedSpendingKey as SaplingExtendedSpendingKey,
    },
};
use orchard::keys::{SpendingKey, FullViewingKey as OrchardFullViewingKey};
use bech32::{Bech32, Hrp};

/// PRF^Expand domain separator for ZIP-32 Orchard child key derivation
const PRF_EXPAND_PERSONALIZATION: &[u8; 16] = b"Zcash_ExpandSeed";
const ZIP32_ORCHARD_CHILD_DOMAIN: u8 = 0x81;

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

fn sapling_incoming_viewing_key_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "zivks",
        NetworkType::Testnet => "zivktestsapling",
        NetworkType::Regtest => "zivkregtestsapling",
    }
}

fn orchard_extfvk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "pirate-extended-viewing-key",
        NetworkType::Testnet => "pirate-extended-viewing-key-test",
        NetworkType::Regtest => "pirate-extended-viewing-key-regtest",
    }
}

/// Orchard extended spending key HRP for a given network.
pub fn orchard_extsk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "pirate-secret-extended-key",
        NetworkType::Testnet => "pirate-secret-extended-key-test",
        NetworkType::Regtest => "pirate-secret-extended-key-regtest",
    }
}

fn orchard_fvk_tag(fvk: &OrchardFullViewingKey) -> Result<[u8; 4]> {
    const ORCHARD_FVK_TAG_PERSONALIZATION: &[u8; 16] = b"ZcashOrchardFVFP";

    let mut fvk_bytes = Vec::new();
    fvk.write(&mut fvk_bytes)
        .map_err(|e| Error::InvalidKey(format!("Failed to serialize Orchard FVK: {e}")))?;

    let mut hasher = Blake2bParams::new()
        .hash_length(32)
        .personal(ORCHARD_FVK_TAG_PERSONALIZATION)
        .to_state();
    hasher.update(&fvk_bytes);
    let hash = hasher.finalize();

    let mut tag = [0u8; 4];
    tag.copy_from_slice(&hash.as_bytes()[..4]);
    Ok(tag)
}

/// Extended spending key (master secret)
#[derive(Clone)]
pub struct ExtendedSpendingKey {
    inner: SaplingExtendedSpendingKey,
}

impl ExtendedSpendingKey {
    /// Access the underlying Sapling extended spending key
    pub fn inner(&self) -> &SaplingExtendedSpendingKey {
        &self.inner
    }

    /// Generate from mnemonic seed phrase
    pub fn from_mnemonic(mnemonic: &str, passphrase: &str) -> Result<Self> {
        Self::from_mnemonic_with_account(mnemonic, passphrase, NetworkType::Mainnet, 0)
    }

    /// Generate from mnemonic seed phrase using ZIP-32 account derivation.
    ///
    /// Path: m/32'/coin_type'/account'
    pub fn from_mnemonic_with_account(
        mnemonic: &str,
        passphrase: &str,
        network: NetworkType,
        account: u32,
    ) -> Result<Self> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic)
            .map_err(|e| Error::InvalidSeed(e.to_string()))?;

        let seed_bytes = mnemonic.to_seed(passphrase);
        let network_params = Network::from_type(network);
        let extsk = sapling_keys::spending_key(
            &seed_bytes,
            network_params.coin_type,
            AccountId::from(account),
        );

        Ok(Self { inner: extsk })
    }

    /// Get seed bytes from mnemonic (for Orchard derivation)
    pub fn seed_bytes_from_mnemonic(mnemonic: &str, passphrase: &str) -> Result<Vec<u8>> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic)
            .map_err(|e| Error::InvalidSeed(e.to_string()))?;
        Ok(mnemonic.to_seed(passphrase).to_vec())
    }

    /// Generate new random mnemonic
    /// 
    /// # Arguments
    /// * `word_count` - Number of words in mnemonic (12, 18, or 24). Defaults to 24.
    /// 
    /// # Returns
    /// BIP39 mnemonic phrase with the specified number of words
    pub fn generate_mnemonic(word_count: Option<u32>) -> String {
        let word_count = word_count.unwrap_or(24);
        
        // BIP39 entropy requirements:
        // 12 words = 128 bits = 16 bytes
        // 18 words = 192 bits = 24 bytes
        // 24 words = 256 bits = 32 bytes
        let (entropy_size, _expected_words) = match word_count {
            12 => (16, 12),
            18 => (24, 18),
            24 => (32, 24),
            _ => {
                // Default to 24 words for invalid input
                (32, 24)
            }
        };
        
        let mut entropy = vec![0u8; entropy_size];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut entropy);
        
        let mnemonic = Mnemonic::from_entropy(&entropy)
            .expect("Entropy should always produce valid mnemonic");
        mnemonic.to_string()
    }

    /// Derive extended full viewing key
    pub fn to_extended_fvk(&self) -> ExtendedFullViewingKey {
        let dfvk = self.inner.to_diversifiable_full_viewing_key();
        ExtendedFullViewingKey { inner: dfvk }
    }

    /// Derive internal (change) full viewing key per ZIP-32.
    pub fn to_internal_fvk(&self) -> ExtendedFullViewingKey {
        let internal = self.inner.derive_internal();
        let dfvk = internal.to_diversifiable_full_viewing_key();
        ExtendedFullViewingKey { inner: dfvk }
    }

    /// Encode the Sapling extended full viewing key (xFVK) for the given network.
    #[allow(deprecated)]
    pub fn to_xfvk_bech32_for_network(&self, network: NetworkType) -> String {
        let xfvk = self.inner.to_extended_full_viewing_key();
        encode_extended_full_viewing_key(sapling_extfvk_hrp_for_network(network), &xfvk)
    }

    /// Serialize extended spending key to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes().to_vec()
    }

    /// Deserialize extended spending key from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 169 {
            return Err(Error::InvalidKey("Invalid spending key length".to_string()));
        }
        let mut key_bytes = [0u8; 169];
        key_bytes.copy_from_slice(bytes);
        let extsk = SaplingExtendedSpendingKey::from_bytes(&key_bytes)
            .map_err(|_| Error::InvalidKey("Invalid spending key bytes".to_string()))?;
        Ok(Self { inner: extsk })
    }

    /// Decode a Sapling extended spending key for any Pirate network.
    pub fn from_bech32_any(encoded: &str) -> Result<(Self, NetworkType)> {
        for network in [NetworkType::Mainnet, NetworkType::Testnet, NetworkType::Regtest] {
            if let Ok(extsk) = decode_extended_spending_key(
                sapling_extsk_hrp_for_network(network),
                encoded,
            ) {
                return Ok((Self { inner: extsk }, network));
            }
        }
        Err(Error::InvalidKey("Invalid Sapling extended spending key".to_string()))
    }
}

/// Extended full viewing key (for viewing transactions)
#[derive(Clone)]
pub struct ExtendedFullViewingKey {
    inner: DiversifiableFullViewingKey,
}

/// Incoming Viewing Key (IVK) - watch-only key for viewing incoming transactions
/// This is a 32-byte value derived from the FullViewingKey's ak and nk components.
/// IVK cannot be used to spend, only to view incoming transactions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingViewingKey {
    /// The 32-byte IVK value
    inner: [u8; 32],
}

impl IncomingViewingKey {
    /// Create IVK from raw 32-byte array
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { inner: bytes }
    }

    /// Get IVK as 32-byte array
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner
    }

    /// Import IVK from legacy string format (zxviews1... hex).
    pub fn from_string(ivk_str: &str) -> Result<Self> {
        if !ivk_str.starts_with("zxviews1") {
            return Err(Error::InvalidKey("IVK must start with zxviews1".to_string()));
        }

        let hex_part = &ivk_str[8..];
        let bytes = hex::decode(hex_part)
            .map_err(|_| Error::InvalidKey("Invalid hex encoding".to_string()))?;

        if bytes.len() != 32 {
            return Err(Error::InvalidKey("IVK must be 32 bytes".to_string()));
        }

        let mut ivk_bytes = [0u8; 32];
        ivk_bytes.copy_from_slice(&bytes);
        Ok(Self { inner: ivk_bytes })
    }

    /// Import IVK from bech32 (zivks... for any Pirate network).
    pub fn from_bech32_any(encoded: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(encoded)
            .map_err(|e| Error::InvalidKey(format!("IVK bech32 decode failed: {e}")))?;
        let hrp_str = hrp.as_str();
        if hrp_str != sapling_incoming_viewing_key_hrp_for_network(NetworkType::Mainnet)
            && hrp_str != sapling_incoming_viewing_key_hrp_for_network(NetworkType::Testnet)
            && hrp_str != sapling_incoming_viewing_key_hrp_for_network(NetworkType::Regtest)
        {
            return Err(Error::InvalidKey("Invalid Sapling IVK HRP".to_string()));
        }
        if data.len() != 32 {
            return Err(Error::InvalidKey("Sapling IVK must be 32 bytes".to_string()));
        }
        let mut ivk_bytes = [0u8; 32];
        ivk_bytes.copy_from_slice(&data[..32]);
        Ok(Self { inner: ivk_bytes })
    }

    /// Export IVK as legacy string format (zxviews1... hex).
    pub fn to_string(&self) -> String {
        format!("zxviews1{}", hex::encode(self.inner))
    }

    /// Get the underlying Sapling IVK bytes for use with trial decryption
    /// The IVK bytes are already computed correctly (with last 5 bits masked)
    /// and can be used directly with sapling_crypto trial decryption functions
    pub fn to_sapling_ivk_bytes(&self) -> [u8; 32] {
        self.inner
    }
}

impl ExtendedFullViewingKey {
    /// Derive internal (change) IVK bytes per ZIP-32.
    pub fn to_internal_ivk_bytes(&self) -> [u8; 32] {
        self.inner.to_ivk(Scope::Internal).to_repr()
    }

    /// Derive payment address at index
    pub fn derive_address(&self, index: u32) -> PaymentAddress {
        let diversifier_index = DiversifierIndex::from(index);
        let addr = self
            .inner
            .address(diversifier_index)
            .unwrap_or_else(|| {
                let (_diversifier_index, addr) = self.inner.default_address();
                addr
            });
        PaymentAddress { inner: addr }
    }

    /// Derive a payment address from a raw diversifier (11 bytes).
    pub fn address_from_diversifier(&self, diversifier: [u8; 11]) -> Option<PaymentAddress> {
        let sapling_div = sapling::Diversifier(diversifier);
        self.inner
            .diversified_address(sapling_div)
            .map(|addr| PaymentAddress { inner: addr })
    }

    /// Decode a Sapling extended full viewing key (xFVK) for the given network.
    pub fn from_xfvk_bech32_for_network(network: NetworkType, encoded: &str) -> Result<Self> {
        let extfvk = decode_extended_full_viewing_key(sapling_extfvk_hrp_for_network(network), encoded)
            .map_err(|_| Error::InvalidKey("Invalid Sapling extended viewing key".to_string()))?;
        let dfvk = DiversifiableFullViewingKey::from(extfvk);
        Ok(Self { inner: dfvk })
    }

    /// Decode a Sapling extended full viewing key (xFVK) for any Pirate network.
    pub fn from_xfvk_bech32_any(encoded: &str) -> Result<Self> {
        for network in [NetworkType::Mainnet, NetworkType::Testnet, NetworkType::Regtest] {
            if let Ok(extfvk) = decode_extended_full_viewing_key(
                sapling_extfvk_hrp_for_network(network),
                encoded,
            ) {
                let dfvk = DiversifiableFullViewingKey::from(extfvk);
                return Ok(Self { inner: dfvk });
            }
        }
        Err(Error::InvalidKey("Invalid Sapling extended viewing key".to_string()))
    }

    /// Export as incoming viewing key (IVK)
    /// IVK is derived from the FullViewingKey's ak and nk components.
    /// This is a 32-byte value that allows viewing incoming transactions but not spending.
    /// Uses the same IVK derivation as `librustzcash_crh_ivk(ak, nk)`.
    pub fn to_ivk(&self) -> IncomingViewingKey {
        // Get the underlying Sapling FullViewingKey from the DFVK (Pirate fork provides fvk()).
        let fvk: &FullViewingKey = self.inner.fvk();
        
        // Serialize FullViewingKey to bytes (96 bytes: 32 ak + 32 nk + 32 ovk)
        let mut fvk_bytes = Vec::new();
        fvk.write(&mut fvk_bytes)
            .expect("FullViewingKey should serialize to 96 bytes");
        
        // Extract ak (first 32 bytes) and nk (next 32 bytes) from serialized FVK
        // FullViewingKey serialization: ak (32 bytes) || nk (32 bytes) || ovk (32 bytes)
        if fvk_bytes.len() < 64 {
            panic!("FullViewingKey serialization should be at least 64 bytes");
        }
        let ak: [u8; 32] = fvk_bytes[0..32].try_into()
            .expect("ak should be 32 bytes");
        let nk: [u8; 32] = fvk_bytes[32..64].try_into()
            .expect("nk should be 32 bytes");
        
        // Compute IVK using CRH_IVK: IVK = CRH(ak || nk)
        // This matches the implementation in pirate/src/rust/src/sapling/spec.rs::crh_ivk
        let mut h = Blake2sParams::new()
            .hash_length(32)
            .personal(constants::CRH_IVK_PERSONALIZATION)
            .to_state();
        h.update(&ak);
        h.update(&nk);
        let mut ivk_bytes: [u8; 32] = h.finalize().as_ref().try_into()
            .expect("Blake2s output should be 32 bytes");
        
        // Drop the last five bits, so it can be interpreted as a scalar.
        ivk_bytes[31] &= 0b0000_0111;
        
        IncomingViewingKey { inner: ivk_bytes }
    }

    /// Get the Sapling nullifier deriving key (nk).
    pub fn nullifier_deriving_key(&self) -> NullifierDerivingKey {
        self.inner.fvk().vk.nk
    }

    /// Get the Sapling outgoing viewing key (ovk).
    pub fn outgoing_viewing_key(&self) -> OutgoingViewingKey {
        self.inner.fvk().ovk
    }

    /// Get the Sapling incoming viewing key (SaplingIvk).
    pub fn sapling_ivk(&self) -> SaplingIvk {
        self.inner.fvk().vk.ivk()
    }

    /// Export IVK as legacy string format (zxviews1... hex).
    pub fn to_ivk_string(&self) -> String {
        let ivk = self.to_ivk();
        format!("zxviews1{}", hex::encode(ivk.inner))
    }

    /// Serialize DFVK bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes().to_vec()
    }

    /// Deserialize DFVK bytes (128) or xFVK bytes (169) into a DFVK.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes.len() {
            128 => {
                let mut key_bytes = [0u8; 128];
                key_bytes.copy_from_slice(bytes);
                DiversifiableFullViewingKey::from_bytes(&key_bytes).map(|inner| Self { inner })
            }
            169 => {
                let extfvk = SaplingExtendedFullViewingKey::read(&mut &bytes[..]).ok()?;
                let dfvk = DiversifiableFullViewingKey::from(extfvk);
                Some(Self { inner: dfvk })
            }
            _ => None,
        }
    }
}

/// Sapling payment address
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentAddress {
    /// Underlying Sapling payment address
    pub inner: sapling::PaymentAddress,
}

impl PaymentAddress {
    fn sapling_hrp_for_network(network: NetworkType) -> &'static str {
        // HRPs from `pirate/src/chainparams.cpp`:
        // - mainnet: "zs"
        // - testnet: "ztestsapling"
        // - regtest: "zregtestsapling"
        match network {
            NetworkType::Mainnet => "zs",
            NetworkType::Testnet => "ztestsapling",
            NetworkType::Regtest => "zregtestsapling",
        }
    }

    /// Encode as Bech32 address for the given network.
    pub fn encode_for_network(&self, network: NetworkType) -> String {
        encode_payment_address(Self::sapling_hrp_for_network(network), &self.inner)
    }

    /// Encode as Bech32 address (defaults to mainnet `zs1...`).
    pub fn encode(&self) -> String {
        self.encode_for_network(NetworkType::Mainnet)
    }

    /// Decode from Bech32 address for the given network.
    pub fn decode_for_network(network: NetworkType, addr: &str) -> Result<Self> {
        let decoded = decode_payment_address(Self::sapling_hrp_for_network(network), addr)
            .map_err(|_| Error::InvalidAddress("Invalid Sapling address".to_string()))?;

        Ok(PaymentAddress { inner: decoded })
    }

    /// Decode from Bech32 Sapling address, accepting any Pirate network HRP.
    pub fn decode_any_network(addr: &str) -> Result<Self> {
        for hrp in ["zs", "ztestsapling", "zregtestsapling"] {
            if let Ok(decoded) = decode_payment_address(hrp, addr) {
                return Ok(PaymentAddress { inner: decoded });
            }
        }
        Err(Error::InvalidAddress("Invalid Sapling address".to_string()))
    }
}

/// Orchard payment address (Pirate encoding: bech32 HRP `pirate`, `pirate-test`, `pirate-regtest`)
///
/// This is **not** a Zcash Unified Address; Pirate encodes the raw Orchard address bytes directly
/// under chain-specific HRPs, as seen in `pirate/src/key_io.cpp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchardPaymentAddress {
    /// Underlying Orchard address (raw 43 bytes)
    pub inner: orchard::Address,
}

impl OrchardPaymentAddress {
    fn orchard_hrp_for_network(network: NetworkType) -> &'static str {
        // HRPs from `pirate/src/chainparams.cpp`:
        // - mainnet: "pirate"
        // - testnet: "pirate-test"
        // - regtest: "pirate-regtest"
        match network {
            NetworkType::Mainnet => "pirate",
            NetworkType::Testnet => "pirate-test",
            NetworkType::Regtest => "pirate-regtest",
        }
    }

    /// Encode as bech32 under Pirate's Orchard HRP (e.g. `pirate1...`).
    pub fn encode_for_network(&self, network: NetworkType) -> Result<String> {
        let hrp = Hrp::parse(Self::orchard_hrp_for_network(network))
            .map_err(|e| Error::InvalidAddress(format!("Invalid Orchard HRP: {e}")))?;
        bech32::encode::<Bech32>(hrp, &self.inner.to_raw_address_bytes())
            .map_err(|e| Error::InvalidAddress(format!("Orchard bech32 encode failed: {e}")))
    }

    /// Decode a Pirate Orchard payment address, accepting any Pirate network HRP.
    pub fn decode_any_network(addr: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(addr)
            .map_err(|e| Error::InvalidAddress(format!("Orchard bech32 decode failed: {e}")))?;

        let hrp_str = hrp.as_str();
        if hrp_str != "pirate" && hrp_str != "pirate-test" && hrp_str != "pirate-regtest" {
            return Err(Error::InvalidAddress("Invalid Orchard address HRP".to_string()));
        }

        let raw: [u8; 43] = data
            .try_into()
            .map_err(|_| Error::InvalidAddress("Invalid Orchard address length".to_string()))?;

        let inner_ct = orchard::Address::from_raw_address_bytes(&raw);
        let inner = match bool::from(inner_ct.is_some()) {
            true => inner_ct.unwrap(),
            false => {
                return Err(Error::InvalidAddress(
                    "Invalid Orchard address bytes".to_string(),
                ))
            }
        };

        Ok(Self { inner })
    }
}

/// Orchard extended spending key
/// 
/// Derived from master seed using ZIP-32 Orchard key derivation.
/// Path: m/32'/coin_type'/account'
#[derive(Clone, Debug)]
pub struct OrchardExtendedSpendingKey {
    /// Underlying Orchard spending key
    pub inner: orchard::keys::SpendingKey,
    /// Chain code for derivation
    pub chain_code: [u8; 32],
    /// Depth in derivation tree
    pub depth: u8,
    /// Parent FVK tag (first 4 bytes of Orchard FVK fingerprint)
    pub parent_fvk_tag: [u8; 4],
    /// Child index
    pub child_index: u32,
}

impl OrchardExtendedSpendingKey {
    /// Derive master Orchard spending key from seed bytes
    /// 
    /// Uses ZIP-32 Orchard master key generation:
    /// I := BLAKE2b-512("ZcashIP32Orchard", seed)
    /// sk_m := I[0..32]
    /// c_m := I[32..64]
    pub fn master(seed: &[u8]) -> Result<Self> {
        if seed.len() < 32 || seed.len() > 252 {
            return Err(Error::InvalidSeed("Seed must be 32-252 bytes".to_string()));
        }

        const ZIP32_ORCHARD_PERSONALIZATION: &[u8; 16] = b"ZcashIP32Orchard";
        
        // I := BLAKE2b-512("ZcashIP32Orchard", seed)
        let i: [u8; 64] = {
            let mut hasher = Blake2bParams::new()
                .hash_length(64)
                .personal(ZIP32_ORCHARD_PERSONALIZATION)
                .to_state();
            hasher.update(seed);
            hasher.finalize().as_bytes().try_into()
                .map_err(|_| Error::InvalidKey("Failed to derive Orchard master key".to_string()))?
        };

        // I_L is used as the master spending key sk_m
        let sk_m = SpendingKey::from_bytes(i[..32].try_into().unwrap());
        if sk_m.is_none().into() {
            return Err(Error::InvalidKey("Invalid Orchard spending key from seed".to_string()));
        }
        let sk_m = sk_m.unwrap();

        // I_R is used as the master chain code c_m
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&i[32..64]);

        Ok(Self {
            inner: sk_m,
            chain_code,
            depth: 0,
            parent_fvk_tag: [0u8; 4],
            child_index: 0,
        })
    }

    /// Derive a child key at the given hardened index
    /// 
    /// Uses ZIP-32 Orchard child key derivation:
    /// I := PRF^Expand(c_par, [0x81] || sk_par || I2LEOSP(i))
    /// sk_i := I[0..32]
    /// c_i := I[32..64]
    /// 
    /// The index must be < 2^31 (non-hardened). It will be automatically hardened
    /// by adding 2^31 (ZIP32 hardened index).
    pub fn derive_child(&self, index: u32) -> Result<Self> {
        if index >= (1u32 << 31) {
            return Err(Error::InvalidKey(format!("Child index {} must be < 2^31", index)));
        }
        
        // Automatically harden the index (add 2^31)
        let hardened_index = index | (1u32 << 31);

        // I := PRF^Expand(c_par, [0x81] || sk_par || I2LEOSP(i))
        // PRF^Expand(sk, dst, t) := BLAKE2b-512("Zcash_ExpandSeed", sk || dst || t)
        let i: [u8; 64] = {
            let mut hasher = Blake2bParams::new()
                .hash_length(64)
                .personal(PRF_EXPAND_PERSONALIZATION)
                .to_state();
            hasher.update(&self.chain_code);
            hasher.update(&[ZIP32_ORCHARD_CHILD_DOMAIN]);
            hasher.update(self.inner.to_bytes());
            hasher.update(&hardened_index.to_le_bytes());
            hasher.finalize().as_bytes().try_into()
                .map_err(|_| Error::InvalidKey("Failed to derive Orchard child key".to_string()))?
        };

        // I_L is used as the child spending key sk_i
        let sk_i = SpendingKey::from_bytes(i[..32].try_into().unwrap());
        if sk_i.is_none().into() {
            return Err(Error::InvalidKey("Invalid Orchard child spending key".to_string()));
        }
        let sk_i = sk_i.unwrap();

        // I_R is used as the child chain code c_i
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&i[32..64]);

        let fvk: OrchardFullViewingKey = (&sk_i).into();
        let parent_fvk_tag = orchard_fvk_tag(&fvk)?;

        Ok(Self {
            inner: sk_i,
            chain_code,
            depth: self.depth + 1,
            parent_fvk_tag,
            child_index: hardened_index,
        })
    }

    /// Derive account-level key from master using path m/32'/coin_type'/account'
    /// 
    /// Derivation path:
    /// - m/32' (ZIP32 purpose, hardened)
    /// - m/32'/coin_type' (BIP-44 coin type, hardened)
    /// - m/32'/coin_type'/account' (account index, hardened)
    pub fn derive_account(&self, coin_type: u32, account: u32) -> Result<Self> {
        const ZIP32_PURPOSE: u32 = 32;
        
        // derive_child expects non-hardened indices (< 2^31) and handles hardening internally
        // Derive m/32' (hardened)
        let purpose_key = self.derive_child(ZIP32_PURPOSE)?;
        
        // Derive m/32'/coin_type' (hardened)
        let coin_type_key = purpose_key.derive_child(coin_type)?;
        
        // Derive m/32'/coin_type'/account' (hardened)
        coin_type_key.derive_child(account)
    }

    /// Derive Orchard extended full viewing key
    pub fn to_extended_fvk(&self) -> OrchardExtendedFullViewingKey {
        let fvk: OrchardFullViewingKey = (&self.inner).into();
        OrchardExtendedFullViewingKey {
            inner: fvk,
            chain_code: self.chain_code,
            depth: self.depth,
            parent_fvk_tag: self.parent_fvk_tag,
            child_index: self.child_index,
        }
    }

    /// Serialize to bytes (73 bytes: depth + parent_fvk_tag + child_index + chain_code + sk)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(73);
        bytes.push(self.depth);
        bytes.extend_from_slice(&self.parent_fvk_tag);
        bytes.extend_from_slice(&self.child_index.to_le_bytes());
        bytes.extend_from_slice(&self.chain_code);
        bytes.extend_from_slice(self.inner.to_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 73 {
            return Err(Error::InvalidKey("Invalid Orchard key length".to_string()));
        }

        let depth = bytes[0];
        let parent_fvk_tag = [bytes[1], bytes[2], bytes[3], bytes[4]];
        let child_index = u32::from_le_bytes([
            bytes[5], bytes[6], bytes[7], bytes[8],
        ]);
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&bytes[9..41]);
        let sk_bytes: [u8; 32] = bytes[41..73]
            .try_into()
            .map_err(|_| Error::InvalidKey("Invalid Orchard key bytes".to_string()))?;

        let mut needs_fallback = false;
        if depth == 0 && child_index != 0 {
            needs_fallback = true;
        }
        if depth > 0 && child_index < (1u32 << 31) {
            needs_fallback = true;
        }
        if depth > 0 && parent_fvk_tag == [0u8; 4] {
            needs_fallback = true;
        }

        let sk_opt = SpendingKey::from_bytes(sk_bytes);
        if sk_opt.is_some().into() && !needs_fallback {
            let sk = sk_opt.unwrap();
            return Ok(Self {
                inner: sk,
                chain_code,
                depth,
                parent_fvk_tag,
                child_index,
            });
        }

        // Fallback to legacy serialization (depth | child_index | chain_code | sk)
        let depth = bytes[0];
        let child_index = u32::from_le_bytes([
            bytes[1], bytes[2], bytes[3], bytes[4],
        ]);
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&bytes[5..37]);
        let sk_bytes: [u8; 32] = bytes[37..69]
            .try_into()
            .map_err(|_| Error::InvalidKey("Invalid Orchard key bytes".to_string()))?;

        let sk = SpendingKey::from_bytes(sk_bytes);
        if sk.is_none().into() {
            return Err(Error::InvalidKey("Invalid Orchard spending key bytes".to_string()));
        }
        let sk = sk.unwrap();
        let parent_fvk_tag = if depth == 0 {
            [0u8; 4]
        } else {
            let fvk: OrchardFullViewingKey = (&sk).into();
            orchard_fvk_tag(&fvk)?
        };

        Ok(Self {
            inner: sk,
            chain_code,
            depth,
            parent_fvk_tag,
            child_index,
        })
    }

    /// Decode an Orchard extended spending key for any Pirate network.
    pub fn from_bech32_any(encoded: &str) -> Result<(Self, NetworkType)> {
        let (hrp, bytes) = bech32::decode(encoded)
            .map_err(|e| Error::InvalidKey(format!("Bech32 decoding failed: {e}")))?;
        let network = match hrp.as_str() {
            "pirate-secret-extended-key" => NetworkType::Mainnet,
            "pirate-secret-extended-key-test" => NetworkType::Testnet,
            "pirate-secret-extended-key-regtest" => NetworkType::Regtest,
            _ => return Err(Error::InvalidKey("Invalid Orchard extended spending key HRP".to_string())),
        };
        let key = Self::from_bytes(&bytes)?;
        Ok((key, network))
    }
}

/// Orchard extended full viewing key
#[derive(Clone, Debug)]
pub struct OrchardExtendedFullViewingKey {
    /// Underlying Orchard full viewing key
    pub inner: OrchardFullViewingKey,
    /// Chain code
    pub chain_code: [u8; 32],
    /// Depth
    pub depth: u8,
    /// Parent FVK tag (first 4 bytes of Orchard FVK fingerprint)
    pub parent_fvk_tag: [u8; 4],
    /// Child index
    pub child_index: u32,
}

impl OrchardExtendedFullViewingKey {
    /// Get default address for this viewing key
    pub fn default_address(&self) -> OrchardPaymentAddress {
        OrchardPaymentAddress {
            inner: self.inner.address_at(0u32, orchard::keys::Scope::External),
        }
    }

    /// Derive address at index
    pub fn address_at(&self, index: u32) -> OrchardPaymentAddress {
        OrchardPaymentAddress {
            inner: self.inner.address_at(index, orchard::keys::Scope::External),
        }
    }

    /// Derive internal (change) address at index.
    pub fn address_at_internal(&self, index: u32) -> OrchardPaymentAddress {
        OrchardPaymentAddress {
            inner: self.inner.address_at(index, orchard::keys::Scope::Internal),
        }
    }

    /// Derive an address from a raw diversifier (11 bytes).
    pub fn address_from_diversifier(&self, diversifier: [u8; 11]) -> Option<OrchardPaymentAddress> {
        let div = orchard::keys::Diversifier::from_bytes(diversifier);
        Some(OrchardPaymentAddress {
            inner: self.inner.address(div, orchard::keys::Scope::External),
        })
    }

    /// Derive Orchard incoming viewing key (IVK) from full viewing key
    /// 
    /// The IVK allows viewing incoming transactions but not spending.
    /// Returns 64 bytes (Orchard IVK format).
    pub fn to_ivk_bytes(&self) -> [u8; 64] {
        let ivk = self.inner.to_ivk(orchard::keys::Scope::External);
        ivk.to_bytes()
    }

    /// Derive internal (change) Orchard IVK bytes.
    pub fn to_internal_ivk_bytes(&self) -> [u8; 64] {
        let ivk = self.inner.to_ivk(orchard::keys::Scope::Internal);
        ivk.to_bytes()
    }

    /// Get the Orchard outgoing viewing key (ovk).
    pub fn to_ovk(&self) -> orchard::keys::OutgoingViewingKey {
        self.inner.to_ovk(orchard::keys::Scope::External)
    }

    /// Encode Orchard extended full viewing key as Bech32 for the given network.
    ///
    /// Format: depth (1 byte) || parentFVKTag (4 bytes) || childIndex (4 bytes) || chaincode (32 bytes) || fvk (96 bytes)
    pub fn to_bech32_for_network(&self, network: NetworkType) -> Result<String> {
        use std::io::Write;

        let mut data = Vec::new();
        data.write_all(&[self.depth])?;
        data.write_all(&self.parent_fvk_tag)?;
        data.write_all(&self.child_index.to_le_bytes())?;
        data.write_all(&self.chain_code)?;

        // Serialize Orchard Full Viewing Key (ak || nk || rivk)
        self.inner.write(&mut data)
            .map_err(|e| Error::InvalidKey(format!("Failed to serialize Orchard FullViewingKey: {}", e)))?;

        let hrp = Hrp::parse(orchard_extfvk_hrp_for_network(network))
            .map_err(|e| Error::InvalidKey(format!("Invalid HRP: {}", e)))?;
        bech32::encode::<Bech32>(hrp, &data)
            .map_err(|e| Error::InvalidKey(format!("Bech32 encoding failed: {}", e)))
    }

    /// Encode Orchard extended full viewing key as Bech32 (defaults to mainnet HRP).
    pub fn to_bech32(&self) -> Result<String> {
        self.to_bech32_for_network(NetworkType::Mainnet)
    }

    /// Decode Orchard extended full viewing key for the given network HRP.
    pub fn from_bech32_for_network(network: NetworkType, encoded: &str) -> Result<Self> {
        let (hrp, bytes) = bech32::decode(encoded)
            .map_err(|e| Error::InvalidKey(format!("Bech32 decoding failed: {}", e)))?;

        if hrp.as_str() != orchard_extfvk_hrp_for_network(network) {
            return Err(Error::InvalidKey(format!(
                "Invalid HRP: expected '{}', got '{}'",
                orchard_extfvk_hrp_for_network(network),
                hrp.as_str()
            )));
        }

        Self::from_bech32_payload(bytes)
    }

    /// Decode Orchard extended full viewing key for any Pirate network HRP.
    pub fn from_bech32_any(encoded: &str) -> Result<Self> {
        let (hrp, bytes) = bech32::decode(encoded)
            .map_err(|e| Error::InvalidKey(format!("Bech32 decoding failed: {}", e)))?;
        let hrp_str = hrp.as_str();
        if hrp_str != "pirate-extended-viewing-key"
            && hrp_str != "pirate-extended-viewing-key-test"
            && hrp_str != "pirate-extended-viewing-key-regtest"
        {
            return Err(Error::InvalidKey("Invalid Orchard extended viewing key HRP".to_string()));
        }

        Self::from_bech32_payload(bytes)
    }

    fn from_bech32_payload(bytes: Vec<u8>) -> Result<Self> {
        // Deserialize: depth (1) || parentFVKTag (4) || childIndex (4) || chaincode (32) || fvk (96)
        if bytes.len() < 137 {
            return Err(Error::InvalidKey("Invalid Extended FVK length".to_string()));
        }

        // Deserialize: depth (1) || parentFVKTag (4) || childIndex (4) || chaincode (32) || fvk (96)
        let depth = bytes[0];
        let parent_fvk_tag = [bytes[1], bytes[2], bytes[3], bytes[4]];
        let child_index = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
        
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&bytes[9..41]);

        // Deserialize Orchard Full Viewing Key (96 bytes: ak || nk || rivk)
        // Orchard FullViewingKey implements Read trait (like Sapling FullViewingKey)
        let fvk_bytes = &bytes[41..137];
        let fvk = OrchardFullViewingKey::read(&mut fvk_bytes.as_ref())
            .map_err(|e| Error::InvalidKey(format!("Failed to deserialize Orchard FullViewingKey: {}", e)))?;

        Ok(Self {
            inner: fvk,
            chain_code,
            depth,
            parent_fvk_tag,
            child_index,
        })
    }

    /// Serialize to bytes (for storage)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(self.depth);
        data.extend_from_slice(&self.parent_fvk_tag);
        data.extend_from_slice(&self.child_index.to_le_bytes());
        data.extend_from_slice(&self.chain_code);
        // Serialize Orchard FullViewingKey using write() method
        self.inner.write(&mut data)
            .expect("Failed to serialize Orchard FullViewingKey");
        data
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 137 {
            return Err(Error::InvalidKey("Invalid Extended FVK length".to_string()));
        }

        let depth = bytes[0];
        let parent_fvk_tag = [bytes[1], bytes[2], bytes[3], bytes[4]];
        let child_index = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
        
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&bytes[9..41]);

        // Deserialize Orchard FullViewingKey using read() method
        let fvk_bytes = &bytes[41..137];
        let fvk = OrchardFullViewingKey::read(&mut fvk_bytes.as_ref())
            .map_err(|e| Error::InvalidKey(format!("Failed to deserialize Orchard FullViewingKey: {}", e)))?;

        Ok(Self {
            inner: fvk,
            chain_code,
            depth,
            parent_fvk_tag,
            child_index,
        })
    }
}

// Test helpers
#[cfg(any(test, feature = "test-helpers"))]
impl PaymentAddress {
    /// Create a test address (for testing only)
    pub fn test_address() -> Self {
        let extsk = SaplingExtendedSpendingKey::master(&[0u8; 32]);
        let dfvk = extsk.to_diversifiable_full_viewing_key();
        let (_, addr) = dfvk.default_address();
        Self { inner: addr }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mnemonic() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic();
        let words: Vec<&str> = mnemonic.split_whitespace().collect();
        assert_eq!(words.len(), 24);
    }

    #[test]
    fn test_from_mnemonic() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic();
        let result = ExtendedSpendingKey::from_mnemonic(&mnemonic, "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_derive_address() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic();
        let sk = ExtendedSpendingKey::from_mnemonic(&mnemonic, "").unwrap();
        let fvk = sk.to_extended_fvk();
        
        let addr1 = fvk.derive_address(0);
        let addr2 = fvk.derive_address(1);
        
        assert_ne!(addr1, addr2);
    }

    #[test]
    fn test_address_encoding() {
        let addr = PaymentAddress::test_address();
        let encoded = addr.encode();
        assert!(encoded.starts_with("zs1"));
    }
}
