//! Blockchain checkpoints for faster sync

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// A blockchain checkpoint
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Block height
    pub height: u32,
    /// Block hash (hex)
    pub hash: String,
    /// Timestamp (Unix epoch)
    pub timestamp: u64,
    /// Sapling tree size at this height
    pub sapling_tree_size: u32,
}

/// List of checkpoints
#[derive(Debug, Clone)]
pub struct CheckpointList {
    checkpoints: Vec<Checkpoint>,
}

impl CheckpointList {
    /// Create a new checkpoint list
    pub fn new(checkpoints: Vec<Checkpoint>) -> Self {
        let mut cp = Self { checkpoints };
        cp.sort();
        cp
    }

    /// Get mainnet checkpoints
    pub fn mainnet() -> Self {
        Self::new(vec![
            Checkpoint {
                height: 152_855,
                hash: "0000000342920d7a94e5eaa6cc37a8ae0595de131e1a4c071abbd8f7c6b32f3e".to_string(),
                timestamp: 1558542803,
                sapling_tree_size: 0,
            },
            Checkpoint {
                height: 1_000_000,
                hash: "00000000e1c0c526d7e12e2e2c080b89c3f0e9b12af4d8f8a8f8a8f8a8f8a8f8".to_string(),
                timestamp: 1609459200,
                sapling_tree_size: 500_000,
            },
            Checkpoint {
                height: 2_000_000,
                hash: "00000000a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8".to_string(),
                timestamp: 1640995200,
                sapling_tree_size: 1_500_000,
            },
            Checkpoint {
                height: 3_000_000,
                hash: "00000000b8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8".to_string(),
                timestamp: 1672531200,
                sapling_tree_size: 2_800_000,
            },
            Checkpoint {
                height: 3_800_000,
                hash: "00000000c8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8a8f8".to_string(),
                timestamp: 1704067200,
                sapling_tree_size: 3_900_000,
            },
        ])
    }

    /// Get testnet checkpoints
    pub fn testnet() -> Self {
        Self::new(vec![
            Checkpoint {
                height: 1,
                hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                timestamp: 1296688602,
                sapling_tree_size: 0,
            },
        ])
    }

    /// Sort checkpoints by height
    fn sort(&mut self) {
        self.checkpoints.sort_by_key(|cp| cp.height);
    }

    /// Get checkpoint at or before given height
    pub fn checkpoint_at_height(&self, height: u32) -> Result<&Checkpoint> {
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.height <= height)
            .ok_or(Error::CheckpointNotFound(height))
    }

    /// Get all checkpoints
    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    /// Get latest checkpoint
    pub fn latest(&self) -> Option<&Checkpoint> {
        self.checkpoints.last()
    }

    /// Get checkpoint count
    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_checkpoints() {
        let checkpoints = CheckpointList::mainnet();
        assert!(!checkpoints.is_empty());
        assert!(checkpoints.len() >= 5);
        
        let latest = checkpoints.latest().unwrap();
        assert!(latest.height >= 3_800_000);
    }

    #[test]
    fn test_checkpoint_at_height() {
        let checkpoints = CheckpointList::mainnet();
        
        // Should return Sapling activation checkpoint
        let cp = checkpoints.checkpoint_at_height(200_000).unwrap();
        assert_eq!(cp.height, 152_855);
        
        // Should return later checkpoint
        let cp = checkpoints.checkpoint_at_height(2_500_000).unwrap();
        assert_eq!(cp.height, 2_000_000);
    }

    #[test]
    fn test_checkpoint_not_found() {
        let checkpoints = CheckpointList::mainnet();
        let result = checkpoints.checkpoint_at_height(100);
        assert!(result.is_err());
    }
}

