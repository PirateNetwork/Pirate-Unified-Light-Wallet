//! Pirate Chain network definitions

use serde::{Deserialize, Serialize};

/// Network type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NetworkType {
    /// Mainnet
    Mainnet,
    /// Testnet
    Testnet,
    /// Regtest (local development)
    Regtest,
}

/// Network configuration
#[derive(Debug, Clone)]
pub struct Network {
    /// Network type
    pub network_type: NetworkType,
    /// Human-readable name
    pub name: &'static str,
    /// Coin type (BIP-44)
    pub coin_type: u32,
    /// RPC port
    pub rpc_port: u16,
    /// P2P port
    pub p2p_port: u16,
    /// Sapling activation height
    pub sapling_activation_height: u32,
    /// Orchard activation height (if activated)
    pub orchard_activation_height: Option<u32>,
    /// Default birthday height (wallet creation)
    pub default_birthday_height: u32,
}

impl Network {
    /// Get mainnet parameters
    pub const fn mainnet() -> Self {
        Self {
            network_type: NetworkType::Mainnet,
            name: "mainnet",
            coin_type: 141, // Pirate Chain BIP-44 coin type
            rpc_port: 45452,
            p2p_port: 45451,
            sapling_activation_height: 152_855,
            orchard_activation_height: None, // Orchard not activated on mainnet
            default_birthday_height: 3_750_000, // Recent checkpoint
        }
    }

    /// Get testnet parameters
    pub const fn testnet() -> Self {
        Self {
            network_type: NetworkType::Testnet,
            name: "testnet",
            coin_type: 1, // Testnet coin type
            rpc_port: 45462,
            p2p_port: 45461,
            sapling_activation_height: 1,
            orchard_activation_height: Some(61),
            default_birthday_height: 61,
        }
    }

    /// Get regtest parameters
    pub const fn regtest() -> Self {
        Self {
            network_type: NetworkType::Regtest,
            name: "regtest",
            coin_type: 1,
            rpc_port: 18344,
            p2p_port: 18445,
            sapling_activation_height: 100,
            orchard_activation_height: Some(200),
            default_birthday_height: 1,
        }
    }

    /// Get network by type
    pub const fn from_type(network_type: NetworkType) -> Self {
        match network_type {
            NetworkType::Mainnet => Self::mainnet(),
            NetworkType::Testnet => Self::testnet(),
            NetworkType::Regtest => Self::regtest(),
        }
    }

    /// Check if Sapling is activated at given height
    pub const fn is_sapling_active(&self, height: u32) -> bool {
        height >= self.sapling_activation_height
    }

    /// Check if Orchard is activated at given height
    pub const fn is_orchard_active(&self, height: u32) -> bool {
        if let Some(activation_height) = self.orchard_activation_height {
            height >= activation_height
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_params() {
        let net = Network::mainnet();
        assert_eq!(net.network_type, NetworkType::Mainnet);
        assert_eq!(net.coin_type, 141);
        assert_eq!(net.rpc_port, 45452);
        assert!(net.is_sapling_active(200_000));
        assert!(!net.is_orchard_active(4_000_000));
    }

    #[test]
    fn test_network_from_type() {
        let net = Network::from_type(NetworkType::Testnet);
        assert_eq!(net.network_type, NetworkType::Testnet);
    }
}

