//! Sapling commitment tree frontier
//!
//! Provides efficient frontier tracking for the Sapling note commitment tree
//! using bridgetree with the canonical `zcash_primitives::sapling::Node` type.

use bridgetree::{BridgeTree, Frontier};
use incrementalmerkletree::{Hashable, Level, MerklePath, Position};
use serde::{Deserialize, Serialize};
use std::io::Read;
use zcash_primitives::{
    merkle_tree::HashSer,
    sapling::{note::ExtractedNoteCommitment, Node, NOTE_COMMITMENT_TREE_DEPTH},
};

use crate::{bridge_tree_codec, Error, Result};

/// Sapling note commitment tree depth (same as in Zcash/Pirate).
pub const SAPLING_TREE_DEPTH: u8 = NOTE_COMMITMENT_TREE_DEPTH as u8;
/// Maximum number of checkpoints to retain for the Sapling witness tree.
pub const MAX_CHECKPOINTS: usize = 100;

const SAPLING_BRIDGETREE_MAGIC: [u8; 4] = *b"SBT2";

/// Alias for compatibility with existing imports.
pub type SaplingCommitment = Node;

fn commitment_from_cmu_bytes(bytes: [u8; 32]) -> Result<Node> {
    let cmu: Option<ExtractedNoteCommitment> = ExtractedNoteCommitment::from_bytes(&bytes).into();
    cmu.map(|value| Node::from_cmu(&value))
        .ok_or_else(|| Error::Sync("Invalid Sapling commitment bytes".to_string()))
}

fn node_from_bytes(bytes: [u8; 32]) -> Result<Node> {
    let mut reader = &bytes[..];
    Node::read(&mut reader).map_err(|e| Error::Sync(format!("Invalid Sapling node bytes: {}", e)))
}

/// Sapling commitment tree frontier.
///
/// Uses a BridgeTree to retain witnesses for marked notes across sync.
pub struct SaplingFrontier {
    inner: BridgeTree<Node, u32, SAPLING_TREE_DEPTH>,
}

impl SaplingFrontier {
    /// Create a new empty frontier.
    pub fn new() -> Self {
        Self {
            inner: BridgeTree::new(MAX_CHECKPOINTS),
        }
    }

    /// Create frontier from existing bridgetree frontier.
    pub fn from_inner(inner: Frontier<Node, SAPLING_TREE_DEPTH>) -> Self {
        let tree = inner.value().map_or_else(
            || BridgeTree::new(MAX_CHECKPOINTS),
            |frontier| BridgeTree::from_frontier(MAX_CHECKPOINTS, frontier.clone()),
        );
        Self { inner: tree }
    }

    /// Initialize this frontier from an existing frontier (tree state).
    pub fn init_from_frontier(&mut self, frontier: Frontier<Node, SAPLING_TREE_DEPTH>) {
        self.inner = frontier.value().map_or_else(
            || BridgeTree::new(MAX_CHECKPOINTS),
            |frontier| BridgeTree::from_frontier(MAX_CHECKPOINTS, frontier.clone()),
        );
    }

    /// Get the current tree position (number of leaves - 1, or None if empty).
    pub fn position(&self) -> Option<u64> {
        self.inner.current_position().map(|pos| u64::from(pos))
    }

    /// Apply a note commitment to the frontier.
    pub fn apply_note_commitment(&mut self, cmu: [u8; 32]) -> Result<()> {
        let commitment = commitment_from_cmu_bytes(cmu)?;
        if !self.inner.append(commitment) {
            return Err(Error::Sync("Sapling note commitment tree is full".to_string()));
        }
        Ok(())
    }

    /// Apply a note commitment and return the new leaf position.
    pub fn apply_note_commitment_with_position(&mut self, cmu: [u8; 32]) -> Result<u64> {
        self.apply_note_commitment(cmu)?;
        let position = self
            .inner
            .current_position()
            .map(u64::from)
            .unwrap_or(0);
        Ok(position)
    }

    /// Mark the current tip position for witness tracking.
    pub fn mark_position(&mut self) -> Result<Position> {
        self.inner
            .mark()
            .ok_or_else(|| Error::Sync("Cannot mark: tree is empty".to_string()))
    }

    /// Get witness for a marked position.
    pub fn witness(&self, position: u64) -> Result<Option<MerklePath<Node, SAPLING_TREE_DEPTH>>> {
        let pos = Position::from(position);
        if !self.inner.marked_positions().contains(&pos) {
            return Ok(None);
        }

        let auth_path = self.inner.witness(pos, 0).map_err(|e| {
            Error::Sync(format!("Failed to compute Sapling witness for {}: {:?}", position, e))
        })?;

        let merkle_path = MerklePath::from_parts(auth_path, pos)
            .map_err(|e| Error::Sync(format!("Failed to build Sapling MerklePath: {:?}", e)))?;

        Ok(Some(merkle_path))
    }

    /// Get the current root hash.
    pub fn root(&self) -> [u8; 32] {
        self.inner
            .root(0)
            .map(|root| root.to_bytes())
            .unwrap_or_else(|| Node::empty_root(Level::from(SAPLING_TREE_DEPTH - 1)).to_bytes())
    }

    /// Serialize frontier to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&SAPLING_BRIDGETREE_MAGIC);
        let tree_bytes = bridge_tree_codec::serialize_bridge_tree(&self.inner)
            .expect("Sapling BridgeTree serialization failed");
        buf.extend_from_slice(&tree_bytes);
        buf
    }

    /// Deserialize frontier from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.starts_with(&SAPLING_BRIDGETREE_MAGIC) {
            let tree = bridge_tree_codec::deserialize_bridge_tree(&bytes[SAPLING_BRIDGETREE_MAGIC.len()..])?;
            return Ok(Self { inner: tree });
        }
        if bytes.starts_with(b"SBT1") {
            return Err(Error::Sync("Unsupported Sapling frontier snapshot version".to_string()));
        }

        Self::read_from_legacy(&mut &bytes[..])
    }

    fn read_from_legacy<R: Read>(mut reader: R) -> Result<Self> {
        let mut version = [0u8];
        reader
            .read_exact(&mut version)
            .map_err(|e| Error::Sync(e.to_string()))?;
        if version[0] != 1 {
            return Err(Error::Sync(format!(
                "Unknown Sapling frontier version: {}",
                version[0]
            )));
        }

        let mut position_bytes = [0u8; 8];
        reader
            .read_exact(&mut position_bytes)
            .map_err(|e| Error::Sync(e.to_string()))?;
        let position = u64::from_le_bytes(position_bytes);

        let mut has_leaf = [0u8];
        reader
            .read_exact(&mut has_leaf)
            .map_err(|e| Error::Sync(e.to_string()))?;

        if has_leaf[0] == 0 {
            return Ok(Self::new());
        }

        let mut leaf_bytes = [0u8; 32];
        reader
            .read_exact(&mut leaf_bytes)
            .map_err(|e| Error::Sync(e.to_string()))?;
        let leaf = node_from_bytes(leaf_bytes)?;

        let mut ommer_count = [0u8];
        reader
            .read_exact(&mut ommer_count)
            .map_err(|e| Error::Sync(e.to_string()))?;

        let mut ommers = Vec::with_capacity(ommer_count[0] as usize);
        for _ in 0..ommer_count[0] {
            let mut ommer_bytes = [0u8; 32];
            reader
                .read_exact(&mut ommer_bytes)
                .map_err(|e| Error::Sync(e.to_string()))?;
            ommers.push(node_from_bytes(ommer_bytes)?);
        }

        let frontier = Frontier::from_parts(Position::from(position), leaf, ommers)
            .map_err(|e| Error::Sync(format!("Invalid legacy frontier: {:?}", e)))?;

        Ok(Self::from_inner(frontier))
    }

    /// Check if frontier is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.current_position().is_none()
    }

    /// Get tree size (number of leaves appended).
    pub fn tree_size(&self) -> u64 {
        self.inner
            .current_position()
            .map(|pos| u64::from(pos) + 1)
            .unwrap_or(0)
    }
}

impl Default for SaplingFrontier {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SaplingFrontier {
    fn clone(&self) -> Self {
        let bytes = self.serialize();
        Self::deserialize(&bytes).unwrap_or_default()
    }
}

/// Snapshot of the frontier at a specific height.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierSnapshot {
    /// Block height at which this snapshot was taken.
    pub height: u32,
    /// Serialized frontier bytes.
    pub frontier_bytes: Vec<u8>,
    /// When this snapshot was created (ISO 8601).
    pub created_at: String,
    /// App version that created this snapshot.
    pub app_version: String,
}

impl FrontierSnapshot {
    /// Create a new frontier snapshot.
    pub fn new(height: u32, frontier: &SaplingFrontier, app_version: &str) -> Self {
        Self {
            height,
            frontier_bytes: frontier.serialize(),
            created_at: chrono::Utc::now().to_rfc3339(),
            app_version: app_version.to_string(),
        }
    }

    /// Restore frontier from snapshot.
    pub fn restore_frontier(&self) -> Result<SaplingFrontier> {
        SaplingFrontier::deserialize(&self.frontier_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bls12_381::Scalar;

    fn valid_cmu(seed: u64) -> [u8; 32] {
        Scalar::from(seed + 1).to_repr()
    }

    #[test]
    fn test_empty_frontier() {
        let frontier = SaplingFrontier::new();
        assert!(frontier.is_empty());
        assert_eq!(frontier.position(), None);
        assert_eq!(frontier.tree_size(), 0);
    }

    #[test]
    fn test_apply_commitment() {
        let mut frontier = SaplingFrontier::new();
        frontier.apply_note_commitment(valid_cmu(1)).unwrap();
        assert!(!frontier.is_empty());
        assert_eq!(frontier.position(), Some(0));
        assert_eq!(frontier.tree_size(), 1);
    }

    #[test]
    fn test_serialize_deserialize_with_data() {
        let mut frontier = SaplingFrontier::new();
        for i in 0..5 {
            frontier.apply_note_commitment(valid_cmu(i)).unwrap();
        }
        let bytes = frontier.serialize();
        let restored = SaplingFrontier::deserialize(&bytes).unwrap();
        assert_eq!(frontier.position(), restored.position());
        assert_eq!(frontier.root(), restored.root());
        assert_eq!(frontier.tree_size(), restored.tree_size());
    }

    #[test]
    fn test_frontier_snapshot() {
        let mut frontier = SaplingFrontier::new();
        frontier.apply_note_commitment(valid_cmu(42)).unwrap();
        let snapshot = FrontierSnapshot::new(100_000, &frontier, "1.0.0");
        let restored = snapshot.restore_frontier().unwrap();
        assert_eq!(frontier.root(), restored.root());
    }
}
