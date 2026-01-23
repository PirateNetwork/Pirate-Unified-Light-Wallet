//! Orchard commitment tree frontier
//!
//! Provides efficient frontier tracking for the Orchard note commitment tree
//! using bridgetree (compatible with zcash_primitives).
//!
//! Uses the Orchard Merkle hash (MerkleHashOrchard) from the orchard crate,
//! aligned with zcash_primitives behavior.
//!
//! Adapted from `orchard_wallet.rs` to maintain the Orchard commitment tree.

use bridgetree::BridgeTree;
use incrementalmerkletree::{MerklePath, Position};
use orchard::tree::MerkleHashOrchard;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use zcash_primitives::{consensus::BlockHeight, sapling::NOTE_COMMITMENT_TREE_DEPTH};

use crate::{bridge_tree_codec, Error, Result};

/// Maximum number of checkpoints to maintain
pub const MAX_CHECKPOINTS: usize = 100;

/// Orchard commitment tree frontier using BridgeTree
///
/// Tracks the Orchard note commitment tree with checkpoints,
/// allowing efficient appends, witness computation, and rewinding.
///
/// Mirrors the `Wallet` structure used by `orchard_wallet.rs`.
pub struct OrchardFrontier {
    /// The incremental Merkle tree used to track note commitments and witnesses
    inner: BridgeTree<MerkleHashOrchard, u32, NOTE_COMMITMENT_TREE_DEPTH>,
    /// The block height at which the last checkpoint was created, if any
    last_checkpoint: Option<BlockHeight>,
    /// Map from note position to whether it's marked (for witness tracking)
    marked_positions: BTreeMap<Position, bool>,
    /// Next position in the note commitment tree
    next_position: u64,
}

impl OrchardFrontier {
    /// Create a new empty frontier
    pub fn new() -> Self {
        Self {
            inner: BridgeTree::new(MAX_CHECKPOINTS),
            last_checkpoint: None,
            marked_positions: BTreeMap::new(),
            next_position: 0,
        }
    }

    /// Get the current tree position (number of leaves - 1, or None if empty)
    pub fn position(&self) -> Option<u64> {
        if self.next_position == 0 {
            None
        } else {
            Some(self.next_position - 1)
        }
    }

    /// Apply a note commitment to the frontier
    ///
    /// This advances the frontier by one leaf position.
    /// Returns the position of the newly added commitment, or None if tree is full.
    pub fn apply_note_commitment(&mut self, cm: [u8; 32]) -> Result<u64> {
        // Convert commitment bytes to ExtractedNoteCommitment, then to MerkleHashOrchard
        // Use the same cmx -> MerkleHashOrchard conversion as the node.
        use orchard::note::ExtractedNoteCommitment;

        let cmx_ct = ExtractedNoteCommitment::from_bytes(&cm);
        let cmx = match cmx_ct.into() {
            Some(c) => c,
            None => {
                return Err(Error::Sync("Invalid commitment bytes".to_string()));
            }
        };

        let commitment = MerkleHashOrchard::from_cmx(&cmx);

        // Append to tree - returns bool in bridgetree 0.4
        if !self.inner.append(commitment) {
            return Err(Error::Sync(
                "Orchard note commitment tree is full".to_string(),
            ));
        }

        let position = self.next_position;
        self.next_position = self.next_position.saturating_add(1);
        Ok(position)
    }

    /// Mark the current tip position for witness tracking
    ///
    /// This marks the most recently appended position so we can compute
    /// a witness for it later. Used when we receive an Orchard note.
    /// Returns the marked position.
    pub fn mark_position(&mut self) -> Result<Position> {
        // In bridgetree 0.4, mark() returns Position directly (not Result)
        // It marks the current tip (most recently appended)
        let pos = self
            .inner
            .mark()
            .ok_or_else(|| Error::Sync("Cannot mark: tree is empty".to_string()))?;
        self.marked_positions.insert(pos, true);
        Ok(pos)
    }

    /// Get witness for a marked position
    ///
    /// Computes the merkle path from the marked position to the current root.
    /// Returns None if the position is not marked or if the tree is empty.
    pub fn witness(
        &self,
        position: u64,
    ) -> Result<Option<MerklePath<MerkleHashOrchard, NOTE_COMMITMENT_TREE_DEPTH>>> {
        let pos = Position::from(position);

        // Check if position is marked
        if !self.marked_positions.contains_key(&pos) {
            return Ok(None);
        }

        // Get witness from bridge tree
        // Use checkpoint_depth = 0 to get witness from latest checkpoint
        // In bridgetree 0.4, witness() returns Result<Vec<MerkleHashOrchard>, Error>
        let auth_path = self.inner.witness(pos, 0).map_err(|e| {
            Error::Sync(format!(
                "Failed to compute witness for position {}: {:?}",
                position, e
            ))
        })?;

        // Create MerklePath from auth_path and position
        // MerklePath::from_parts constructs from authentication path and position
        let merkle_path = MerklePath::from_parts(auth_path, pos)
            .map_err(|e| Error::Sync(format!("Failed to create MerklePath: {:?}", e)))?;

        Ok(Some(merkle_path))
    }

    /// Get the current root hash
    pub fn root(&self) -> Option<[u8; 32]> {
        // In bridgetree 0.4, root() returns Option<MerkleHashOrchard>
        // checkpoint_depth = 0 means latest checkpoint
        self.inner
            .root(0)
            .map(|root: MerkleHashOrchard| root.to_bytes())
    }

    /// Checkpoint the tree at a specific block height
    ///
    /// This creates a checkpoint that allows rewinding to this point.
    /// Returns false if the height doesn't immediately succeed the last checkpoint.
    pub fn checkpoint(&mut self, height: BlockHeight) -> bool {
        // Checkpoints must be in order of sequential block height
        if let Some(last_height) = self.last_checkpoint {
            let expected_height = BlockHeight::from_u32(<u32>::from(last_height) + 1);
            if height != expected_height {
                tracing::error!(
                    "Expected checkpoint height {}, given {}",
                    expected_height,
                    height
                );
                return false;
            }
        }

        // In bridgetree 0.4, checkpoint takes u32
        // Keep tree checkpoints aligned with block height.
        self.inner.checkpoint(<u32>::from(height));
        self.last_checkpoint = Some(height);
        true
    }

    /// Rewind one checkpoint
    ///
    /// In bridgetree 0.4, rewind() takes no arguments and rewinds one checkpoint at a time.
    /// Returns false if no checkpoints remain.
    pub fn rewind_one(&mut self) -> bool {
        let rewound = self.inner.rewind();
        if rewound {
            self.next_position = self
                .inner
                .current_position()
                .map(|pos| u64::from(pos) + 1)
                .unwrap_or(0);
        }
        rewound
    }

    /// Rewind to a specific checkpoint height
    ///
    /// Removes all checkpoints and commitments after the specified height.
    /// This requires calling rewind_one() multiple times.
    pub fn rewind_to_height(&mut self, height: BlockHeight) -> Result<()> {
        if let Some(checkpoint_height) = self.last_checkpoint {
            if height > checkpoint_height {
                return Ok(()); // Nothing to rewind
            }

            // Calculate how many checkpoints to rewind
            let mut blocks_to_rewind = <u32>::from(checkpoint_height) - <u32>::from(height);
            if blocks_to_rewind == 0 {
                blocks_to_rewind = 1;
            }
            let checkpoint_count = self.inner.checkpoints().len();

            for _ in 0..blocks_to_rewind {
                if !self.inner.rewind() {
                    // No more checkpoints
                    if !self.inner.marked_indices().is_empty() {
                        return Err(Error::Sync(format!(
                            "Insufficient checkpoints to rewind to height {} (had {} checkpoints)",
                            height, checkpoint_count
                        )));
                    }
                    break;
                }
            }

            // Update last_checkpoint
            if checkpoint_count > blocks_to_rewind as usize {
                self.last_checkpoint = Some(height);
            } else {
                self.last_checkpoint = None;
            }

            // Remove marked positions that are after the rewind
            // marked_indices() returns a reference to a collection of marked positions
            // We need to check which positions are still valid
            // In bridgetree 0.4, marked_indices() returns something iterable
            let marked_indices: BTreeSet<Position> =
                self.inner.marked_indices().keys().copied().collect();
            self.marked_positions
                .retain(|&pos, _| marked_indices.contains(&pos));
        }

        self.next_position = self
            .inner
            .current_position()
            .map(|pos| u64::from(pos) + 1)
            .unwrap_or(0);

        Ok(())
    }

    /// Get tree size (number of leaves appended)
    /// In bridgetree 0.4, we use marked_indices to estimate size
    pub fn tree_size(&self) -> u64 {
        self.next_position
    }

    /// Check if frontier is empty
    pub fn is_empty(&self) -> bool {
        self.next_position == 0
    }

    /// Get last checkpoint height
    pub fn last_checkpoint(&self) -> Option<BlockHeight> {
        self.last_checkpoint
    }

    /// Serialize frontier to bytes
    pub fn serialize(&self) -> Vec<u8> {
        bridge_tree_codec::serialize_bridge_tree(&self.inner)
            .expect("Orchard BridgeTree serialization failed")
    }

    /// Deserialize frontier from bytes
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        let inner = bridge_tree_codec::deserialize_bridge_tree(bytes)?;
        let last_checkpoint = inner
            .checkpoints()
            .back()
            .map(|cp| BlockHeight::from_u32(*cp.id()));
        let marked_positions = inner
            .marked_positions()
            .into_iter()
            .map(|pos| (pos, true))
            .collect();
        let next_position = inner
            .current_position()
            .map(|pos| u64::from(pos) + 1)
            .unwrap_or(0);
        Ok(Self {
            inner,
            last_checkpoint,
            marked_positions,
            next_position,
        })
    }

    /// Initialize from a frontier (tree state) and reset positions.
    pub fn init_from_frontier(
        &mut self,
        frontier: incrementalmerkletree::frontier::Frontier<
            MerkleHashOrchard,
            NOTE_COMMITMENT_TREE_DEPTH,
        >,
    ) {
        self.inner = frontier.value().map_or_else(
            || BridgeTree::new(MAX_CHECKPOINTS),
            |nonempty_frontier| {
                BridgeTree::from_frontier(MAX_CHECKPOINTS, nonempty_frontier.clone())
            },
        );
        self.last_checkpoint = None;
        self.marked_positions.clear();
        self.next_position = frontier
            .value()
            .map(|f| u64::from(f.position()) + 1)
            .unwrap_or(0);
    }
}

impl Default for OrchardFrontier {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for OrchardFrontier {
    fn clone(&self) -> Self {
        let bytes = self.serialize();
        Self::deserialize(&bytes).unwrap_or_else(|_| Self::new())
    }
}

/// Snapshot of the Orchard frontier at a specific height
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchardFrontierSnapshot {
    /// Block height at which this snapshot was taken
    pub height: u32,
    /// Serialized frontier bytes
    pub frontier_bytes: Vec<u8>,
    /// When this snapshot was created (ISO 8601)
    pub created_at: String,
    /// App version that created this snapshot
    pub app_version: String,
}

impl OrchardFrontierSnapshot {
    /// Create a new frontier snapshot
    pub fn new(height: u32, frontier: &OrchardFrontier, app_version: &str) -> Self {
        Self {
            height,
            frontier_bytes: frontier.serialize(),
            created_at: chrono::Utc::now().to_rfc3339(),
            app_version: app_version.to_string(),
        }
    }

    /// Restore frontier from snapshot
    pub fn restore_frontier(&self) -> Result<OrchardFrontier> {
        OrchardFrontier::deserialize(&self.frontier_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_frontier() {
        let frontier = OrchardFrontier::new();
        assert!(frontier.is_empty());
        assert_eq!(frontier.position(), None);
        assert_eq!(frontier.tree_size(), 0);
    }

    #[test]
    fn test_apply_commitment() {
        let mut frontier = OrchardFrontier::new();
        let cm = [1u8; 32];

        let position = frontier.apply_note_commitment(cm).unwrap();

        assert!(!frontier.is_empty());
        assert_eq!(position, 0);
        assert_eq!(frontier.tree_size(), 1);
    }

    #[test]
    fn test_multiple_commitments() {
        let mut frontier = OrchardFrontier::new();

        for i in 0..10 {
            let mut cm = [0u8; 32];
            cm[0] = i;
            let pos = frontier.apply_note_commitment(cm).unwrap();
            assert_eq!(pos, u64::from(i));
        }

        assert_eq!(frontier.tree_size(), 10);
    }

    #[test]
    fn test_mark_and_witness() {
        let mut frontier = OrchardFrontier::new();

        // Add some commitments
        for i in 0..5 {
            let mut cm = [0u8; 32];
            cm[0] = i;
            let pos = frontier.apply_note_commitment(cm).unwrap();

            // Mark position for witness tracking
            if i == 2 {
                let marked = frontier.mark_position().unwrap();
                assert_eq!(marked, Position::from(pos));
            }
        }

        // Get witness for marked position
        let witness = frontier.witness(2).unwrap();
        assert!(witness.is_some());
    }

    #[test]
    fn test_checkpoint() {
        let mut frontier = OrchardFrontier::new();

        // Add some commitments
        for i in 0..10 {
            let mut cm = [0u8; 32];
            cm[0] = i;
            frontier.apply_note_commitment(cm).unwrap();
        }

        // Checkpoint at height 100
        use zcash_primitives::consensus::BlockHeight;
        let height = BlockHeight::from_u32(100);
        assert!(frontier.checkpoint(height));

        // Add more commitments
        for i in 10..15 {
            let mut cm = [0u8; 32];
            cm[0] = i;
            frontier.apply_note_commitment(cm).unwrap();
        }

        // Rewind to checkpoint
        frontier.rewind_to_height(height).unwrap();

        // Tree should be back to size 10
        assert_eq!(frontier.tree_size(), 10);
    }
}
