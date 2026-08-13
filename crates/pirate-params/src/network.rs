//! Pirate Chain network definitions

use serde::{Deserialize, Serialize};

/// Sapling activation height for the deployed Pirate Chain testnet.
pub const TESTNET_SAPLING_ACTIVATION_HEIGHT: u32 = 61;
/// Ironwood activation height for the deployed Pirate Chain testnet.
pub const TESTNET_IRONWOOD_ACTIVATION_HEIGHT: u32 = 297;
/// Mainnet block-time threshold for deriving the Ironwood activation height.
///
/// This is 2026-10-03 19:00:00 UTC. The full node treats the threshold as
/// exclusive and activates Ironwood 60 blocks after the crossing block.
pub const MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP: u32 = 1_791_054_000;
/// Number of blocks between the mainnet timestamp transition and activation.
pub const MAINNET_IRONWOOD_ACTIVATION_DELAY: u32 = 60;

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
    /// Overwinter activation height
    pub overwinter_activation_height: u32,
    /// Ironwood activation height (if activated)
    pub ironwood_activation_height: Option<u32>,
    /// Block-time threshold used to derive Ironwood's activation height.
    pub ironwood_activation_timestamp: Option<u32>,
    /// Blocks between a timestamp transition and Ironwood activation.
    pub ironwood_activation_delay: u32,
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
            overwinter_activation_height: 152_855,
            sapling_activation_height: 152_855,
            ironwood_activation_height: None,
            ironwood_activation_timestamp: Some(MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP),
            ironwood_activation_delay: MAINNET_IRONWOOD_ACTIVATION_DELAY,
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
            overwinter_activation_height: TESTNET_SAPLING_ACTIVATION_HEIGHT,
            sapling_activation_height: TESTNET_SAPLING_ACTIVATION_HEIGHT,
            ironwood_activation_height: Some(TESTNET_IRONWOOD_ACTIVATION_HEIGHT),
            ironwood_activation_timestamp: None,
            ironwood_activation_delay: 0,
            default_birthday_height: TESTNET_SAPLING_ACTIVATION_HEIGHT,
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
            overwinter_activation_height: 50,
            sapling_activation_height: 100,
            ironwood_activation_height: Some(200),
            ironwood_activation_timestamp: None,
            ironwood_activation_delay: 0,
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

    /// Check if Ironwood is activated at given height
    pub const fn is_ironwood_active(&self, height: u32) -> bool {
        self.is_ironwood_active_with_resolved_height(height, None)
    }

    /// Check Ironwood activation using a chain-derived height when required.
    pub const fn is_ironwood_active_with_resolved_height(
        &self,
        height: u32,
        resolved_activation_height: Option<u32>,
    ) -> bool {
        let activation_height = match self.ironwood_activation_height {
            Some(height) => Some(height),
            None => resolved_activation_height,
        };
        if let Some(activation_height) = activation_height {
            height >= activation_height
        } else {
            false
        }
    }

    /// Derive Ironwood's activation height from adjacent canonical blocks.
    ///
    /// The comparison and delay intentionally mirror `komodo_activate_ironwood`.
    pub const fn derive_ironwood_activation_height(
        &self,
        previous_time: u32,
        current_height: u32,
        current_time: u32,
    ) -> Option<u32> {
        match self.ironwood_activation_timestamp {
            Some(timestamp) if previous_time <= timestamp && current_time > timestamp => {
                current_height.checked_add(self.ironwood_activation_delay)
            }
            _ => None,
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
        assert!(!net.is_ironwood_active(4_000_000));
        assert_eq!(
            net.ironwood_activation_timestamp,
            Some(MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP)
        );
        assert_eq!(
            net.ironwood_activation_delay,
            MAINNET_IRONWOOD_ACTIVATION_DELAY
        );
    }

    #[test]
    fn test_network_from_type() {
        let net = Network::from_type(NetworkType::Testnet);
        assert_eq!(net.network_type, NetworkType::Testnet);
        assert_eq!(
            net.overwinter_activation_height,
            TESTNET_SAPLING_ACTIVATION_HEIGHT
        );
        assert_eq!(
            net.sapling_activation_height,
            TESTNET_SAPLING_ACTIVATION_HEIGHT
        );
        assert!(!net.is_sapling_active(TESTNET_SAPLING_ACTIVATION_HEIGHT - 1));
        assert!(net.is_sapling_active(TESTNET_SAPLING_ACTIVATION_HEIGHT));
        assert_eq!(
            net.ironwood_activation_height,
            Some(TESTNET_IRONWOOD_ACTIVATION_HEIGHT)
        );
        assert!(!net.is_ironwood_active(TESTNET_IRONWOOD_ACTIVATION_HEIGHT - 1));
        assert!(net.is_ironwood_active(TESTNET_IRONWOOD_ACTIVATION_HEIGHT));
    }

    #[test]
    fn mainnet_derives_ironwood_height_from_strict_timestamp_crossing() {
        let net = Network::mainnet();
        let crossing_height = 4_200_000;
        let timestamp = MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP;

        assert_eq!(
            net.derive_ironwood_activation_height(timestamp, crossing_height, timestamp),
            None
        );
        assert_eq!(
            net.derive_ironwood_activation_height(timestamp, crossing_height, timestamp + 1),
            Some(crossing_height + MAINNET_IRONWOOD_ACTIVATION_DELAY)
        );
        assert_eq!(
            net.derive_ironwood_activation_height(timestamp + 1, crossing_height, timestamp + 2),
            None
        );
    }

    #[test]
    fn resolved_mainnet_height_controls_activation_boundary() {
        let net = Network::mainnet();
        let activation_height = 4_200_060;

        assert!(!net.is_ironwood_active_with_resolved_height(
            activation_height - 1,
            Some(activation_height)
        ));
        assert!(
            net.is_ironwood_active_with_resolved_height(activation_height, Some(activation_height))
        );
    }
}
