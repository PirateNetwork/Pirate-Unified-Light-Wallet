//! Note selection algorithms for transaction building
//!
//! Implements various selection strategies: first-fit, smallest-first, largest-first.

use crate::{Error, Result};
use zcash_primitives::{
    sapling::{Diversifier, Note, Node},
};
use incrementalmerkletree::MerklePath;

/// Note type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteType {
    /// Sapling note.
    Sapling,
    /// Orchard note.
    Orchard,
}

/// Sapling or Orchard note for selection
#[derive(Debug)]
pub struct SelectableNote {
    /// Note type (Sapling or Orchard)
    pub note_type: NoteType,
    /// Note value in arrrtoshis
    pub value: u64,
    /// Note commitment
    pub commitment: Vec<u8>,
    /// Nullifier (if known)
    pub nullifier: Option<Vec<u8>>,
    /// Block height note was received
    pub height: u64,
    /// Transaction ID
    pub txid: Vec<u8>,
    /// Output index
    pub output_index: u32,
    /// Optional merkle path for spends (Sapling)
    pub merkle_path: Option<MerklePath<Node, { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH }>>,
    /// Optional diversifier used to derive the address (Sapling)
    pub diversifier: Option<Diversifier>,
    /// Optional full Sapling note
    pub note: Option<Note>,
    /// Optional Orchard anchor (Orchard)
    pub orchard_anchor: Option<orchard::tree::Anchor>,
    /// Optional Orchard note position (Orchard)
    pub orchard_position: Option<u64>,
    /// Optional full Orchard note (Orchard)
    pub orchard_note: Option<orchard::Note>,
    /// Optional Orchard merkle path (Orchard)
    pub orchard_merkle_path: Option<orchard::tree::MerklePath>,
}

impl SelectableNote {
    /// Create new selectable Sapling note
    pub fn new(value: u64, commitment: Vec<u8>, height: u64, txid: Vec<u8>, output_index: u32) -> Self {
        Self {
            note_type: NoteType::Sapling,
            value,
            commitment,
            nullifier: None,
            height,
            txid,
            output_index,
            merkle_path: None,
            diversifier: None,
            note: None,
            orchard_anchor: None,
            orchard_position: None,
            orchard_note: None,
            orchard_merkle_path: None,
        }
    }

    /// Create new selectable Orchard note
    pub fn new_orchard(
        value: u64,
        commitment: Vec<u8>,
        height: u64,
        txid: Vec<u8>,
        output_index: u32,
    ) -> Self {
        Self {
            note_type: NoteType::Orchard,
            value,
            commitment,
            nullifier: None,
            height,
            txid,
            output_index,
            merkle_path: None,
            diversifier: None,
            note: None,
            orchard_anchor: None,
            orchard_position: None,
            orchard_note: None,
            orchard_merkle_path: None,
        }
    }

    /// Set nullifier
    pub fn with_nullifier(mut self, nullifier: Vec<u8>) -> Self {
        self.nullifier = Some(nullifier);
        self
    }

    /// Attach Sapling witness path and diversifier
    pub fn with_witness(
        mut self,
        path: MerklePath<Node, { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH }>,
        diversifier: Diversifier,
        note: Note,
    ) -> Self {
        self.merkle_path = Some(path);
        self.diversifier = Some(diversifier);
        self.note = Some(note);
        self
    }

    /// Attach Orchard witness data
    pub fn with_orchard_witness(
        mut self,
        anchor: orchard::tree::Anchor,
        position: u64,
        merkle_path: orchard::tree::MerklePath,
        note: orchard::Note,
    ) -> Self {
        self.orchard_anchor = Some(anchor);
        self.orchard_position = Some(position);
        self.orchard_merkle_path = Some(merkle_path);
        self.orchard_note = Some(note);
        self
    }
}

/// Note selection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select notes in order received (FIFO)
    FirstFit,
    /// Select smallest notes first (minimize change, privacy)
    SmallestFirst,
    /// Select largest notes first (minimize inputs)
    LargestFirst,
    /// Select oldest notes first (clear out old UTXOs)
    OldestFirst,
}

/// Note selection result
#[derive(Debug)]
pub struct SelectionResult {
    /// Selected notes
    pub notes: Vec<SelectableNote>,
    /// Total value of selected notes
    pub total_value: u64,
    /// Change amount (if any)
    pub change: u64,
}

/// Note selector
pub struct NoteSelector {
    strategy: SelectionStrategy,
}

impl NoteSelector {
    /// Create selector with strategy
    pub fn new(strategy: SelectionStrategy) -> Self {
        Self { strategy }
    }

    /// Select notes to cover target amount plus fee
    /// 
    /// Takes ownership of available_notes because SelectableNote can't be cloned
    /// (Orchard MerklePath doesn't implement Clone)
    pub fn select_notes(
        &self,
        mut available_notes: Vec<SelectableNote>,
        target_amount: u64,
        fee: u64,
    ) -> Result<SelectionResult> {
        let required = target_amount
            .checked_add(fee)
            .ok_or_else(|| Error::InsufficientFunds("Amount overflow".to_string()))?;

        tracing::debug!(
            "Selecting notes: target={}, fee={}, required={}",
            target_amount,
            fee,
            required
        );

        // Sort notes based on strategy
        self.sort_notes(&mut available_notes);

        // Select notes
        let mut selected = Vec::new();
        let mut total = 0u64;

        for note in available_notes {
            if total >= required {
                break;
            }

            // Move note into selected (can't clone because Orchard MerklePath doesn't implement Clone)
            let note_value = note.value;
            selected.push(note);
            total = total
                .checked_add(note_value)
                .ok_or_else(|| Error::InsufficientFunds("Value overflow".to_string()))?;
        }

        // Check if we have enough
        if total < required {
            return Err(Error::InsufficientFunds(format!(
                "Required {} arrrtoshis, have {} arrrtoshis",
                required, total
            )));
        }

        let change = total - required;

        tracing::info!(
            "Selected {} notes, total={}, change={}",
            selected.len(),
            total,
            change
        );

        Ok(SelectionResult {
            notes: selected,
            total_value: total,
            change,
        })
    }

    fn sort_notes(&self, notes: &mut [SelectableNote]) {
        match self.strategy {
            SelectionStrategy::FirstFit => {
                // Already in order, no sorting needed
            }
            SelectionStrategy::SmallestFirst => {
                notes.sort_by(|a, b| a.value.cmp(&b.value));
            }
            SelectionStrategy::LargestFirst => {
                notes.sort_by(|a, b| b.value.cmp(&a.value));
            }
            SelectionStrategy::OldestFirst => {
                notes.sort_by(|a, b| a.height.cmp(&b.height));
            }
        }
    }

    /// Check if notes are sufficient without selecting
    pub fn check_sufficient(
        available_notes: &[SelectableNote],
        required_amount: u64,
    ) -> bool {
        let total: u64 = available_notes.iter().map(|n| n.value).sum();
        total >= required_amount
    }

    /// Get total available value
    pub fn total_available(available_notes: &[SelectableNote]) -> u64 {
        available_notes.iter().map(|n| n.value).sum()
    }

    /// Optimize selection (try multiple strategies, pick best)
    /// 
    /// Note: Since SelectableNote can't be cloned (Orchard MerklePath doesn't implement Clone),
    /// this function takes ownership and can only try one strategy. For now, it uses SmallestFirst
    /// which is typically best for privacy. If you need to try multiple strategies, you'll need
    /// to call select_notes multiple times with different note sets.
    pub fn optimize_selection(
        available_notes: Vec<SelectableNote>,
        target_amount: u64,
        fee: u64,
    ) -> Result<SelectionResult> {
        // Use SmallestFirst strategy (best for privacy - minimizes change)
        // Note: We can't try multiple strategies because SelectableNote can't be cloned
        let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
        selector.select_notes(available_notes, target_amount, fee)
    }
}

impl Default for NoteSelector {
    fn default() -> Self {
        Self::new(SelectionStrategy::SmallestFirst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_notes() -> Vec<SelectableNote> {
        vec![
            SelectableNote::new(100_000, vec![1], 1000, vec![1], 0),
            SelectableNote::new(500_000, vec![2], 1001, vec![2], 0),
            SelectableNote::new(250_000, vec![3], 1002, vec![3], 0),
            SelectableNote::new(1_000_000, vec![4], 1003, vec![4], 0),
        ]
    }

    #[test]
    fn test_smallest_first_selection() {
        let notes = create_test_notes();
        let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);

        let result = selector.select_notes(&notes, 300_000, 10_000).unwrap();

        // Should select smallest notes first: 100k + 250k + 500k = 850k
        // Required: 310k, so actually 100k + 250k = 350k should be enough
        assert!(result.total_value >= 310_000);
        assert_eq!(result.change, result.total_value - 310_000);
    }

    #[test]
    fn test_largest_first_selection() {
        let notes = create_test_notes();
        let selector = NoteSelector::new(SelectionStrategy::LargestFirst);

        let result = selector.select_notes(&notes, 300_000, 10_000).unwrap();

        // Should select largest note first: 1M
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.notes[0].value, 1_000_000);
    }

    #[test]
    fn test_insufficient_funds() {
        let notes = create_test_notes();
        let selector = NoteSelector::new(SelectionStrategy::FirstFit);

        let result = selector.select_notes(&notes, 5_000_000, 10_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_total_available() {
        let notes = create_test_notes();
        let total = NoteSelector::total_available(&notes);
        assert_eq!(total, 1_850_000);
    }

    #[test]
    fn test_check_sufficient() {
        let notes = create_test_notes();

        assert!(NoteSelector::check_sufficient(&notes, 1_000_000));
        assert!(!NoteSelector::check_sufficient(&notes, 2_000_000));
    }

    #[test]
    fn test_optimize_selection() {
        let notes = create_test_notes();

        let result = NoteSelector::optimize_selection(&notes, 300_000, 10_000).unwrap();

        // Should find a good selection
        assert!(result.total_value >= 310_000);
        assert!(result.notes.len() <= notes.len());
    }

    #[test]
    fn test_exact_amount() {
        let notes = vec![SelectableNote::new(100_000, vec![1], 1000, vec![1], 0)];
        let selector = NoteSelector::new(SelectionStrategy::FirstFit);

        let result = selector.select_notes(&notes, 90_000, 10_000).unwrap();

        assert_eq!(result.total_value, 100_000);
        assert_eq!(result.change, 0); // Exact match
    }
}
