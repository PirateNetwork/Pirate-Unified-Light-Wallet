//! Consensus parameters for Pirate Chain

use crate::network::{Network, NetworkType};

/// Consensus parameters
#[derive(Debug, Clone)]
pub struct ConsensusParams {
    /// Network configuration
    pub network: Network,
    /// Target block time in seconds
    pub block_time_target: u32,
    /// Coinbase maturity (blocks)
    pub coinbase_maturity: u32,
    /// Maximum supply (arrrtoshis)
    pub max_money: u64,
    /// Founders reward percentage (0-100)
    pub founders_reward_percent: u8,
    /// Block subsidy reduction interval
    pub subsidy_halving_interval: u32,
}

impl ConsensusParams {
    /// Create consensus params for mainnet
    pub fn mainnet() -> Self {
        Self {
            network: Network::mainnet(),
            block_time_target: 60, // 60 seconds
            coinbase_maturity: 100,
            max_money: 200_000_000 * 100_000_000, // 200M ARRR
            founders_reward_percent: 0,           // No founders reward for Pirate
            subsidy_halving_interval: 388_885,
        }
    }

    /// Create consensus params for testnet
    pub fn testnet() -> Self {
        Self {
            network: Network::testnet(),
            block_time_target: 60,
            coinbase_maturity: 100,
            max_money: 200_000_000 * 100_000_000,
            founders_reward_percent: 0,
            subsidy_halving_interval: 388_885,
        }
    }

    /// Create consensus params for regtest
    pub fn regtest() -> Self {
        Self {
            network: Network::regtest(),
            block_time_target: 1, // 1 second for testing
            coinbase_maturity: 10,
            max_money: 200_000_000 * 100_000_000,
            founders_reward_percent: 0,
            subsidy_halving_interval: 150, // Fast halvings for testing
        }
    }

    /// Get consensus params by network type
    pub fn from_network(network_type: NetworkType) -> Self {
        match network_type {
            NetworkType::Mainnet => Self::mainnet(),
            NetworkType::Testnet => Self::testnet(),
            NetworkType::Regtest => Self::regtest(),
        }
    }

    /// Calculate block subsidy at given height
    pub fn block_subsidy(&self, height: u32) -> u64 {
        let halvings = height / self.subsidy_halving_interval;

        // Initial subsidy: 256 ARRR
        let mut subsidy = 256 * 100_000_000u64;

        // Halve for each halving period
        for _ in 0..halvings {
            subsidy /= 2;
        }

        subsidy
    }

    /// Check if amount is valid (within max supply)
    pub fn is_valid_amount(&self, amount: u64) -> bool {
        amount <= self.max_money
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_consensus() {
        let params = ConsensusParams::mainnet();
        assert_eq!(params.block_time_target, 60);
        assert_eq!(params.coinbase_maturity, 100);
    }

    #[test]
    fn test_block_subsidy() {
        let params = ConsensusParams::mainnet();

        // Initial subsidy
        assert_eq!(params.block_subsidy(0), 256 * 100_000_000);

        // After first halving
        assert_eq!(params.block_subsidy(388_885), 128 * 100_000_000);

        // After second halving
        assert_eq!(params.block_subsidy(777_770), 64 * 100_000_000);
    }

    #[test]
    fn test_valid_amount() {
        let params = ConsensusParams::mainnet();
        assert!(params.is_valid_amount(1_000_000));
        assert!(params.is_valid_amount(params.max_money));
        assert!(!params.is_valid_amount(params.max_money + 1));
    }
}
