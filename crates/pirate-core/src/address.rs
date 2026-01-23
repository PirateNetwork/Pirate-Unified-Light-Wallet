//! Address derivation and management with diversifier rotation
//!
//! Provides fresh, unlinkable Sapling addresses by default through diversifier rotation.

use crate::diversifier::DiversifierIndex;
use crate::keys::ExtendedFullViewingKey;
use crate::{Error, Result};
use std::collections::HashMap;

/// Sapling payment address
#[derive(Debug, Clone)]
pub struct SaplingAddress {
    /// The address string (zs...)
    pub address: String,
    /// Diversifier index used
    pub diversifier_index: DiversifierIndex,
    /// Optional label
    pub label: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl SaplingAddress {
    /// Create new address
    pub fn new(address: String, diversifier_index: DiversifierIndex) -> Self {
        Self {
            address,
            diversifier_index,
            label: None,
            created_at: chrono::Utc::now(),
        }
    }

    /// Set label
    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }

    /// Validate address format
    pub fn validate(&self) -> Result<()> {
        if !self.address.starts_with("zs") {
            return Err(Error::InvalidAddress(
                "Sapling address must start with 'zs'".to_string(),
            ));
        }

        // Basic length check (Sapling addresses are typically 78 characters)
        if self.address.len() < 70 || self.address.len() > 85 {
            return Err(Error::InvalidAddress("Invalid address length".to_string()));
        }

        Ok(())
    }
}

/// Address manager for generating and tracking unlinkable addresses
pub struct AddressManager {
    /// Current diversifier index
    current_index: DiversifierIndex,
    /// Viewing key used to derive Sapling payment addresses
    viewing_key: Option<ExtendedFullViewingKey>,
    /// Generated addresses
    addresses: HashMap<DiversifierIndex, SaplingAddress>,
    /// Address book (external addresses with labels)
    address_book: HashMap<String, String>,
}

impl AddressManager {
    /// Create new address manager
    pub fn new() -> Self {
        Self {
            current_index: DiversifierIndex::default(),
            viewing_key: None,
            addresses: HashMap::new(),
            address_book: HashMap::new(),
        }
    }

    /// Create an address manager that can derive addresses from the given viewing key.
    pub fn new_with_viewing_key(viewing_key: ExtendedFullViewingKey) -> Self {
        Self {
            current_index: DiversifierIndex::default(),
            viewing_key: Some(viewing_key),
            addresses: HashMap::new(),
            address_book: HashMap::new(),
        }
    }

    /// Attach/replace the viewing key used for address derivation.
    pub fn set_viewing_key(&mut self, viewing_key: ExtendedFullViewingKey) {
        self.viewing_key = Some(viewing_key);
    }

    /// Generate a fresh, unlinkable address
    pub fn generate_fresh_address(&mut self) -> Result<SaplingAddress> {
        let vk = self.viewing_key.as_ref().ok_or_else(|| {
            Error::InvalidKey(
                "AddressManager cannot derive addresses without an attached viewing key"
                    .to_string(),
            )
        })?;

        let address = vk.derive_address(self.current_index.as_u32()).encode();

        let sapling_address = SaplingAddress::new(address, self.current_index);

        self.addresses
            .insert(self.current_index, sapling_address.clone());
        self.current_index = self.current_index.next();

        tracing::info!(
            "Generated fresh address with diversifier {}",
            sapling_address.diversifier_index.as_u32()
        );

        Ok(sapling_address)
    }

    /// Get current address (reuse if available)
    pub fn get_current_address(&mut self) -> Result<SaplingAddress> {
        // Check if we have an address at current index
        if let Some(addr) = self.addresses.get(&self.current_index) {
            return Ok(addr.clone());
        }

        // Generate new one
        self.generate_fresh_address()
    }

    /// Get next unlinkable address (always generates new)
    pub fn get_next_address(&mut self) -> Result<SaplingAddress> {
        self.generate_fresh_address()
    }

    /// Get address by diversifier index
    pub fn get_address_by_index(&self, index: DiversifierIndex) -> Option<&SaplingAddress> {
        self.addresses.get(&index)
    }

    /// List all generated addresses
    pub fn list_addresses(&self) -> Vec<&SaplingAddress> {
        let mut addrs: Vec<_> = self.addresses.values().collect();
        addrs.sort_by_key(|a| a.diversifier_index.as_u32());
        addrs
    }

    /// Add label to address
    pub fn label_address(&mut self, address: &str, label: String) -> Result<()> {
        // Check if it's one of our addresses
        for addr in self.addresses.values_mut() {
            if addr.address == address {
                addr.label = Some(label.clone());
                tracing::info!("Labeled own address: {}", label);
                return Ok(());
            }
        }

        // Otherwise, add to address book (external address)
        self.address_book.insert(address.to_string(), label.clone());
        tracing::info!("Added address to address book: {}", label);

        Ok(())
    }

    /// Get label for address
    pub fn get_label(&self, address: &str) -> Option<&String> {
        // Check own addresses first
        for addr in self.addresses.values() {
            if addr.address == address {
                return addr.label.as_ref();
            }
        }

        // Check address book
        self.address_book.get(address)
    }

    /// Get address book entries
    pub fn get_address_book(&self) -> &HashMap<String, String> {
        &self.address_book
    }

    /// Reset to specific diversifier index
    pub fn reset_to_index(&mut self, index: DiversifierIndex) {
        self.current_index = index;
        tracing::info!("Reset diversifier index to {}", index.as_u32());
    }

    /// Get current diversifier index
    pub fn current_diversifier_index(&self) -> DiversifierIndex {
        self.current_index
    }
}

impl Default for AddressManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse Sapling address and validate
pub fn parse_sapling_address(address: &str) -> Result<SaplingAddress> {
    // Perform real decoding + HRP validation using Pirate's Sapling HRPs.
    let _ = crate::keys::PaymentAddress::decode_any_network(address)?;

    let addr = SaplingAddress::new(address.to_string(), DiversifierIndex::default());
    addr.validate()?;
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::ExtendedSpendingKey;

    #[test]
    fn test_diversifier_index() {
        let idx = DiversifierIndex::new(0);
        assert_eq!(idx.as_u32(), 0);

        let next = idx.next();
        assert_eq!(next.as_u32(), 1);
    }

    #[test]
    fn test_address_manager_fresh_addresses() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic(None);
        let sk = ExtendedSpendingKey::from_mnemonic(&mnemonic, "").unwrap();
        let fvk = sk.to_extended_fvk();

        let mut manager = AddressManager::new_with_viewing_key(fvk);

        let addr1 = manager.generate_fresh_address().unwrap();
        let addr2 = manager.generate_fresh_address().unwrap();

        // Should have different diversifier indices
        assert_ne!(
            addr1.diversifier_index.as_u32(),
            addr2.diversifier_index.as_u32()
        );

        // Should be unlinkable (different addresses)
        assert_ne!(addr1.address, addr2.address);
    }

    #[test]
    fn test_address_labeling() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic(None);
        let sk = ExtendedSpendingKey::from_mnemonic(&mnemonic, "").unwrap();
        let fvk = sk.to_extended_fvk();
        let mut manager = AddressManager::new_with_viewing_key(fvk);

        let addr = manager.generate_fresh_address().unwrap();
        manager
            .label_address(&addr.address, "My Savings".to_string())
            .unwrap();

        let label = manager.get_label(&addr.address);
        assert_eq!(label, Some(&"My Savings".to_string()));
    }

    #[test]
    fn test_address_book() {
        let mut manager = AddressManager::new();

        manager
            .label_address("zs1externaladdress123", "Alice".to_string())
            .unwrap();

        let label = manager.get_label("zs1externaladdress123");
        assert_eq!(label, Some(&"Alice".to_string()));

        let book = manager.get_address_book();
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn test_list_addresses() {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic(None);
        let sk = ExtendedSpendingKey::from_mnemonic(&mnemonic, "").unwrap();
        let fvk = sk.to_extended_fvk();
        let mut manager = AddressManager::new_with_viewing_key(fvk);

        manager.generate_fresh_address().unwrap();
        manager.generate_fresh_address().unwrap();
        manager.generate_fresh_address().unwrap();

        let addrs = manager.list_addresses();
        assert_eq!(addrs.len(), 3);

        // Should be sorted by diversifier index
        for i in 1..addrs.len() {
            assert!(addrs[i - 1].diversifier_index.as_u32() < addrs[i].diversifier_index.as_u32());
        }
    }

    #[test]
    fn test_address_validation() {
        let valid = SaplingAddress::new("zs1test".to_string(), DiversifierIndex::new(0));
        assert!(valid.validate().is_err()); // Too short

        let invalid_prefix = SaplingAddress::new("zt1test".to_string(), DiversifierIndex::new(0));
        assert!(invalid_prefix.validate().is_err());
    }
}
