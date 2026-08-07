//! Semantic equivalence oracle for sync implementations.
//!
//! Wallet fields are encrypted with randomized nonces, so comparing database
//! files or ciphertext rows cannot prove two sync paths are equivalent. This
//! module compares decrypted wallet outcomes and deterministic chain/tree state.

use crate::{Database, Error, NoteType, Repository, Result, SyncStateStorage};
use rusqlite::types::ValueRef;
use std::collections::BTreeMap;

/// One canonical value in a semantic snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticOracleValue {
    /// SQL null.
    Null,
    /// Signed integer.
    Integer(i64),
    /// IEEE-754 value represented by its stable bit pattern.
    Real(u64),
    /// UTF-8 text.
    Text(String),
    /// Binary data.
    Blob(Vec<u8>),
}

/// Decrypted and normalized sync result for one wallet account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticOracleSnapshot {
    /// `(spendable, pending, total)` at the persisted local height.
    pub balance: (u64, u64, u64),
    /// Canonical decrypted notes, excluding unstable database row IDs.
    pub notes: Vec<Vec<SemanticOracleValue>>,
    /// Canonical transaction history.
    pub transactions: Vec<Vec<SemanticOracleValue>>,
    /// Unlinked nullifiers and their spending transaction IDs.
    pub unlinked_spends: Vec<Vec<SemanticOracleValue>>,
    /// Deterministic commitment-tree state, roots, and checkpoints.
    pub trees: BTreeMap<String, Vec<Vec<SemanticOracleValue>>>,
    /// Witness-ready note identities at the persisted anchor.
    pub witnesses: Vec<Vec<SemanticOracleValue>>,
    /// Deterministic repair and scan-queue state.
    pub repairs: BTreeMap<String, Vec<Vec<SemanticOracleValue>>>,
    /// Persisted sync cursor and canonical reorg window.
    pub cursor: BTreeMap<String, Vec<Vec<SemanticOracleValue>>>,
}

/// A named semantic difference between two sync results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticOracleDifference {
    /// Domain that differs, such as `notes` or `trees`.
    pub domain: String,
    /// Human-readable mismatch summary.
    pub detail: String,
}

impl SemanticOracleSnapshot {
    /// Capture all wallet semantics that an optimized sync path is allowed to affect.
    pub fn capture(db: &Database, account_id: i64) -> Result<Self> {
        let repo = Repository::new(db);
        let sync_state = SyncStateStorage::new(db).load_sync_state()?;
        let local_height = sync_state.local_height;
        let balance = repo.calculate_balance(account_id, local_height, 1)?;

        let mut notes = repo
            .get_account_notes(account_id)?
            .into_iter()
            .map(|note| {
                vec![
                    text(note_type_name(note.note_type)),
                    integer(note.account_id),
                    optional_integer(note.key_id),
                    integer(note.value),
                    blob(note.nullifier),
                    blob(note.commitment),
                    integer(i64::from(note.spent)),
                    integer(note.height),
                    blob(note.txid),
                    integer(note.output_index),
                    optional_blob(note.spent_txid),
                    optional_blob(note.diversifier),
                    optional_blob(note.note),
                    optional_integer(note.position),
                    optional_blob(note.memo),
                    optional_integer(note.address_id),
                ]
            })
            .collect::<Vec<_>>();
        notes.sort();

        let mut transactions = repo
            .get_transactions_with_options(account_id, None, local_height, 1, false)?
            .into_iter()
            .map(|tx| {
                vec![
                    text(tx.txid),
                    integer(tx.height),
                    integer(tx.timestamp),
                    integer(tx.amount),
                    SemanticOracleValue::Integer(i64::try_from(tx.fee).unwrap_or(i64::MAX)),
                    optional_blob(tx.memo),
                ]
            })
            .collect::<Vec<_>>();
        transactions.sort();

        let mut unlinked_spends = repo
            .list_unlinked_spend_nullifiers_with_txid(account_id)?
            .into_iter()
            .map(|(note_type, nullifier, spending_txid)| {
                vec![
                    text(note_type_name(note_type)),
                    blob(nullifier.to_vec()),
                    blob(spending_txid.to_vec()),
                ]
            })
            .collect::<Vec<_>>();
        unlinked_spends.sort();

        let trees = capture_tables(
            db,
            &[
                TableSpec::all("sapling_tree_cap"),
                TableSpec::all("sapling_tree_checkpoint_marks_removed"),
                TableSpec::all("sapling_tree_checkpoints"),
                TableSpec::all("sapling_tree_shards"),
                TableSpec::all("sapling_tree_retained_checkpoints"),
                TableSpec::all("orchard_tree_cap"),
                TableSpec::all("orchard_tree_checkpoint_marks_removed"),
                TableSpec::all("orchard_tree_checkpoints"),
                TableSpec::all("orchard_tree_shards"),
                TableSpec::all("orchard_tree_retained_checkpoints"),
                TableSpec::columns("checkpoints", &["height", "hash", "sapling_tree_size"]),
            ],
        )?;

        let witness_height = sync_state
            .last_checkpoint_height
            .min(sync_state.local_height);
        let mut witnesses = if witness_height == 0 {
            Vec::new()
        } else {
            repo.get_unspent_selectable_notes_at_anchor_filtered(
                account_id,
                witness_height,
                1,
                None,
                None,
            )?
            .into_iter()
            .map(|note| {
                let note_type = match note.note_type {
                    pirate_core::NoteType::Sapling => "Sapling",
                    pirate_core::NoteType::Ironwood => "Ironwood",
                };
                let position = note
                    .sapling_position
                    .or(note.ironwood_position)
                    .and_then(|value| i64::try_from(value).ok());
                let witness_material = match note.note_type {
                    pirate_core::NoteType::Sapling => note.merkle_path.as_ref().map(|path| {
                        path.path_elems()
                            .iter()
                            .flat_map(|node| node.to_bytes())
                            .collect::<Vec<_>>()
                    }),
                    pirate_core::NoteType::Ironwood => {
                        note.ironwood_merkle_path.as_ref().map(|path| {
                            let mut bytes = format!("{path:?}").into_bytes();
                            if let Some(anchor) = note.ironwood_anchor {
                                bytes.extend_from_slice(&anchor.to_bytes());
                            }
                            bytes
                        })
                    }
                };
                vec![
                    text(note_type),
                    blob(note.txid),
                    integer(i64::from(note.output_index)),
                    optional_integer(position),
                    blob(note.commitment),
                    optional_blob(witness_material),
                ]
            })
            .collect::<Vec<_>>()
        };
        witnesses.sort();

        let repairs = capture_tables(
            db,
            &[
                TableSpec::columns(
                    "scan_queue",
                    &["range_start", "range_end", "priority", "status", "reason"],
                ),
                TableSpec::columns(
                    "spendability_state",
                    &[
                        "spendable",
                        "rescan_required",
                        "target_height",
                        "anchor_height",
                        "validated_anchor_height",
                        "repair_queued",
                        "repair_from_height",
                        "reason_code",
                    ],
                ),
                TableSpec::all("sapling_note_shards"),
                TableSpec::all("orchard_note_shards"),
            ],
        )?;

        let cursor = capture_tables(
            db,
            &[
                TableSpec::columns(
                    "sync_state",
                    &["local_height", "target_height", "last_checkpoint_height"],
                ),
                TableSpec::all("chain_blocks"),
            ],
        )?;

        Ok(Self {
            balance,
            notes,
            transactions,
            unlinked_spends,
            trees,
            witnesses,
            repairs,
            cursor,
        })
    }

    /// Return every semantic domain that differs from `other`.
    pub fn differences(&self, other: &Self) -> Vec<SemanticOracleDifference> {
        let mut differences = Vec::new();
        compare_domain(&mut differences, "balance", &self.balance, &other.balance);
        compare_domain(&mut differences, "notes", &self.notes, &other.notes);
        compare_domain(
            &mut differences,
            "transactions",
            &self.transactions,
            &other.transactions,
        );
        compare_domain(
            &mut differences,
            "nullifiers/spends",
            &self.unlinked_spends,
            &other.unlinked_spends,
        );
        compare_domain(
            &mut differences,
            "trees/checkpoints",
            &self.trees,
            &other.trees,
        );
        compare_domain(
            &mut differences,
            "witnesses",
            &self.witnesses,
            &other.witnesses,
        );
        compare_domain(
            &mut differences,
            "repair queues",
            &self.repairs,
            &other.repairs,
        );
        compare_domain(&mut differences, "sync cursor", &self.cursor, &other.cursor);
        differences
    }

    /// Fail with a compact domain report when two snapshots are not equivalent.
    pub fn ensure_equivalent(&self, other: &Self) -> Result<()> {
        let differences = self.differences(other);
        if differences.is_empty() {
            return Ok(());
        }
        let summary = differences
            .iter()
            .map(|difference| format!("{}: {}", difference.domain, difference.detail))
            .collect::<Vec<_>>()
            .join("; ");
        Err(Error::Validation(format!(
            "sync semantic differential oracle failed: {}",
            summary
        )))
    }
}

fn compare_domain<T: std::fmt::Debug + PartialEq>(
    differences: &mut Vec<SemanticOracleDifference>,
    domain: &str,
    baseline: &T,
    candidate: &T,
) {
    if baseline != candidate {
        differences.push(SemanticOracleDifference {
            domain: domain.to_string(),
            detail: format!("baseline={baseline:?}, candidate={candidate:?}"),
        });
    }
}

#[derive(Clone, Copy)]
struct TableSpec {
    name: &'static str,
    columns: Option<&'static [&'static str]>,
}

impl TableSpec {
    const fn all(name: &'static str) -> Self {
        Self {
            name,
            columns: None,
        }
    }

    const fn columns(name: &'static str, columns: &'static [&'static str]) -> Self {
        Self {
            name,
            columns: Some(columns),
        }
    }
}

fn capture_tables(
    db: &Database,
    specs: &[TableSpec],
) -> Result<BTreeMap<String, Vec<Vec<SemanticOracleValue>>>> {
    let mut captured = BTreeMap::new();
    for spec in specs {
        if !table_exists(db, spec.name)? {
            continue;
        }
        let available = table_columns(db, spec.name)?;
        let columns = match spec.columns {
            Some(requested) => requested
                .iter()
                .filter(|column| available.iter().any(|value| value == **column))
                .copied()
                .collect::<Vec<_>>(),
            None => available
                .iter()
                .map(String::as_str)
                .filter(|column| !matches!(*column, "created_at" | "updated_at"))
                .collect::<Vec<_>>(),
        };
        if columns.is_empty() {
            continue;
        }

        let quoted_columns = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {} FROM {}",
            quoted_columns,
            quote_identifier(spec.name)
        );
        let mut statement = db.conn().prepare(&sql)?;
        let mut rows = statement.query([])?;
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            let mut cells = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                cells.push(value_from_ref(row.get_ref(index)?));
            }
            values.push(cells);
        }
        values.sort();
        captured.insert(spec.name.to_string(), values);
    }
    Ok(captured)
}

fn table_exists(db: &Database, table: &str) -> Result<bool> {
    Ok(db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn table_columns(db: &Database, table: &str) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", quote_identifier(table));
    let mut statement = db.conn().prepare(&sql)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn value_from_ref(value: ValueRef<'_>) -> SemanticOracleValue {
    match value {
        ValueRef::Null => SemanticOracleValue::Null,
        ValueRef::Integer(value) => SemanticOracleValue::Integer(value),
        ValueRef::Real(value) => SemanticOracleValue::Real(value.to_bits()),
        ValueRef::Text(value) => {
            SemanticOracleValue::Text(String::from_utf8_lossy(value).into_owned())
        }
        ValueRef::Blob(value) => SemanticOracleValue::Blob(value.to_vec()),
    }
}

fn note_type_name(note_type: NoteType) -> &'static str {
    match note_type {
        NoteType::Sapling => "Sapling",
        NoteType::Ironwood => "Ironwood",
    }
}

fn integer(value: i64) -> SemanticOracleValue {
    SemanticOracleValue::Integer(value)
}

fn text(value: impl Into<String>) -> SemanticOracleValue {
    SemanticOracleValue::Text(value.into())
}

fn blob(value: Vec<u8>) -> SemanticOracleValue {
    SemanticOracleValue::Blob(value)
}

fn optional_integer(value: Option<i64>) -> SemanticOracleValue {
    value.map_or(SemanticOracleValue::Null, integer)
}

fn optional_blob(value: Option<Vec<u8>>) -> SemanticOracleValue {
    value.map_or(SemanticOracleValue::Null, blob)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        truncate_above_height, ChainBlockRow, EncryptionAlgorithm, EncryptionKey, MasterKey,
        NoteRecord, SyncStateStorage,
    };
    use tempfile::NamedTempFile;

    fn test_db() -> (NamedTempFile, Database) {
        let file = NamedTempFile::new().expect("temporary database");
        let key = EncryptionKey::from_passphrase("oracle-test", &[7u8; 32])
            .expect("derive encryption key");
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key).expect("open database");
        (file, db)
    }

    fn note(value: i64, output_index: i64) -> NoteRecord {
        NoteRecord {
            id: None,
            account_id: 1,
            key_id: Some(9),
            note_type: NoteType::Sapling,
            value,
            nullifier: vec![output_index as u8; 32],
            commitment: vec![output_index as u8 + 1; 32],
            spent: false,
            height: 100,
            txid: vec![3; 32],
            output_index,
            address_id: None,
            spent_txid: None,
            diversifier: Some(vec![4; 11]),
            note: Some(vec![5; 32]),
            position: Some(output_index),
            memo: None,
        }
    }

    fn chain_block(height: u64, marker: u8) -> ChainBlockRow {
        ChainBlockRow {
            height,
            hash: vec![marker; 32],
            prev_hash: vec![marker.saturating_sub(1); 32],
            time: height as u32,
        }
    }

    fn add_final_non_note_semantics(db: &Database) {
        Repository::new(db)
            .upsert_unlinked_spend_nullifiers_with_txid(
                1,
                &[(NoteType::Sapling, [31; 32], [32; 32])],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO sapling_tree_shards (shard_index, subtree_end_height, root_hash, shard_data, contains_marked) VALUES (0, 101, ?1, NULL, 0)",
                [vec![41u8; 32]],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at) VALUES (100, 102, 10, 'done', 'oracle', 'a', 'b')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "UPDATE spendability_state SET spendable = 1, rescan_required = 0, target_height = 101, anchor_height = 100, validated_anchor_height = 100, repair_queued = 0, repair_from_height = 0, reason_code = 'READY' WHERE id = 1",
                [],
            )
            .unwrap();
    }

    #[test]
    fn equivalent_semantics_ignore_randomized_ciphertext() {
        let (_left_file, left) = test_db();
        let (_right_file, right) = test_db();
        Repository::new(&left).insert_note(&note(42, 0)).unwrap();
        Repository::new(&right).insert_note(&note(42, 0)).unwrap();
        SyncStateStorage::new(&left)
            .save_sync_state(100, 100, 0)
            .unwrap();
        SyncStateStorage::new(&right)
            .save_sync_state(100, 100, 0)
            .unwrap();

        let baseline = SemanticOracleSnapshot::capture(&left, 1).unwrap();
        let candidate = SemanticOracleSnapshot::capture(&right, 1).unwrap();
        baseline.ensure_equivalent(&candidate).unwrap();
    }

    #[test]
    fn reports_the_domain_containing_a_regression() {
        let (_left_file, left) = test_db();
        let (_right_file, right) = test_db();
        Repository::new(&left).insert_note(&note(42, 0)).unwrap();
        Repository::new(&right).insert_note(&note(43, 0)).unwrap();
        SyncStateStorage::new(&left)
            .save_sync_state(100, 100, 0)
            .unwrap();
        SyncStateStorage::new(&right)
            .save_sync_state(100, 100, 0)
            .unwrap();

        let baseline = SemanticOracleSnapshot::capture(&left, 1).unwrap();
        let candidate = SemanticOracleSnapshot::capture(&right, 1).unwrap();
        let differences = baseline.differences(&candidate);
        assert!(differences
            .iter()
            .any(|difference| difference.domain == "balance"));
        assert!(differences
            .iter()
            .any(|difference| difference.domain == "notes"));
    }

    #[test]
    fn optimized_interruption_and_reorg_path_matches_sequential_baseline() {
        let (_baseline_file, baseline) = test_db();
        Repository::new(&baseline)
            .insert_note(&note(42, 0))
            .unwrap();
        let mut second_note = note(24, 1);
        second_note.height = 101;
        second_note.txid = vec![4; 32];
        Repository::new(&baseline)
            .insert_note(&second_note)
            .unwrap();
        let baseline_state = SyncStateStorage::new(&baseline);
        baseline_state
            .save_chain_blocks(&[chain_block(100, 10), chain_block(101, 11)])
            .unwrap();
        baseline_state.save_sync_state(101, 101, 100).unwrap();
        add_final_non_note_semantics(&baseline);

        let candidate_file = NamedTempFile::new().unwrap();
        let candidate_key = EncryptionKey::from_passphrase("oracle-resume", &[8u8; 32]).unwrap();
        let candidate_master = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let mut candidate = Database::open(
            candidate_file.path(),
            &candidate_key,
            candidate_master.clone(),
        )
        .unwrap();
        Repository::new(&candidate)
            .insert_note(&note(42, 0))
            .unwrap();
        SyncStateStorage::new(&candidate)
            .save_chain_blocks(&[chain_block(100, 10)])
            .unwrap();
        SyncStateStorage::new(&candidate)
            .save_sync_state(100, 101, 100)
            .unwrap();

        // Resume after an interruption, observe an orphan, then roll back to
        // the common ancestor before applying the canonical block.
        drop(candidate);
        candidate =
            Database::open_existing(candidate_file.path(), &candidate_key, candidate_master)
                .unwrap();
        let mut orphan = note(999, 9);
        orphan.height = 101;
        orphan.txid = vec![99; 32];
        Repository::new(&candidate).insert_note(&orphan).unwrap();
        SyncStateStorage::new(&candidate)
            .save_chain_blocks(&[chain_block(101, 99)])
            .unwrap();
        SyncStateStorage::new(&candidate)
            .save_sync_state(101, 101, 100)
            .unwrap();
        truncate_above_height(&candidate, 100).unwrap();
        Repository::new(&candidate)
            .insert_note(&second_note)
            .unwrap();
        SyncStateStorage::new(&candidate)
            .save_chain_blocks(&[chain_block(101, 11)])
            .unwrap();
        SyncStateStorage::new(&candidate)
            .save_sync_state(101, 101, 100)
            .unwrap();
        add_final_non_note_semantics(&candidate);

        let baseline_snapshot = SemanticOracleSnapshot::capture(&baseline, 1).unwrap();
        let optimized_snapshot = SemanticOracleSnapshot::capture(&candidate, 1).unwrap();
        baseline_snapshot
            .ensure_equivalent(&optimized_snapshot)
            .unwrap();
    }
}
