//! Note management

use serde::{Deserialize, Serialize};

/// Note value (arrrtoshis)
pub type NoteValue = u64;

/// Nullifier (spent note identifier)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nullifier(pub [u8; 32]);

/// Sapling note
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// Value in arrrtoshis
    pub value: NoteValue,
    /// Nullifier
    pub nullifier: Nullifier,
    /// Note commitment
    pub commitment: [u8; 32],
    /// Is spent
    pub spent: bool,
}

impl Note {
    /// Create new note
    pub fn new(value: NoteValue, nullifier: Nullifier, commitment: [u8; 32]) -> Self {
        Self {
            value,
            nullifier,
            commitment,
            spent: false,
        }
    }
}

