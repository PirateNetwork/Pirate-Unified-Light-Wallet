//! Data access layer

mod address;

use crate::address_book::ColorTag;
use crate::frontier_witness::{
    construct_anchor_witnesses_from_db_state, resolve_orchard_anchor_from_db_state,
};
use crate::{models::*, Database, Result};
use pirate_core::DEFAULT_FEE;
use pirate_params::consensus::ConsensusParams;
use rusqlite::params_from_iter;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::ops::Deref;
use std::rc::Rc;

type NullifierBytes = [u8; 32];
type SpendingTxidBytes = [u8; 32];
type TypedNullifier = (NoteType, NullifierBytes);
type TypedUnlinkedSpend = (NoteType, NullifierBytes, SpendingTxidBytes);
type TypedUnlinkedSpendMap = HashMap<TypedNullifier, SpendingTxidBytes>;
const NOTE_SHARD_INDEX_BITS: u32 = 16;

/// Repository for database operations
pub struct Repository<'a> {
    db: RepositoryDatabase<'a>,
}

enum RepositoryDatabase<'a> {
    Borrowed(&'a Database),
    Shared(Rc<Database>),
}

impl Deref for RepositoryDatabase<'_> {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(db) => db,
            Self::Shared(db) => db.as_ref(),
        }
    }
}

/// Orchard note reference for validation (minimal decrypted fields).
#[derive(Debug, Clone)]
pub struct OrchardNoteRef {
    /// Raw txid bytes (32 bytes).
    pub txid: Vec<u8>,
    /// Output/action index within the transaction.
    pub output_index: i64,
    /// Note commitment (cmx).
    pub commitment: [u8; 32],
    /// Block height (if known).
    pub height: i64,
}

/// Result of a witness-consistency check at a fixed anchor height.
#[derive(Debug, Clone, Default)]
pub struct WitnessCheckResult {
    /// Total eligible unspent notes considered.
    pub considered_notes: usize,
    /// Missing witness/material count for Sapling notes.
    pub sapling_missing: usize,
    /// Missing witness/material count for Orchard notes.
    pub orchard_missing: usize,
    /// Deterministic queued repair ranges `(start, end_exclusive)`.
    pub repair_ranges: Vec<(u64, u64)>,
}

#[derive(Debug, Clone)]
struct WitnessCheckNoteMeta {
    note_type: crate::models::NoteType,
    height: i64,
    txid: Vec<u8>,
    output_index: i64,
    position: Option<i64>,
    has_note_material: bool,
}

fn note_value_is_valid(value: i64) -> bool {
    if value <= 0 {
        return false;
    }
    value as u64 <= ConsensusParams::mainnet().max_money
}

fn memo_bytes_are_effectively_empty(memo: &[u8]) -> bool {
    memo.is_empty() || memo.iter().all(|b| *b == 0)
}

/// Convert raw txid bytes (internal/little-endian) into canonical display hex.
fn txid_hex_from_bytes(txid_bytes: &[u8]) -> String {
    let mut display = txid_bytes.to_vec();
    display.reverse();
    hex::encode(display)
}

/// Reverse a txid hex string by bytes.
fn reverse_txid_hex(txid_hex: &str) -> Option<String> {
    if txid_hex.len() != 64 {
        return None;
    }
    let mut bytes = hex::decode(txid_hex).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    bytes.reverse();
    Some(hex::encode(bytes))
}

impl<'a> Repository<'a> {
    /// Create repository
    pub fn new(db: &'a Database) -> Self {
        Self {
            db: RepositoryDatabase::Borrowed(db),
        }
    }

    /// Create repository backed by a shared database handle.
    pub fn from_shared(db: Rc<Database>) -> Repository<'static> {
        Repository {
            db: RepositoryDatabase::Shared(db),
        }
    }

    /// Encrypt sensitive BLOB data
    fn encrypt_blob(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.db
            .master_key()
            .encrypt(data)
            .map_err(|e| crate::Error::Encryption(format!("Failed to encrypt data: {}", e)))
    }

    /// Decrypt sensitive BLOB data
    fn decrypt_blob(&self, encrypted: &[u8]) -> Result<Vec<u8>> {
        self.db
            .master_key()
            .decrypt(encrypted)
            .map_err(|e| crate::Error::Encryption(format!("Failed to decrypt data: {}", e)))
    }

    /// Encrypt optional BLOB
    fn encrypt_optional_blob(&self, data: Option<&[u8]>) -> Result<Option<Vec<u8>>> {
        match data {
            Some(d) => self.encrypt_blob(d).map(Some),
            None => Ok(None),
        }
    }

    /// Decrypt optional BLOB
    fn decrypt_optional_blob(&self, encrypted: Option<Vec<u8>>) -> Result<Option<Vec<u8>>> {
        match encrypted {
            Some(e) => self.decrypt_blob(&e).map(Some),
            None => Ok(None),
        }
    }

    /// Encrypt integer as BLOB (for privacy)
    fn encrypt_int64(&self, value: i64) -> Result<Vec<u8>> {
        self.encrypt_blob(&value.to_le_bytes())
    }

    /// Decrypt integer from BLOB
    fn decrypt_int64(&self, encrypted: &[u8]) -> Result<i64> {
        let decrypted = self.decrypt_blob(encrypted)?;
        if decrypted.len() != 8 {
            return Err(crate::Error::Encryption(
                "Invalid encrypted integer length".to_string(),
            ));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&decrypted);
        Ok(i64::from_le_bytes(bytes))
    }

    /// Encrypt optional integer as BLOB
    fn encrypt_optional_int64(&self, value: Option<i64>) -> Result<Option<Vec<u8>>> {
        match value {
            Some(v) => self.encrypt_int64(v).map(Some),
            None => Ok(None),
        }
    }

    /// Decrypt optional integer from BLOB
    fn decrypt_optional_int64(&self, encrypted: Option<Vec<u8>>) -> Result<Option<i64>> {
        match encrypted {
            Some(e) => self.decrypt_int64(&e).map(Some),
            None => Ok(None),
        }
    }

    /// Encrypt boolean as BLOB (for privacy)
    fn encrypt_bool(&self, value: bool) -> Result<Vec<u8>> {
        self.encrypt_blob(&[value as u8])
    }

    /// Decrypt boolean from BLOB
    fn decrypt_bool(&self, encrypted: &[u8]) -> Result<bool> {
        let decrypted = self.decrypt_blob(encrypted)?;
        if decrypted.is_empty() {
            return Err(crate::Error::Encryption(
                "Invalid encrypted boolean length".to_string(),
            ));
        }
        Ok(decrypted[0] != 0)
    }

    /// Decrypt string from BLOB
    fn decrypt_string(&self, encrypted: &[u8]) -> Result<String> {
        let decrypted = self.decrypt_blob(encrypted)?;
        String::from_utf8(decrypted)
            .map_err(|e| crate::Error::Encryption(format!("Failed to decode string: {}", e)))
    }

    fn decode_wallet_secret_wallet_id(&self, value: ValueRef<'_>) -> Result<(String, bool)> {
        match value {
            ValueRef::Text(text) => Ok((
                String::from_utf8(text.to_vec()).map_err(|e| {
                    crate::Error::Encryption(format!("Failed to decode plaintext wallet_id: {}", e))
                })?,
                false,
            )),
            ValueRef::Blob(blob) => {
                if let Ok(wallet_id) = self.decrypt_string(blob) {
                    return Ok((wallet_id, true));
                }

                Ok((
                    String::from_utf8(blob.to_vec()).map_err(|e| {
                        crate::Error::Encryption(format!("Failed to decode wallet_id bytes: {}", e))
                    })?,
                    false,
                ))
            }
            _ => Err(crate::Error::Encryption(
                "Unexpected wallet_id storage type in wallet_secrets".to_string(),
            )),
        }
    }

    /// Normalize wallet_secrets to a canonical layout keyed by plaintext wallet_id.
    ///
    /// Older versions stored wallet_id encrypted with a random nonce, which broke
    /// `ON CONFLICT(wallet_id)` semantics and allowed duplicate logical rows.
    pub fn normalize_wallet_secrets_storage(&self) -> Result<bool> {
        #[derive(Clone)]
        struct CanonicalWalletSecretRow {
            wallet_id: String,
            account_id: Vec<u8>,
            extsk: Vec<u8>,
            dfvk: Option<Vec<u8>>,
            orchard_extsk: Option<Vec<u8>>,
            sapling_ivk: Option<Vec<u8>>,
            orchard_ivk: Option<Vec<u8>>,
            encrypted_mnemonic: Option<Vec<u8>>,
            mnemonic_language: Option<String>,
            created_at: Vec<u8>,
            created_at_plain: i64,
        }

        let mut stmt = self.db.conn().prepare(
            "SELECT rowid, wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, mnemonic_language, created_at
             FROM wallet_secrets
             ORDER BY rowid ASC",
        )?;

        let mut rows = stmt.query([])?;
        let mut logical_row_count = 0usize;
        let mut saw_encrypted_wallet_id = false;
        let mut canonical_rows: HashMap<String, CanonicalWalletSecretRow> = HashMap::new();

        while let Some(row) = rows.next()? {
            logical_row_count += 1;
            let (wallet_id, was_encrypted) =
                self.decode_wallet_secret_wallet_id(row.get_ref(1)?)?;
            saw_encrypted_wallet_id |= was_encrypted;

            let account_id: Vec<u8> = row.get(2)?;
            let extsk: Vec<u8> = row.get(3)?;
            let dfvk: Option<Vec<u8>> = row.get(4)?;
            let orchard_extsk: Option<Vec<u8>> = row.get(5)?;
            let sapling_ivk: Option<Vec<u8>> = row.get(6)?;
            let orchard_ivk: Option<Vec<u8>> = row.get(7)?;
            let encrypted_mnemonic: Option<Vec<u8>> = row.get(8)?;
            let mnemonic_language: Option<String> = row.get(9)?;
            let created_at: Vec<u8> = row.get(10)?;
            let created_at_plain = self.decrypt_int64(&created_at)?;

            canonical_rows
                .entry(wallet_id.clone())
                .and_modify(|existing| {
                    existing.account_id = account_id.clone();
                    if !extsk.is_empty() {
                        existing.extsk = extsk.clone();
                    }
                    if dfvk.as_ref().is_some_and(|v| !v.is_empty()) {
                        existing.dfvk = dfvk.clone();
                    }
                    if orchard_extsk.as_ref().is_some_and(|v| !v.is_empty()) {
                        existing.orchard_extsk = orchard_extsk.clone();
                    }
                    if sapling_ivk.as_ref().is_some_and(|v| !v.is_empty()) {
                        existing.sapling_ivk = sapling_ivk.clone();
                    }
                    if orchard_ivk.as_ref().is_some_and(|v| !v.is_empty()) {
                        existing.orchard_ivk = orchard_ivk.clone();
                    }
                    if encrypted_mnemonic.as_ref().is_some_and(|v| !v.is_empty()) {
                        existing.encrypted_mnemonic = encrypted_mnemonic.clone();
                    }
                    if mnemonic_language.is_some() {
                        existing.mnemonic_language = mnemonic_language.clone();
                    }
                    if created_at_plain < existing.created_at_plain {
                        existing.created_at = created_at.clone();
                        existing.created_at_plain = created_at_plain;
                    }
                })
                .or_insert(CanonicalWalletSecretRow {
                    wallet_id,
                    account_id,
                    extsk,
                    dfvk,
                    orchard_extsk,
                    sapling_ivk,
                    orchard_ivk,
                    encrypted_mnemonic,
                    mnemonic_language,
                    created_at,
                    created_at_plain,
                });
        }
        drop(rows);
        drop(stmt);

        let duplicates_present = logical_row_count != canonical_rows.len();
        if !saw_encrypted_wallet_id && !duplicates_present {
            return Ok(false);
        }

        self.db
            .conn()
            .execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
        let normalize_result: Result<()> = (|| {
            self.db.conn().execute_batch(
                r#"
                CREATE TABLE wallet_secrets_normalized (
                    wallet_id TEXT PRIMARY KEY,
                    account_id INTEGER NOT NULL,
                    extsk BLOB NOT NULL,
                    dfvk BLOB,
                    orchard_extsk BLOB,
                    sapling_ivk BLOB,
                    orchard_ivk BLOB,
                    encrypted_mnemonic BLOB,
                    mnemonic_language TEXT,
                    created_at INTEGER NOT NULL
                );
                "#,
            )?;

            for row in canonical_rows.into_values() {
                self.db.conn().execute(
                    "INSERT INTO wallet_secrets_normalized (wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, mnemonic_language, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        row.wallet_id,
                        row.account_id,
                        row.extsk,
                        row.dfvk,
                        row.orchard_extsk,
                        row.sapling_ivk,
                        row.orchard_ivk,
                        row.encrypted_mnemonic,
                        row.mnemonic_language,
                        row.created_at,
                    ],
                )?;
            }

            self.db.conn().execute_batch(
                "DROP TABLE wallet_secrets;
                 ALTER TABLE wallet_secrets_normalized RENAME TO wallet_secrets;",
            )?;

            Ok(())
        })();

        match normalize_result {
            Ok(()) => {
                self.db.conn().execute_batch("COMMIT;")?;
                tracing::info!(
                    "Normalized wallet_secrets storage (encrypted_wallet_ids={}, duplicates={})",
                    saw_encrypted_wallet_id,
                    duplicates_present
                );
                Ok(true)
            }
            Err(err) => {
                let _ = self.db.conn().execute_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    fn shard_index_from_position(&self, position: i64) -> Option<i64> {
        if position < 0 {
            return None;
        }
        let pos_u64 = u64::try_from(position).ok()?;
        i64::try_from(pos_u64 >> NOTE_SHARD_INDEX_BITS).ok()
    }

    fn tree_shard_height_range_for_position(
        &self,
        note_type: crate::models::NoteType,
        position: i64,
    ) -> Result<Option<(u64, u64)>> {
        let Some(shard_index) = self.shard_index_from_position(position) else {
            return Ok(None);
        };
        let table = match note_type {
            crate::models::NoteType::Sapling => "sapling_tree_shards",
            crate::models::NoteType::Orchard => "orchard_tree_shards",
        };
        let mut stmt = self.db.conn().prepare(&format!(
            "SELECT
                (SELECT subtree_end_height FROM {table} prev WHERE prev.shard_index = shards.shard_index - 1),
                shards.subtree_end_height
             FROM {table} shards
             WHERE shards.shard_index = ?1"
        ))?;
        let row = stmt
            .query_row(params![shard_index], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .optional()?;
        let Some((prev_end_opt, end_opt)) = row else {
            return Ok(None);
        };
        let Some(end_i64) = end_opt else {
            return Ok(None);
        };
        let start_u64 = prev_end_opt
            .and_then(|value| u64::try_from(value).ok())
            .map(|value| value.saturating_add(1))
            .unwrap_or(1);
        let end_u64 = match u64::try_from(end_i64) {
            Ok(value) => value.saturating_add(1),
            Err(_) => return Ok(None),
        };
        Ok(Some((start_u64, end_u64)))
    }

    fn upsert_note_shard_metadata(
        &self,
        note_type: crate::models::NoteType,
        position: Option<i64>,
        height: i64,
    ) -> Result<()> {
        if height <= 0 {
            return Ok(());
        }
        let Some(position_i64) = position else {
            return Ok(());
        };
        let Some(shard_index) = self.shard_index_from_position(position_i64) else {
            return Ok(());
        };
        let start_position = shard_index << NOTE_SHARD_INDEX_BITS;
        let end_position_exclusive = (shard_index + 1) << NOTE_SHARD_INDEX_BITS;
        let sql = match note_type {
            crate::models::NoteType::Sapling => {
                r#"
                INSERT INTO sapling_note_shards (
                    shard_index,
                    start_position,
                    end_position_exclusive,
                    subtree_start_height,
                    subtree_end_height,
                    contains_marked
                ) VALUES (?1, ?2, ?3, ?4, ?4, 1)
                ON CONFLICT(shard_index) DO UPDATE SET
                    start_position = excluded.start_position,
                    end_position_exclusive = excluded.end_position_exclusive,
                    subtree_start_height = MIN(subtree_start_height, excluded.subtree_start_height),
                    subtree_end_height = CASE
                        WHEN subtree_end_height IS NULL THEN excluded.subtree_end_height
                        WHEN excluded.subtree_end_height IS NULL THEN subtree_end_height
                        ELSE MAX(subtree_end_height, excluded.subtree_end_height)
                    END,
                    contains_marked = 1
                "#
            }
            crate::models::NoteType::Orchard => {
                r#"
                INSERT INTO orchard_note_shards (
                    shard_index,
                    start_position,
                    end_position_exclusive,
                    subtree_start_height,
                    subtree_end_height,
                    contains_marked
                ) VALUES (?1, ?2, ?3, ?4, ?4, 1)
                ON CONFLICT(shard_index) DO UPDATE SET
                    start_position = excluded.start_position,
                    end_position_exclusive = excluded.end_position_exclusive,
                    subtree_start_height = MIN(subtree_start_height, excluded.subtree_start_height),
                    subtree_end_height = CASE
                        WHEN subtree_end_height IS NULL THEN excluded.subtree_end_height
                        WHEN excluded.subtree_end_height IS NULL THEN subtree_end_height
                        ELSE MAX(subtree_end_height, excluded.subtree_end_height)
                    END,
                    contains_marked = 1
                "#
            }
        };
        self.db.conn().execute(
            sql,
            params![shard_index, start_position, end_position_exclusive, height],
        )?;
        Ok(())
    }

    /// Insert account
    pub fn insert_account(&self, account: &Account) -> Result<i64> {
        self.db.conn().execute(
            "INSERT INTO accounts (name, created_at) VALUES (?1, ?2)",
            params![account.name, account.created_at],
        )?;
        Ok(self.db.conn().last_insert_rowid())
    }

    /// Clear chain-derived state (notes, transactions, sync logs, and addresses).
    pub fn clear_chain_state(&self) -> Result<()> {
        let conn = self.db.conn();
        let updated_at = chrono::Utc::now().to_rfc3339();

        conn.execute_batch("BEGIN IMMEDIATE")?;

        let result = (|| {
            conn.execute("DELETE FROM notes", [])?;
            conn.execute("DELETE FROM transactions", [])?;
            conn.execute("DELETE FROM memos", [])?;
            conn.execute("DELETE FROM unlinked_spend_nullifiers", [])?;
            conn.execute("DELETE FROM checkpoints", [])?;
            conn.execute("DELETE FROM frontier_snapshots", [])?;
            conn.execute("DELETE FROM sync_logs", [])?;
            conn.execute("DELETE FROM addresses", [])?;
            conn.execute("DELETE FROM sapling_note_shards", [])?;
            conn.execute("DELETE FROM orchard_note_shards", [])?;
            conn.execute("DELETE FROM sapling_tree_cap", [])?;
            conn.execute("DELETE FROM sapling_tree_checkpoint_marks_removed", [])?;
            conn.execute("DELETE FROM sapling_tree_checkpoints", [])?;
            conn.execute("DELETE FROM sapling_tree_shards", [])?;
            conn.execute("DELETE FROM orchard_tree_cap", [])?;
            conn.execute("DELETE FROM orchard_tree_checkpoint_marks_removed", [])?;
            conn.execute("DELETE FROM orchard_tree_checkpoints", [])?;
            conn.execute("DELETE FROM orchard_tree_shards", [])?;
            conn.execute(
                r#"
                UPDATE sync_state SET
                    local_height = 0,
                    target_height = 0,
                    last_checkpoint_height = 0,
                    updated_at = ?1
                WHERE id = 1
                "#,
                params![updated_at],
            )?;
            Ok(())
        })();

        if result.is_err() {
            let _ = conn.execute_batch("ROLLBACK");
        } else {
            conn.execute_batch("COMMIT")?;
        }

        result
    }

    /// Get account by ID
    pub fn get_account(&self, id: i64) -> Result<Account> {
        let account = self.db.conn().query_row(
            "SELECT id, name, created_at FROM accounts WHERE id = ?1",
            [id],
            |row| {
                Ok(Account {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )?;
        Ok(account)
    }

    fn insert_note_with_options(
        &self,
        note: &NoteRecord,
        update_shard_metadata: bool,
    ) -> Result<i64> {
        let note_type_str = match note.note_type {
            crate::models::NoteType::Sapling => "Sapling",
            crate::models::NoteType::Orchard => "Orchard",
        };

        // Encrypt ALL fields for maximum privacy (Pirate Chain privacy-first wallet)
        let encrypted_account_id = self.encrypt_int64(note.account_id)?;
        let encrypted_value = self.encrypt_int64(note.value)?;
        let encrypted_nullifier = self.encrypt_blob(&note.nullifier)?;
        let encrypted_commitment = self.encrypt_blob(&note.commitment)?;
        let encrypted_spent = self.encrypt_bool(note.spent)?;
        let encrypted_height = self.encrypt_int64(note.height)?;
        let encrypted_txid = self.encrypt_blob(&note.txid)?;
        let encrypted_output_index = self.encrypt_int64(note.output_index)?;
        let encrypted_diversifier =
            self.encrypt_blob(note.diversifier.as_deref().unwrap_or(&[]))?;
        let encrypted_note = self.encrypt_blob(note.note.as_deref().unwrap_or(&[]))?;
        let encrypted_position = self.encrypt_optional_int64(note.position)?;
        let encrypted_memo = self.encrypt_optional_blob(note.memo.as_deref())?;
        let encrypted_spent_txid = self.encrypt_optional_blob(note.spent_txid.as_deref())?;
        let encrypted_address_id = self.encrypt_optional_int64(note.address_id)?;
        let encrypted_key_id = self.encrypt_optional_int64(note.key_id)?;

        self.db.conn().execute(
            "INSERT INTO notes (account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                encrypted_account_id,
                note_type_str,
                encrypted_value,
                encrypted_nullifier,
                encrypted_commitment,
                encrypted_spent,
                encrypted_height,
                encrypted_txid,
                encrypted_output_index,
                encrypted_spent_txid,
                encrypted_diversifier,
                encrypted_note,
                encrypted_position,
                encrypted_memo,
                encrypted_address_id,
                encrypted_key_id,
            ],
        )?;
        let inserted_id = self.db.conn().last_insert_rowid();
        if update_shard_metadata {
            if let Err(e) =
                self.upsert_note_shard_metadata(note.note_type, note.position, note.height)
            {
                let _ = self
                    .db
                    .conn()
                    .execute("DELETE FROM notes WHERE id = ?1", params![inserted_id]);
                return Err(e);
            }
        }
        Ok(inserted_id)
    }

    /// Insert note with encrypted sensitive fields
    pub fn insert_note(&self, note: &NoteRecord) -> Result<i64> {
        self.insert_note_with_options(note, true)
    }

    /// Insert note without updating shard metadata. Intended for batch sync persistence.
    pub fn insert_note_without_shard_metadata(&self, note: &NoteRecord) -> Result<i64> {
        self.insert_note_with_options(note, false)
    }

    /// Update an existing note by row id (encrypts before storage)
    fn update_note_by_id_with_options(
        &self,
        note: &NoteRecord,
        update_shard_metadata: bool,
    ) -> Result<()> {
        let id = note.id.ok_or_else(|| {
            crate::error::Error::Storage("Missing note id for update".to_string())
        })?;
        let note_type_str = match note.note_type {
            crate::models::NoteType::Sapling => "Sapling",
            crate::models::NoteType::Orchard => "Orchard",
        };
        let encrypted_account_id = self.encrypt_int64(note.account_id)?;
        let encrypted_value = self.encrypt_int64(note.value)?;
        let encrypted_nullifier = self.encrypt_blob(&note.nullifier)?;
        let encrypted_commitment = self.encrypt_blob(&note.commitment)?;
        let encrypted_spent = self.encrypt_bool(note.spent)?;
        let encrypted_height = self.encrypt_int64(note.height)?;
        let encrypted_txid = self.encrypt_blob(&note.txid)?;
        let encrypted_output_index = self.encrypt_int64(note.output_index)?;
        let encrypted_diversifier =
            self.encrypt_blob(note.diversifier.as_deref().unwrap_or(&[]))?;
        let encrypted_note = self.encrypt_blob(note.note.as_deref().unwrap_or(&[]))?;
        let encrypted_position = self.encrypt_optional_int64(note.position)?;
        let encrypted_memo = self.encrypt_optional_blob(note.memo.as_deref())?;
        let encrypted_spent_txid = self.encrypt_optional_blob(note.spent_txid.as_deref())?;
        let encrypted_address_id = self.encrypt_optional_int64(note.address_id)?;
        let encrypted_key_id = self.encrypt_optional_int64(note.key_id)?;

        self.db.conn().execute(
            "UPDATE notes SET account_id = ?1, note_type = ?2, value = ?3, nullifier = ?4, commitment = ?5, spent = ?6, height = ?7, txid = ?8, output_index = ?9, spent_txid = ?10, diversifier = ?11, note = ?12, position = ?13, memo = ?14, address_id = ?15, key_id = ?16 WHERE id = ?17",
            params![
                encrypted_account_id,
                note_type_str,
                encrypted_value,
                encrypted_nullifier,
                encrypted_commitment,
                encrypted_spent,
                encrypted_height,
                encrypted_txid,
                encrypted_output_index,
                encrypted_spent_txid,
                encrypted_diversifier,
                encrypted_note,
                encrypted_position,
                encrypted_memo,
                encrypted_address_id,
                encrypted_key_id,
                id,
            ],
        )?;
        if update_shard_metadata {
            self.upsert_note_shard_metadata(note.note_type, note.position, note.height)?;
        }
        Ok(())
    }

    /// Update an existing note by row id (encrypts before storage)
    pub fn update_note_by_id(&self, note: &NoteRecord) -> Result<()> {
        self.update_note_by_id_with_options(note, true)
    }

    /// Update an existing note without updating shard metadata. Intended for batch sync persistence.
    pub fn update_note_by_id_without_shard_metadata(&self, note: &NoteRecord) -> Result<()> {
        self.update_note_by_id_with_options(note, false)
    }

    /// Batch-update note shard metadata for a sync batch.
    pub fn upsert_note_shard_metadata_batch<I>(&self, entries: I) -> Result<()>
    where
        I: IntoIterator<Item = (crate::models::NoteType, Option<i64>, i64)>,
    {
        let mut aggregated: HashMap<(crate::models::NoteType, i64), (i64, i64, i64, i64)> =
            HashMap::new();
        for (note_type, position, height) in entries {
            if height <= 0 {
                continue;
            }
            let Some(position_i64) = position else {
                continue;
            };
            let Some(shard_index) = self.shard_index_from_position(position_i64) else {
                continue;
            };
            let start_position = shard_index << NOTE_SHARD_INDEX_BITS;
            let end_position_exclusive = (shard_index + 1) << NOTE_SHARD_INDEX_BITS;
            let entry = aggregated.entry((note_type, shard_index)).or_insert((
                start_position,
                end_position_exclusive,
                height,
                height,
            ));
            entry.2 = entry.2.min(height);
            entry.3 = entry.3.max(height);
        }

        for (
            (note_type, shard_index),
            (start_position, end_position_exclusive, min_height, max_height),
        ) in aggregated
        {
            let sql = match note_type {
                crate::models::NoteType::Sapling => {
                    r#"
                    INSERT INTO sapling_note_shards (
                        shard_index,
                        start_position,
                        end_position_exclusive,
                        subtree_start_height,
                        subtree_end_height,
                        contains_marked
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 1)
                    ON CONFLICT(shard_index) DO UPDATE SET
                        start_position = excluded.start_position,
                        end_position_exclusive = excluded.end_position_exclusive,
                        subtree_start_height = MIN(subtree_start_height, excluded.subtree_start_height),
                        subtree_end_height = CASE
                            WHEN subtree_end_height IS NULL THEN excluded.subtree_end_height
                            WHEN excluded.subtree_end_height IS NULL THEN subtree_end_height
                            ELSE MAX(subtree_end_height, excluded.subtree_end_height)
                        END,
                        contains_marked = 1
                    "#
                }
                crate::models::NoteType::Orchard => {
                    r#"
                    INSERT INTO orchard_note_shards (
                        shard_index,
                        start_position,
                        end_position_exclusive,
                        subtree_start_height,
                        subtree_end_height,
                        contains_marked
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 1)
                    ON CONFLICT(shard_index) DO UPDATE SET
                        start_position = excluded.start_position,
                        end_position_exclusive = excluded.end_position_exclusive,
                        subtree_start_height = MIN(subtree_start_height, excluded.subtree_start_height),
                        subtree_end_height = CASE
                            WHEN subtree_end_height IS NULL THEN excluded.subtree_end_height
                            WHEN excluded.subtree_end_height IS NULL THEN subtree_end_height
                            ELSE MAX(subtree_end_height, excluded.subtree_end_height)
                        END,
                        contains_marked = 1
                    "#
                }
            };
            self.db.conn().execute(
                sql,
                params![
                    shard_index,
                    start_position,
                    end_position_exclusive,
                    min_height,
                    max_height
                ],
            )?;
        }

        Ok(())
    }

    /// Insert or update a transaction record.
    ///
    /// We store the **block timestamp** (first confirmation time) when available.
    pub fn upsert_transaction(
        &self,
        txid_hex: &str,
        height: i64,
        timestamp: i64,
        fee: i64,
    ) -> Result<()> {
        self.db.conn().execute(
            "INSERT INTO transactions (txid, height, timestamp, fee)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(txid) DO UPDATE SET
               height=excluded.height,
               timestamp=excluded.timestamp,
               fee=excluded.fee",
            params![txid_hex, height, timestamp, fee],
        )?;
        Ok(())
    }

    /// Insert or update an outgoing memo for a transaction.
    pub fn upsert_tx_memo(&self, txid_hex: &str, memo: &[u8]) -> Result<()> {
        let encrypted_memo = self.encrypt_blob(memo)?;

        let tx_id: Option<i64> = self
            .db
            .conn()
            .query_row(
                "SELECT id FROM transactions WHERE txid = ?1",
                params![txid_hex],
                |row| row.get(0),
            )
            .optional()?;

        let tx_id = match tx_id {
            Some(id) => id,
            None => {
                let timestamp = chrono::Utc::now().timestamp();
                self.db.conn().execute(
                    "INSERT INTO transactions (txid, height, timestamp, fee) VALUES (?1, ?2, ?3, ?4)",
                    params![txid_hex, 0, timestamp, 0],
                )?;
                self.db.conn().last_insert_rowid()
            }
        };

        self.db
            .conn()
            .execute("DELETE FROM memos WHERE tx_id = ?1", params![tx_id])?;
        self.db.conn().execute(
            "INSERT INTO memos (tx_id, memo) VALUES (?1, ?2)",
            params![tx_id, encrypted_memo],
        )?;

        Ok(())
    }

    /// Fetch a transaction-level memo, if present.
    pub fn get_tx_memo(&self, txid_hex: &str) -> Result<Option<Vec<u8>>> {
        let lookup_memo = |lookup_txid: &str| -> Result<Option<Vec<u8>>> {
            let mut stmt = self.db.conn().prepare(
                "SELECT m.memo FROM memos m INNER JOIN transactions t ON m.tx_id = t.id WHERE t.txid = ?1 ORDER BY m.id DESC LIMIT 1",
            )?;
            let encrypted: Option<Vec<u8>> = stmt
                .query_row(params![lookup_txid], |row| row.get(0))
                .optional()?;
            Ok(encrypted)
        };

        let encrypted = lookup_memo(txid_hex)?.or_else(|| {
            reverse_txid_hex(txid_hex)
                .and_then(|alt| if alt != txid_hex { Some(alt) } else { None })
                .and_then(|alt| lookup_memo(&alt).ok().flatten())
        });

        match encrypted {
            Some(enc) => self.decrypt_blob(&enc).map(Some),
            None => Ok(None),
        }
    }

    fn get_transaction_timestamps(&self, txids: &[String]) -> Result<HashMap<String, i64>> {
        if txids.is_empty() {
            return Ok(HashMap::new());
        }

        // Build a `WHERE txid IN (?, ?, ...)` query dynamically.
        let placeholders = std::iter::repeat_n("?", txids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT txid, timestamp FROM transactions WHERE txid IN ({})",
            placeholders
        );

        let mut stmt = self.db.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(txids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (txid, ts) = r?;
            map.insert(txid, ts);
        }
        Ok(map)
    }

    fn get_transaction_heights(&self, txids: &[String]) -> Result<HashMap<String, i64>> {
        if txids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = std::iter::repeat_n("?", txids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT txid, height FROM transactions WHERE txid IN ({})",
            placeholders
        );

        let mut stmt = self.db.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(txids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (txid, height) = r?;
            map.insert(txid, height);
        }

        Ok(map)
    }

    fn get_transaction_fees(&self, txids: &[String]) -> Result<HashMap<String, u64>> {
        if txids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = std::iter::repeat_n("?", txids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT txid, fee FROM transactions WHERE txid IN ({})",
            placeholders
        );

        let mut stmt = self.db.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(txids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (txid, fee) = r?;
            let fee = if fee < 0 { 0 } else { fee as u64 };
            map.insert(txid, fee);
        }

        Ok(map)
    }

    fn get_transaction_memos(&self, txids: &[String]) -> Result<HashMap<String, Vec<u8>>> {
        if txids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = std::iter::repeat_n("?", txids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT t.txid, m.memo FROM memos m INNER JOIN transactions t ON m.tx_id = t.id WHERE t.txid IN ({})",
            placeholders
        );

        let mut stmt = self.db.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(txids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (txid, encrypted_memo) = r?;
            let memo = self.decrypt_blob(&encrypted_memo)?;
            map.insert(txid, memo);
        }

        Ok(map)
    }

    /// Get unspent notes for account with decrypted sensitive fields
    /// Note: Since account_id is encrypted, we decrypt all notes and filter in memory for privacy
    pub fn get_unspent_notes(&self, account_id: i64) -> Result<Vec<NoteRecord>> {
        // Since account_id is encrypted, we need to decrypt all notes and filter
        // This is less efficient but necessary for maximum privacy
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id FROM notes",
        )?;

        let notes = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling, // Default to Sapling for backward compatibility
                };

                // All fields are encrypted for privacy - need explicit type annotations
                let encrypted_account_id: Vec<u8> = row.get::<_, Vec<u8>>(1)?;
                let encrypted_value: Vec<u8> = row.get::<_, Vec<u8>>(3)?;
                let encrypted_nullifier: Vec<u8> = row.get::<_, Vec<u8>>(4)?;
                let encrypted_commitment: Vec<u8> = row.get::<_, Vec<u8>>(5)?;
                let encrypted_spent: Vec<u8> = row.get::<_, Vec<u8>>(6)?;
                let encrypted_height: Vec<u8> = row.get::<_, Vec<u8>>(7)?;
                let encrypted_txid: Vec<u8> = row.get::<_, Vec<u8>>(8)?;
                let encrypted_output_index: Vec<u8> = row.get::<_, Vec<u8>>(9)?;
                let encrypted_spent_txid: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(10)?;
                let encrypted_diversifier: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(11)?;
                let encrypted_note: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(12)?;
                let encrypted_position: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(13)?;
                let encrypted_memo: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(14)?;
                let encrypted_address_id: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(15)?;
                let encrypted_key_id: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(16)?;

                // Note: Decryption happens after collecting to handle errors properly
                Ok((
                    row.get(0)?, // id
                    encrypted_account_id,
                    note_type,
                    encrypted_value,
                    encrypted_nullifier,
                    encrypted_commitment,
                    encrypted_spent,
                    encrypted_height,
                    encrypted_txid,
                    encrypted_output_index,
                    encrypted_spent_txid,
                    encrypted_diversifier,
                    encrypted_note,
                    encrypted_position,
                    encrypted_memo,
                    encrypted_address_id,
                    encrypted_key_id,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Decrypt all notes including all metadata for privacy.
        // If duplicate rows exist for the same output, any spent row wins so stale
        // unspent duplicates cannot inflate balance after migrations/re-syncs.
        let total_rows = notes.len();
        let mut invalid_values = 0usize;
        let mut spent_outputs: HashSet<(Vec<u8>, i64, crate::models::NoteType)> = HashSet::new();
        let mut unspent_by_output: HashMap<(Vec<u8>, i64, crate::models::NoteType), NoteRecord> =
            HashMap::new();
        for (
            id,
            enc_account_id,
            note_type,
            enc_value,
            enc_nullifier,
            enc_commitment,
            enc_spent,
            enc_height,
            enc_txid,
            enc_output_index,
            enc_spent_txid,
            enc_diversifier,
            enc_note,
            enc_position,
            enc_memo,
            enc_address_id,
            enc_key_id,
        ) in notes
        {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }
            let decrypted_txid = self.decrypt_blob(&enc_txid)?;
            let decrypted_output_index = self.decrypt_int64(&enc_output_index)?;
            let key = (decrypted_txid.clone(), decrypted_output_index, note_type);

            let decrypted_spent = self.decrypt_bool(&enc_spent)?;
            if decrypted_spent {
                spent_outputs.insert(key.clone());
                unspent_by_output.remove(&key);
                continue;
            }
            if spent_outputs.contains(&key) {
                continue;
            }

            let value = self.decrypt_int64(&enc_value)?;
            if !note_value_is_valid(value) {
                invalid_values += 1;
                continue;
            }
            let candidate = NoteRecord {
                id,
                account_id: decrypted_account_id,
                key_id: self.decrypt_optional_int64(enc_key_id)?,
                note_type,
                value,
                nullifier: self.decrypt_blob(&enc_nullifier)?,
                commitment: self.decrypt_blob(&enc_commitment)?,
                spent: decrypted_spent,
                height: self.decrypt_int64(&enc_height)?,
                txid: decrypted_txid,
                output_index: decrypted_output_index,
                spent_txid: self.decrypt_optional_blob(enc_spent_txid)?,
                diversifier: self.decrypt_optional_blob(enc_diversifier)?,
                note: self.decrypt_optional_blob(enc_note)?,
                position: self.decrypt_optional_int64(enc_position)?,
                memo: self.decrypt_optional_blob(enc_memo)?,
                address_id: self.decrypt_optional_int64(enc_address_id)?,
            };

            match unspent_by_output.get(&key) {
                Some(existing) => {
                    // Prefer the latest row for this output key.
                    let existing_id = existing.id.unwrap_or_default();
                    let candidate_id = candidate.id.unwrap_or_default();
                    if candidate_id > existing_id {
                        unspent_by_output.insert(key, candidate);
                    }
                }
                None => {
                    unspent_by_output.insert(key, candidate);
                }
            }
        }
        let decrypted_notes: Vec<NoteRecord> = unspent_by_output.into_values().collect();
        let matched = decrypted_notes.len();

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_unspent_notes","timestamp":{},"location":"repository.rs:344","message":"get_unspent_notes","data":{{"account_id":{},"rows":{},"matched":{},"invalid_values":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, account_id, total_rows, matched, invalid_values
            );
        });
        // #endregion

        Ok(decrypted_notes)
    }

    fn get_unspent_note_witness_metas(&self, account_id: i64) -> Result<Vec<WitnessCheckNoteMeta>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, spent, height, txid, output_index, position, note FROM notes",
        )?;

        let rows = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling,
                };
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    note_type,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Option<Vec<u8>>>(7)?,
                    row.get::<_, Option<Vec<u8>>>(8)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut spent_outputs: HashSet<(Vec<u8>, i64, crate::models::NoteType)> = HashSet::new();
        let mut metas_by_output: HashMap<
            (Vec<u8>, i64, crate::models::NoteType),
            (i64, WitnessCheckNoteMeta),
        > = HashMap::new();

        for (
            id,
            enc_account_id,
            note_type,
            enc_spent,
            enc_height,
            enc_txid,
            enc_output_index,
            enc_position,
            enc_note,
        ) in rows
        {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let txid = self.decrypt_blob(&enc_txid)?;
            let output_index = self.decrypt_int64(&enc_output_index)?;
            let key = (txid.clone(), output_index, note_type);

            let spent = self.decrypt_bool(&enc_spent)?;
            if spent {
                spent_outputs.insert(key.clone());
                metas_by_output.remove(&key);
                continue;
            }
            if spent_outputs.contains(&key) {
                continue;
            }

            let note_meta = WitnessCheckNoteMeta {
                note_type,
                height: self.decrypt_int64(&enc_height)?,
                txid,
                output_index,
                position: self.decrypt_optional_int64(enc_position)?,
                has_note_material: self
                    .decrypt_optional_blob(enc_note)?
                    .is_some_and(|value| !value.is_empty()),
            };

            match metas_by_output.get(&key) {
                Some((existing_id, _)) if *existing_id >= id => {}
                _ => {
                    metas_by_output.insert(key, (id, note_meta));
                }
            }
        }

        Ok(metas_by_output
            .into_values()
            .map(|(_, meta)| meta)
            .collect())
    }

    fn load_blocked_shard_ranges(
        &self,
        note_type: crate::models::NoteType,
        anchor_height: i64,
        wallet_birthday: i64,
        shard_indices: &HashSet<i64>,
    ) -> Result<HashMap<i64, Vec<(u64, u64)>>> {
        if shard_indices.is_empty() {
            return Ok(HashMap::new());
        }

        let view_name = match note_type {
            crate::models::NoteType::Sapling => "v_sapling_shard_unscanned_ranges",
            crate::models::NoteType::Orchard => "v_orchard_shard_unscanned_ranges",
        };
        let placeholders = std::iter::repeat_n("?", shard_indices.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT shard_index, block_range_start, block_range_end
             FROM {view_name}
             WHERE block_range_start <= ?1
               AND block_range_end > ?2
               AND shard_index IN ({placeholders})
             ORDER BY shard_index ASC, block_range_start ASC"
        );
        let mut params_vec = vec![Value::from(anchor_height), Value::from(wallet_birthday)];
        let mut ordered_indices: Vec<_> = shard_indices.iter().copied().collect();
        ordered_indices.sort_unstable();
        params_vec.extend(ordered_indices.into_iter().map(Value::from));

        let mut stmt = self.db.conn().prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params_vec.iter()))?;
        let mut blocked: HashMap<i64, Vec<(u64, u64)>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let shard_index: i64 = row.get(0)?;
            let start_i64: i64 = row.get(1)?;
            let end_i64: i64 = row.get(2)?;
            let Ok(start_u64) = u64::try_from(start_i64) else {
                continue;
            };
            let Ok(end_u64) = u64::try_from(end_i64) else {
                continue;
            };
            blocked
                .entry(shard_index)
                .or_default()
                .push((start_u64, end_u64));
        }

        Ok(blocked)
    }

    /// Get notes that are eligible for spend reconciliation.
    ///
    /// This returns a canonical per-output view used by sync spend matching:
    /// - prefer unspent rows
    /// - otherwise allow spent rows missing `spent_txid` (unresolved local state)
    /// - ignore rows already spent with a known `spent_txid`
    ///
    /// If duplicates exist for a single output key, the latest row id wins within
    /// the same priority class.
    pub fn get_spend_reconciliation_notes(&self, account_id: i64) -> Result<Vec<NoteRecord>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id FROM notes",
        )?;

        let notes = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling,
                };

                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    note_type,
                    row.get::<_, Vec<u8>>(3)?,          // encrypted value
                    row.get::<_, Vec<u8>>(4)?,          // encrypted nullifier
                    row.get::<_, Vec<u8>>(5)?,          // encrypted commitment
                    row.get::<_, Vec<u8>>(6)?,          // encrypted spent
                    row.get::<_, Vec<u8>>(7)?,          // encrypted height
                    row.get::<_, Vec<u8>>(8)?,          // encrypted txid
                    row.get::<_, Vec<u8>>(9)?,          // encrypted output_index
                    row.get::<_, Option<Vec<u8>>>(10)?, // encrypted spent_txid
                    row.get::<_, Option<Vec<u8>>>(11)?, // encrypted diversifier
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted key_id
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // (txid, output_index, note_type) -> (priority, note)
        // priority: 2=unspent, 1=spent without spent_txid
        let mut canonical: HashMap<(Vec<u8>, i64, crate::models::NoteType), (u8, NoteRecord)> =
            HashMap::new();

        for (
            id,
            enc_account_id,
            note_type,
            enc_value,
            enc_nullifier,
            enc_commitment,
            enc_spent,
            enc_height,
            enc_txid,
            enc_output_index,
            enc_spent_txid,
            enc_diversifier,
            enc_note,
            enc_position,
            enc_memo,
            enc_address_id,
            enc_key_id,
        ) in notes
        {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let spent = self.decrypt_bool(&enc_spent)?;
            let spent_txid = self.decrypt_optional_blob(enc_spent_txid)?;
            let priority = if !spent {
                2
            } else if spent_txid.is_none() {
                1
            } else {
                0
            };
            if priority == 0 {
                continue;
            }

            let txid = self.decrypt_blob(&enc_txid)?;
            let output_index = self.decrypt_int64(&enc_output_index)?;
            let key = (txid.clone(), output_index, note_type);

            let candidate = NoteRecord {
                id: Some(id),
                account_id: decrypted_account_id,
                key_id: self.decrypt_optional_int64(enc_key_id)?,
                note_type,
                value: self.decrypt_int64(&enc_value)?,
                nullifier: self.decrypt_blob(&enc_nullifier)?,
                commitment: self.decrypt_blob(&enc_commitment)?,
                spent,
                height: self.decrypt_int64(&enc_height)?,
                txid,
                output_index,
                address_id: self.decrypt_optional_int64(enc_address_id)?,
                spent_txid,
                diversifier: self.decrypt_optional_blob(enc_diversifier)?,
                note: self.decrypt_optional_blob(enc_note)?,
                position: self.decrypt_optional_int64(enc_position)?,
                memo: self.decrypt_optional_blob(enc_memo)?,
            };

            match canonical.get(&key) {
                Some((existing_pri, existing_note)) => {
                    let candidate_id = candidate.id.unwrap_or_default();
                    let existing_id = existing_note.id.unwrap_or_default();
                    if priority > *existing_pri
                        || (priority == *existing_pri && candidate_id > existing_id)
                    {
                        canonical.insert(key, (priority, candidate));
                    }
                }
                None => {
                    canonical.insert(key, (priority, candidate));
                }
            }
        }

        Ok(canonical.into_values().map(|(_, n)| n).collect())
    }

    /// Insert wallet secret (encrypted EXTSK)
    ///
    /// Note: This function expects the secret fields to already be encrypted.
    /// Use the encrypt_wallet_secret_fields helper to encrypt before calling.
    pub fn upsert_wallet_secret(&self, secret: &WalletSecret) -> Result<()> {
        // wallet_id is stored plaintext as the canonical lookup key.
        let encrypted_account_id = self.encrypt_int64(secret.account_id)?;
        let encrypted_created_at = self.encrypt_int64(secret.created_at)?;

        self.db.conn().execute(
            "INSERT INTO wallet_secrets (wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, mnemonic_language, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(wallet_id) DO UPDATE SET account_id=excluded.account_id, extsk=excluded.extsk, dfvk=excluded.dfvk, orchard_extsk=excluded.orchard_extsk, sapling_ivk=excluded.sapling_ivk, orchard_ivk=excluded.orchard_ivk, encrypted_mnemonic=excluded.encrypted_mnemonic, mnemonic_language=excluded.mnemonic_language, created_at=excluded.created_at",
            params![secret.wallet_id, encrypted_account_id, secret.extsk, secret.dfvk, secret.orchard_extsk, secret.sapling_ivk, secret.orchard_ivk, secret.encrypted_mnemonic, secret.mnemonic_language, encrypted_created_at],
        )?;
        Ok(())
    }

    /// Encrypt wallet secret fields before storage
    /// Note: account_id and created_at are encrypted in upsert_wallet_secret.
    pub fn encrypt_wallet_secret_fields(&self, secret: &WalletSecret) -> Result<WalletSecret> {
        Ok(WalletSecret {
            wallet_id: secret.wallet_id.clone(), // Stored plaintext as canonical lookup key
            account_id: secret.account_id,       // Will be encrypted in upsert_wallet_secret
            extsk: self.encrypt_blob(&secret.extsk)?,
            dfvk: self.encrypt_optional_blob(secret.dfvk.as_deref())?, // Encrypt viewing key for privacy
            orchard_extsk: self.encrypt_optional_blob(secret.orchard_extsk.as_deref())?,
            sapling_ivk: self.encrypt_optional_blob(secret.sapling_ivk.as_deref())?, // Encrypt viewing key for privacy
            orchard_ivk: self.encrypt_optional_blob(secret.orchard_ivk.as_deref())?, // Encrypt viewing key for privacy
            encrypted_mnemonic: self.encrypt_optional_blob(secret.encrypted_mnemonic.as_deref())?,
            mnemonic_language: secret.mnemonic_language.clone(),
            created_at: secret.created_at, // Will be encrypted in upsert_wallet_secret
        })
    }

    /// Get wallet secret and decrypt encrypted fields.
    pub fn get_wallet_secret(&self, wallet_id: &str) -> Result<Option<WalletSecret>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, mnemonic_language, created_at
             FROM wallet_secrets
             WHERE wallet_id = ?1",
        )?;

        let mut rows = stmt.query(params![wallet_id])?;
        if let Some(row) = rows.next()? {
            let wallet_id_plain: String = row.get(0)?;
            let encrypted_account_id: Vec<u8> = row.get(1)?;
            let encrypted_extsk: Vec<u8> = row.get(2)?;
            let encrypted_dfvk: Option<Vec<u8>> = row.get(3)?;
            let encrypted_orchard_extsk: Option<Vec<u8>> = row.get(4)?;
            let encrypted_sapling_ivk: Option<Vec<u8>> = row.get(5)?;
            let encrypted_orchard_ivk: Option<Vec<u8>> = row.get(6)?;
            let encrypted_mnemonic: Option<Vec<u8>> = row.get(7)?;
            let mnemonic_language: Option<String> = row.get(8)?;
            let encrypted_created_at: Vec<u8> = row.get(9)?;

            let account_id = self.decrypt_int64(&encrypted_account_id)?;
            let extsk = self.decrypt_blob(&encrypted_extsk)?;
            let dfvk = self.decrypt_optional_blob(encrypted_dfvk)?; // Decrypt viewing key for privacy
            let orchard_extsk = self.decrypt_optional_blob(encrypted_orchard_extsk)?;
            let sapling_ivk = self.decrypt_optional_blob(encrypted_sapling_ivk)?; // Decrypt viewing key for privacy
            let orchard_ivk = self.decrypt_optional_blob(encrypted_orchard_ivk)?; // Decrypt viewing key for privacy
            let encrypted_mnemonic = self.decrypt_optional_blob(encrypted_mnemonic)?;
            let created_at = self.decrypt_int64(&encrypted_created_at)?;

            return Ok(Some(WalletSecret {
                wallet_id: wallet_id_plain,
                account_id,
                extsk,
                dfvk,
                orchard_extsk,
                sapling_ivk,
                orchard_ivk,
                encrypted_mnemonic,
                mnemonic_language,
                created_at,
            }));
        }

        Ok(None)
    }

    /// Insert or update an account key (expects encrypted key material fields).
    pub fn upsert_account_key(&self, key: &AccountKey) -> Result<i64> {
        let key_type_str = match key.key_type {
            KeyType::Seed => "seed",
            KeyType::ImportSpend => "import_spend",
            KeyType::ImportView => "import_view",
        };
        let key_scope_str = match key.key_scope {
            KeyScope::Account => "account",
            KeyScope::SingleAddress => "single_address",
        };
        let spendable = if key.spendable { 1 } else { 0 };

        if let Some(id) = key.id {
            self.db.conn().execute(
                "UPDATE account_keys SET account_id = ?1, key_type = ?2, key_scope = ?3, label = ?4, birthday_height = ?5, created_at = ?6, spendable = ?7, sapling_extsk = ?8, sapling_dfvk = ?9, orchard_extsk = ?10, orchard_fvk = ?11, encrypted_mnemonic = ?12 WHERE id = ?13",
                params![
                    key.account_id,
                    key_type_str,
                    key_scope_str,
                    key.label,
                    key.birthday_height,
                    key.created_at,
                    spendable,
                    key.sapling_extsk,
                    key.sapling_dfvk,
                    key.orchard_extsk,
                    key.orchard_fvk,
                    key.encrypted_mnemonic,
                    id,
                ],
            )?;
            Ok(id)
        } else {
            self.db.conn().execute(
                "INSERT INTO account_keys (account_id, key_type, key_scope, label, birthday_height, created_at, spendable, sapling_extsk, sapling_dfvk, orchard_extsk, orchard_fvk, encrypted_mnemonic)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    key.account_id,
                    key_type_str,
                    key_scope_str,
                    key.label,
                    key.birthday_height,
                    key.created_at,
                    spendable,
                    key.sapling_extsk,
                    key.sapling_dfvk,
                    key.orchard_extsk,
                    key.orchard_fvk,
                    key.encrypted_mnemonic,
                ],
            )?;
            Ok(self.db.conn().last_insert_rowid())
        }
    }

    /// Encrypt account key material before storage.
    pub fn encrypt_account_key_fields(&self, key: &AccountKey) -> Result<AccountKey> {
        Ok(AccountKey {
            id: key.id,
            account_id: key.account_id,
            key_type: key.key_type,
            key_scope: key.key_scope,
            label: key.label.clone(),
            birthday_height: key.birthday_height,
            created_at: key.created_at,
            spendable: key.spendable,
            sapling_extsk: self.encrypt_optional_blob(key.sapling_extsk.as_deref())?,
            sapling_dfvk: self.encrypt_optional_blob(key.sapling_dfvk.as_deref())?,
            orchard_extsk: self.encrypt_optional_blob(key.orchard_extsk.as_deref())?,
            orchard_fvk: self.encrypt_optional_blob(key.orchard_fvk.as_deref())?,
            encrypted_mnemonic: self.encrypt_optional_blob(key.encrypted_mnemonic.as_deref())?,
        })
    }

    /// Load all account keys for a wallet account (decrypts key material).
    pub fn get_account_keys(&self, account_id: i64) -> Result<Vec<AccountKey>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_type, key_scope, label, birthday_height, created_at, spendable, sapling_extsk, sapling_dfvk, orchard_extsk, orchard_fvk, encrypted_mnemonic FROM account_keys WHERE account_id = ?1",
        )?;

        let keys = stmt
            .query_map([account_id], |row| {
                let key_type_str: String = row.get(2)?;
                let key_scope_str: String = row.get(3)?;
                let key_type = match key_type_str.as_str() {
                    "import_spend" => KeyType::ImportSpend,
                    "import_view" => KeyType::ImportView,
                    _ => KeyType::Seed,
                };
                let key_scope = match key_scope_str.as_str() {
                    "single_address" => KeyScope::SingleAddress,
                    _ => KeyScope::Account,
                };
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    key_type,
                    key_scope,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<Vec<u8>>>(8)?,
                    row.get::<_, Option<Vec<u8>>>(9)?,
                    row.get::<_, Option<Vec<u8>>>(10)?,
                    row.get::<_, Option<Vec<u8>>>(11)?,
                    row.get::<_, Option<Vec<u8>>>(12)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut decrypted = Vec::with_capacity(keys.len());
        for (
            id,
            acc_id,
            key_type,
            key_scope,
            label,
            birthday_height,
            created_at,
            spendable_raw,
            sapling_extsk,
            sapling_dfvk,
            orchard_extsk,
            orchard_fvk,
            encrypted_mnemonic,
        ) in keys
        {
            decrypted.push(AccountKey {
                id: Some(id),
                account_id: acc_id,
                key_type,
                key_scope,
                label,
                birthday_height,
                created_at,
                spendable: spendable_raw != 0,
                sapling_extsk: self.decrypt_optional_blob(sapling_extsk)?,
                sapling_dfvk: self.decrypt_optional_blob(sapling_dfvk)?,
                orchard_extsk: self.decrypt_optional_blob(orchard_extsk)?,
                orchard_fvk: self.decrypt_optional_blob(orchard_fvk)?,
                encrypted_mnemonic: self.decrypt_optional_blob(encrypted_mnemonic)?,
            });
        }

        Ok(decrypted)
    }

    /// Load a single account key by id (decrypts key material).
    pub fn get_account_key_by_id(&self, key_id: i64) -> Result<Option<AccountKey>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_type, key_scope, label, birthday_height, created_at, spendable, sapling_extsk, sapling_dfvk, orchard_extsk, orchard_fvk, encrypted_mnemonic FROM account_keys WHERE id = ?1",
        )?;

        let row = stmt
            .query_row([key_id], |row| {
                let key_type_str: String = row.get(2)?;
                let key_scope_str: String = row.get(3)?;
                let key_type = match key_type_str.as_str() {
                    "import_spend" => KeyType::ImportSpend,
                    "import_view" => KeyType::ImportView,
                    _ => KeyType::Seed,
                };
                let key_scope = match key_scope_str.as_str() {
                    "single_address" => KeyScope::SingleAddress,
                    _ => KeyScope::Account,
                };
                Ok((
                    row.get::<_, i64>(0)?, // id
                    row.get::<_, i64>(1)?, // account_id
                    key_type,
                    key_scope,
                    row.get::<_, Option<String>>(4)?,   // label
                    row.get::<_, i64>(5)?,              // birthday_height
                    row.get::<_, i64>(6)?,              // created_at
                    row.get::<_, i64>(7)?,              // spendable
                    row.get::<_, Option<Vec<u8>>>(8)?,  // sapling_extsk
                    row.get::<_, Option<Vec<u8>>>(9)?,  // sapling_dfvk
                    row.get::<_, Option<Vec<u8>>>(10)?, // orchard_extsk
                    row.get::<_, Option<Vec<u8>>>(11)?, // orchard_fvk
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted_mnemonic
                ))
            })
            .optional()?;

        let Some((
            id,
            account_id,
            key_type,
            key_scope,
            label,
            birthday_height,
            created_at,
            spendable_raw,
            sapling_extsk,
            sapling_dfvk,
            orchard_extsk,
            orchard_fvk,
            encrypted_mnemonic,
        )) = row
        else {
            return Ok(None);
        };

        Ok(Some(AccountKey {
            id: Some(id),
            account_id,
            key_type,
            key_scope,
            label,
            birthday_height,
            created_at,
            spendable: spendable_raw != 0,
            sapling_extsk: self.decrypt_optional_blob(sapling_extsk)?,
            sapling_dfvk: self.decrypt_optional_blob(sapling_dfvk)?,
            orchard_extsk: self.decrypt_optional_blob(orchard_extsk)?,
            orchard_fvk: self.decrypt_optional_blob(orchard_fvk)?,
            encrypted_mnemonic: self.decrypt_optional_blob(encrypted_mnemonic)?,
        }))
    }

    /// Delete an account key by id.
    pub fn delete_account_key(&self, key_id: i64) -> Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM account_keys WHERE id = ?1", [key_id])?;
        Ok(())
    }

    /// Get unspent notes as `SelectableNote` (for transaction building).
    /// Compatibility wrapper for `get_unspent_selectable_notes_filtered`.
    pub fn get_unspent_selectable_notes(
        &self,
        account_id: i64,
        key_id_filter: Option<i64>,
    ) -> Result<Vec<pirate_core::selection::SelectableNote>> {
        let key_ids = key_id_filter.map(|id| vec![id]);
        self.get_unspent_selectable_notes_filtered(account_id, key_ids, None)
    }

    /// Get unspent notes as `SelectableNote` (for transaction building) with optional filters.
    /// When filters are provided, notes matching either filter are included.
    pub fn get_unspent_selectable_notes_filtered(
        &self,
        account_id: i64,
        key_ids_filter: Option<Vec<i64>>,
        address_ids_filter: Option<Vec<i64>>,
    ) -> Result<Vec<pirate_core::selection::SelectableNote>> {
        use orchard::note::{
            Note as OrchardNote, Nullifier as OrchardNullifier, RandomSeed as OrchardRandomSeed,
        };
        use orchard::value::NoteValue as OrchardNoteValue;
        use orchard::Address as OrchardAddress;
        use pirate_core::selection::SelectableNote;
        use zcash_primitives::sapling::value::NoteValue as SaplingNoteValue;
        use zcash_primitives::sapling::{Note as SaplingNote, PaymentAddress, Rseed};

        const SAPLING_NOTE_BYTES_VERSION: u8 = 1;
        const ORCHARD_NOTE_BYTES_VERSION: u8 = 1;

        fn parse_sapling_note_bytes(bytes: &[u8]) -> Option<([u8; 43], u8, [u8; 32])> {
            let expected = 1 + 43 + 1 + 32;
            if bytes.len() >= expected && bytes[0] == SAPLING_NOTE_BYTES_VERSION {
                let mut address = [0u8; 43];
                address.copy_from_slice(&bytes[1..44]);
                let leadbyte = bytes[44];
                let mut rseed = [0u8; 32];
                rseed.copy_from_slice(&bytes[45..77]);
                return Some((address, leadbyte, rseed));
            }

            let legacy_expected = 43 + 1 + 32;
            if bytes.len() == legacy_expected {
                let mut address = [0u8; 43];
                address.copy_from_slice(&bytes[0..43]);
                let leadbyte = bytes[43];
                let mut rseed = [0u8; 32];
                rseed.copy_from_slice(&bytes[44..76]);
                return Some((address, leadbyte, rseed));
            }

            None
        }

        fn parse_orchard_note_bytes(bytes: &[u8]) -> Option<([u8; 43], [u8; 32], [u8; 32])> {
            let expected = 1 + 43 + 32 + 32;
            if bytes.len() >= expected && bytes[0] == ORCHARD_NOTE_BYTES_VERSION {
                let mut address = [0u8; 43];
                address.copy_from_slice(&bytes[1..44]);
                let mut rho = [0u8; 32];
                rho.copy_from_slice(&bytes[44..76]);
                let mut rseed = [0u8; 32];
                rseed.copy_from_slice(&bytes[76..108]);
                return Some((address, rho, rseed));
            }

            let legacy_expected = 43 + 32 + 32;
            if bytes.len() == legacy_expected {
                let mut address = [0u8; 43];
                address.copy_from_slice(&bytes[0..43]);
                let mut rho = [0u8; 32];
                rho.copy_from_slice(&bytes[43..75]);
                let mut rseed = [0u8; 32];
                rseed.copy_from_slice(&bytes[75..107]);
                return Some((address, rho, rseed));
            }

            None
        }

        let key_filter = key_ids_filter.map(|ids| ids.into_iter().collect::<HashSet<_>>());
        let address_filter = address_ids_filter.map(|ids| ids.into_iter().collect::<HashSet<_>>());
        let key_ids_count = key_filter.as_ref().map_or(0, |set| set.len());
        let address_ids_count = address_filter.as_ref().map_or(0, |set| set.len());
        let key_id_log = if key_ids_count == 1 {
            *key_filter.as_ref().unwrap().iter().next().unwrap()
        } else {
            -1
        };
        let address_id_log = if address_ids_count == 1 {
            *address_filter.as_ref().unwrap().iter().next().unwrap()
        } else {
            -1
        };

        let notes = self.get_unspent_notes(account_id)?;
        let eligible_address_ids: HashSet<i64> = self
            .get_all_addresses(account_id)?
            .into_iter()
            .filter_map(|address| {
                let id = address.id?;
                let unlabeled = address
                    .label
                    .as_ref()
                    .is_none_or(|label| label.trim().is_empty());
                let untagged = address.color_tag == ColorTag::None;
                if unlabeled && untagged {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        let mut skipped_key_mismatch = 0usize;
        let mut skipped_key_missing = 0usize;
        let mut skipped_address_mismatch = 0usize;
        let mut skipped_address_missing = 0usize;
        let notes = if key_filter.is_some() || address_filter.is_some() {
            let mut filtered = Vec::new();
            for note in notes {
                let key_match = key_filter
                    .as_ref()
                    .is_some_and(|set| note.key_id.is_some_and(|id| set.contains(&id)));
                let address_match = address_filter
                    .as_ref()
                    .is_some_and(|set| note.address_id.is_some_and(|id| set.contains(&id)));

                if key_match || address_match {
                    filtered.push(note);
                    continue;
                }

                if key_filter.is_some() {
                    match note.key_id {
                        Some(_) => skipped_key_mismatch += 1,
                        None => skipped_key_missing += 1,
                    }
                }

                if address_filter.is_some() {
                    match note.address_id {
                        Some(_) => skipped_address_mismatch += 1,
                        None => skipped_address_missing += 1,
                    }
                }
            }
            filtered
        } else {
            notes
        };
        let total_notes = notes.len();
        let mut result = Vec::with_capacity(total_notes);
        let mut skipped_missing_position = 0usize;
        let mut skipped_missing_note = 0usize;
        let mut skipped_invalid_address = 0usize;
        let mut skipped_invalid_rseed = 0usize;
        let mut skipped_invalid_note = 0usize;

        for n in notes {
            match n.note_type {
                crate::models::NoteType::Sapling => {
                    let (address_bytes, leadbyte, rseed_bytes) =
                        match n.note.as_deref().and_then(parse_sapling_note_bytes) {
                            Some(data) => data,
                            None => {
                                skipped_missing_note += 1;
                                continue;
                            }
                        };

                    let address = match PaymentAddress::from_bytes(&address_bytes) {
                        Some(addr) => addr,
                        None => {
                            skipped_invalid_address += 1;
                            continue;
                        }
                    };

                    let rseed = if leadbyte == 0x02 {
                        Rseed::AfterZip212(rseed_bytes)
                    } else {
                        let rcm = Option::from(jubjub::Fr::from_bytes(&rseed_bytes));
                        match rcm {
                            Some(value) => Rseed::BeforeZip212(value),
                            None => {
                                skipped_invalid_rseed += 1;
                                continue;
                            }
                        }
                    };

                    let value = match u64::try_from(n.value) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let note_value = SaplingNoteValue::from_raw(value);
                    let note = SaplingNote::from_parts(address, note_value, rseed);

                    let mut sn = SelectableNote::new(
                        value,
                        n.commitment.clone(),
                        n.height as u64,
                        n.txid.clone(),
                        n.output_index as u32,
                    );
                    sn = sn.with_key_id(n.key_id);
                    if let Some(position) = n.position {
                        if position >= 0 {
                            sn = sn.with_sapling_position(position as u64);
                        }
                    }
                    sn.auto_consolidation_eligible = n
                        .address_id
                        .is_some_and(|id| eligible_address_ids.contains(&id));

                    if let Some(nullifier) = (!n.nullifier.is_empty()).then(|| n.nullifier.clone())
                    {
                        sn = sn.with_nullifier(nullifier);
                    }
                    sn.diversifier = Some(*address.diversifier());
                    sn.note = Some(note);
                    result.push(sn);
                }
                crate::models::NoteType::Orchard => {
                    let (address_bytes, rho_bytes, rseed_bytes) =
                        match n.note.as_deref().and_then(parse_orchard_note_bytes) {
                            Some(data) => data,
                            None => {
                                skipped_missing_note += 1;
                                continue;
                            }
                        };

                    let address = match Option::from(OrchardAddress::from_raw_address_bytes(
                        &address_bytes,
                    )) {
                        Some(addr) => addr,
                        None => {
                            skipped_invalid_address += 1;
                            continue;
                        }
                    };

                    let rho = match Option::from(OrchardNullifier::from_bytes(&rho_bytes)) {
                        Some(value) => value,
                        None => {
                            skipped_invalid_note += 1;
                            continue;
                        }
                    };

                    let rseed = match Option::from(OrchardRandomSeed::from_bytes(rseed_bytes, &rho))
                    {
                        Some(value) => value,
                        None => {
                            skipped_invalid_rseed += 1;
                            continue;
                        }
                    };

                    let value = match u64::try_from(n.value) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let note_value = OrchardNoteValue::from_raw(value);
                    let note: OrchardNote = match Option::from(OrchardNote::from_parts(
                        address, note_value, rho, rseed,
                    )) {
                        Some(value) => value,
                        None => {
                            skipped_invalid_note += 1;
                            continue;
                        }
                    };

                    let mut sn = SelectableNote::new_orchard(
                        value,
                        n.commitment.clone(),
                        n.height as u64,
                        n.txid.clone(),
                        n.output_index as u32,
                    );
                    sn = sn.with_key_id(n.key_id);
                    sn.auto_consolidation_eligible = n
                        .address_id
                        .is_some_and(|id| eligible_address_ids.contains(&id));

                    if let Some(nullifier) = (!n.nullifier.is_empty()).then(|| n.nullifier.clone())
                    {
                        sn = sn.with_nullifier(nullifier);
                    }

                    let position = match n.position {
                        Some(pos) => pos as u64,
                        None => {
                            skipped_missing_position += 1;
                            continue;
                        }
                    };
                    sn.orchard_position = Some(position);
                    sn.orchard_note = Some(note);
                    result.push(sn);
                }
            }
        }

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_selectable_notes","timestamp":{},"location":"repository.rs:621","message":"get_unspent_selectable_notes","data":{{"account_id":{},"key_id":{},"address_id":{},"key_ids_count":{},"address_ids_count":{},"notes":{},"selectable":{},"missing_position":{},"missing_note":{},"invalid_address":{},"invalid_rseed":{},"invalid_note":{},"skipped_key_mismatch":{},"skipped_key_missing":{},"skipped_address_mismatch":{},"skipped_address_missing":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                account_id,
                key_id_log,
                address_id_log,
                key_ids_count,
                address_ids_count,
                total_notes,
                result.len(),
                skipped_missing_position,
                skipped_missing_note,
                skipped_invalid_address,
                skipped_invalid_rseed,
                skipped_invalid_note,
                skipped_key_mismatch,
                skipped_key_missing,
                skipped_address_mismatch,
                skipped_address_missing
            );
        });
        // #endregion

        Ok(result)
    }

    /// Get selectable notes constrained to a fixed anchor-height spendability epoch.
    ///
    /// This filters notes to those confirmed at or before `anchor_height`, which
    /// aligns spend selection with canonical fixed-anchor behavior.
    pub fn get_unspent_selectable_notes_at_anchor_filtered(
        &self,
        account_id: i64,
        anchor_height: u64,
        _min_confirmations: u32,
        key_ids_filter: Option<Vec<i64>>,
        address_ids_filter: Option<Vec<i64>>,
    ) -> Result<Vec<pirate_core::selection::SelectableNote>> {
        if anchor_height == 0 {
            return Ok(Vec::new());
        }
        let wallet_birthday = self.get_wallet_birthday_height(account_id)?.unwrap_or(0);
        let anchor_height_i64 = i64::try_from(anchor_height).map_err(|_| {
            crate::Error::Storage(format!("anchor_height {} exceeds i64::MAX", anchor_height))
        })?;
        let wallet_birthday_i64 = i64::try_from(wallet_birthday).map_err(|_| {
            crate::Error::Storage(format!(
                "wallet_birthday {} exceeds i64::MAX",
                wallet_birthday
            ))
        })?;

        let mut notes = self.get_unspent_selectable_notes_filtered(
            account_id,
            key_ids_filter,
            address_ids_filter,
        )?;
        if notes.is_empty() {
            return Ok(notes);
        }

        // Keep only notes at/below anchor and at/above wallet birthday.
        notes.retain(|note| note.height <= anchor_height && note.height >= wallet_birthday);
        if notes.is_empty() {
            return Ok(notes);
        }

        // Canonical shard-gate filtering:
        // - use persistent v_* shard-unscanned views from migration
        // - evaluate scannability in SQL using NOT EXISTS semantics
        // - keep deterministic wallet-birthday + fixed-anchor constraints
        notes.retain(|note| match note.note_type {
            pirate_core::selection::NoteType::Sapling => note.sapling_position.is_some(),
            pirate_core::selection::NoteType::Orchard => note.orchard_position.is_some(),
        });
        if notes.is_empty() {
            return Ok(notes);
        }

        let mut sapling_shard_indices: HashSet<i64> = HashSet::new();
        let mut orchard_shard_indices: HashSet<i64> = HashSet::new();
        for note in &notes {
            let note_position_i64 = match note.note_type {
                pirate_core::selection::NoteType::Sapling => note
                    .sapling_position
                    .and_then(|position| i64::try_from(position).ok()),
                pirate_core::selection::NoteType::Orchard => note
                    .orchard_position
                    .and_then(|position| i64::try_from(position).ok()),
            };
            let Some(position_i64) = note_position_i64 else {
                continue;
            };
            let Some(shard_index) = self.shard_index_from_position(position_i64) else {
                continue;
            };
            match note.note_type {
                pirate_core::selection::NoteType::Sapling => {
                    sapling_shard_indices.insert(shard_index);
                }
                pirate_core::selection::NoteType::Orchard => {
                    orchard_shard_indices.insert(shard_index);
                }
            }
        }
        let blocked_sapling_shards = self.load_blocked_shard_ranges(
            crate::models::NoteType::Sapling,
            anchor_height_i64,
            wallet_birthday_i64,
            &sapling_shard_indices,
        )?;
        let blocked_orchard_shards = self.load_blocked_shard_ranges(
            crate::models::NoteType::Orchard,
            anchor_height_i64,
            wallet_birthday_i64,
            &orchard_shard_indices,
        )?;

        let mut filtered = Vec::with_capacity(notes.len());
        for note in notes {
            let note_position_i64 = match note.note_type {
                pirate_core::selection::NoteType::Sapling => note
                    .sapling_position
                    .and_then(|position| i64::try_from(position).ok()),
                pirate_core::selection::NoteType::Orchard => note
                    .orchard_position
                    .and_then(|position| i64::try_from(position).ok()),
            };
            let Some(note_position_i64) = note_position_i64 else {
                continue;
            };
            let Some(shard_index) = self.shard_index_from_position(note_position_i64) else {
                continue;
            };
            let blocked = match note.note_type {
                pirate_core::selection::NoteType::Sapling => {
                    blocked_sapling_shards.contains_key(&shard_index)
                }
                pirate_core::selection::NoteType::Orchard => {
                    blocked_orchard_shards.contains_key(&shard_index)
                }
            };
            if !blocked {
                filtered.push(note);
            }
        }
        notes = filtered;
        construct_anchor_witnesses_from_db_state(&self.db, anchor_height, notes)
    }

    /// Resolve Orchard anchor from persisted DB state at-or-below `anchor_height`.
    pub fn resolve_orchard_anchor_from_db_state(
        &self,
        anchor_height: u64,
    ) -> Result<Option<orchard::tree::Anchor>> {
        resolve_orchard_anchor_from_db_state(&self.db, anchor_height)
    }

    /// Resolve Sapling anchor root bytes from persisted DB state at-or-below `anchor_height`.
    pub fn resolve_sapling_root_from_db_state(
        &self,
        anchor_height: u64,
    ) -> Result<Option<[u8; 32]>> {
        crate::frontier_witness::resolve_sapling_root_from_db_state(&self.db, anchor_height)
            .map(|root| root.map(|node| node.to_bytes()))
    }

    /// Get the minimum wallet birthday height across account keys for an account.
    ///
    /// Uses wallet-birthday semantics for constraining spendability
    /// gates and repair ranges.
    pub fn get_wallet_birthday_height(&self, account_id: i64) -> Result<Option<u64>> {
        let birthday_i64: Option<i64> = self.db.conn().query_row(
            "SELECT MIN(birthday_height) FROM account_keys WHERE account_id = ?1 AND birthday_height > 0",
            params![account_id],
            |row| row.get(0),
        )?;
        birthday_i64
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    crate::Error::Storage(format!(
                        "wallet birthday height out of range for account {}: {}",
                        account_id, value
                    ))
                })
            })
            .transpose()
    }

    /// Check witness/material availability and return deterministic FoundNote repair ranges.
    ///
    /// This function is intentionally queue-first: it never mutates note rows and does not
    /// repair inline. Callers should enqueue returned ranges via scan-queue processing.
    pub fn check_witnesses(
        &self,
        account_id: i64,
        anchor_height: u64,
        wallet_birthday: u64,
    ) -> Result<WitnessCheckResult> {
        let mut result = WitnessCheckResult::default();
        if anchor_height == 0 {
            return Ok(result);
        }

        let note_metas = self.get_unspent_note_witness_metas(account_id)?;
        if note_metas.is_empty() {
            return Ok(result);
        }

        let birthday = wallet_birthday.max(1);
        let mut pending_ranges: Vec<(u64, u64)> = Vec::new();
        let anchor_height_i64 = i64::try_from(anchor_height).map_err(|_| {
            crate::Error::Storage(format!("anchor_height {} exceeds i64::MAX", anchor_height))
        })?;
        let birthday_i64 = i64::try_from(birthday).map_err(|_| {
            crate::Error::Storage(format!("wallet_birthday {} exceeds i64::MAX", birthday))
        })?;
        let mut sapling_shard_indices: HashSet<i64> = HashSet::new();
        let mut orchard_shard_indices: HashSet<i64> = HashSet::new();
        for note in &note_metas {
            let note_height = match u64::try_from(note.height) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if note_height == 0 || note_height < birthday || note_height > anchor_height {
                continue;
            }
            let Some(position_i64) = note.position.filter(|position| *position >= 0) else {
                continue;
            };
            let Some(shard_index) = self.shard_index_from_position(position_i64) else {
                continue;
            };
            match note.note_type {
                crate::models::NoteType::Sapling => {
                    sapling_shard_indices.insert(shard_index);
                }
                crate::models::NoteType::Orchard => {
                    orchard_shard_indices.insert(shard_index);
                }
            }
        }
        let blocked_sapling_shards = self.load_blocked_shard_ranges(
            crate::models::NoteType::Sapling,
            anchor_height_i64,
            birthday_i64,
            &sapling_shard_indices,
        )?;
        let blocked_orchard_shards = self.load_blocked_shard_ranges(
            crate::models::NoteType::Orchard,
            anchor_height_i64,
            birthday_i64,
            &orchard_shard_indices,
        )?;
        type AnchorCandidate = (Vec<u8>, u32, u8, u64, Option<i64>);
        let mut anchor_candidates: Vec<AnchorCandidate> = Vec::new();

        for note in note_metas {
            let note_height = match u64::try_from(note.height) {
                Ok(value) => value,
                Err(_) => continue,
            };
            // Skip unconfirmed notes (height 0). Witnesses/anchors are only meaningful for mined notes.
            if note_height == 0 {
                continue;
            }
            if note_height < birthday || note_height > anchor_height {
                continue;
            }
            result.considered_notes = result.considered_notes.saturating_add(1);

            let missing_note_material = !note.has_note_material;
            let missing_position = note.position.is_none_or(|pos| pos < 0);
            let note_position_i64 = note.position.filter(|position| *position >= 0);
            if !missing_note_material && note_position_i64.is_some() {
                let note_pool = match note.note_type {
                    crate::models::NoteType::Sapling => 0u8,
                    crate::models::NoteType::Orchard => 1u8,
                };
                if let Ok(output_index) = u32::try_from(note.output_index) {
                    anchor_candidates.push((
                        note.txid.clone(),
                        output_index,
                        note_pool,
                        note_height,
                        note_position_i64,
                    ));
                }
            }
            let mut derived_range_count = 0usize;
            if let Some(position_i64) = note_position_i64 {
                if let Some(shard_index) = self.shard_index_from_position(position_i64) {
                    let blocked_ranges = match note.note_type {
                        crate::models::NoteType::Sapling => {
                            blocked_sapling_shards.get(&shard_index)
                        }
                        crate::models::NoteType::Orchard => {
                            blocked_orchard_shards.get(&shard_index)
                        }
                    };
                    if let Some(ranges) = blocked_ranges {
                        for (start_u64, end_u64) in ranges {
                            let range_start = (*start_u64).max(birthday).max(1);
                            let capped_end_exclusive =
                                (*end_u64).min(anchor_height.saturating_add(1));
                            let range_end = capped_end_exclusive.max(range_start.saturating_add(1));
                            pending_ranges.push((range_start, range_end));
                            derived_range_count = derived_range_count.saturating_add(1);
                        }
                    }
                }
            }

            // Queue-first behavior: if this note is blocked by unscanned shard ranges at
            // the fixed anchor, mark it as witness-unavailable and enqueue repair ranges
            // even when note material/position itself is present.
            if derived_range_count > 0 {
                match note.note_type {
                    crate::models::NoteType::Sapling => {
                        result.sapling_missing = result.sapling_missing.saturating_add(1);
                    }
                    crate::models::NoteType::Orchard => {
                        result.orchard_missing = result.orchard_missing.saturating_add(1);
                    }
                }
                continue;
            }

            if missing_note_material || missing_position {
                match note.note_type {
                    crate::models::NoteType::Sapling => {
                        result.sapling_missing = result.sapling_missing.saturating_add(1);
                    }
                    crate::models::NoteType::Orchard => {
                        result.orchard_missing = result.orchard_missing.saturating_add(1);
                    }
                }
                let mut queued_range = false;
                if let Some(position_i64) = note_position_i64 {
                    let shard_table = match note.note_type {
                        crate::models::NoteType::Sapling => "sapling_note_shards",
                        crate::models::NoteType::Orchard => "orchard_note_shards",
                    };
                    if let Some(shard_index) = self.shard_index_from_position(position_i64) {
                        let mut shard_stmt = self.db.conn().prepare(&format!(
                            "SELECT subtree_start_height, subtree_end_height FROM {} WHERE shard_index = ?1",
                            shard_table
                        ))?;
                        if let Some((start_i64, end_i64_opt)) = shard_stmt
                            .query_row(params![shard_index], |row| {
                                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
                            })
                            .optional()?
                        {
                            if let Ok(start_u64) = u64::try_from(start_i64) {
                                let range_start = start_u64.max(birthday).max(1);
                                let end_u64 = end_i64_opt
                                    .and_then(|value| u64::try_from(value).ok())
                                    .unwrap_or(anchor_height);
                                let capped_end = end_u64.min(anchor_height);
                                let range_end = capped_end
                                    .saturating_add(1)
                                    .max(range_start.saturating_add(1));
                                pending_ranges.push((range_start, range_end));
                                queued_range = true;
                            }
                        }
                    }
                }
                // If we can't derive a shard-address-based range (missing/invalid position or
                // absent shard metadata), fall back to a deterministic height-based replay window.
                // Fail-closed behavior: notes missing required spend
                // material must be repaired via re-scan before spending.
                if !queued_range {
                    let range_start = note_height.max(birthday).max(1);
                    let range_end = anchor_height
                        .saturating_add(1)
                        .max(range_start.saturating_add(1));
                    pending_ranges.push((range_start, range_end));
                }
            }
        }

        // Validate anchor witness-construction readiness (not only metadata/scannability).
        //
        // If fixed-anchor candidate notes exist but cannot be hydrated from the
        // current shardtree checkpoint state, queue a deterministic FoundNote replay
        // range so maintenance runs before send-time.
        if !anchor_candidates.is_empty() {
            let anchor_ready = self.get_unspent_selectable_notes_at_anchor_filtered(
                account_id,
                anchor_height,
                10,
                None,
                None,
            )?;
            if anchor_ready.len() < anchor_candidates.len() {
                let mut ready_keys: HashSet<(Vec<u8>, u32, u8)> =
                    HashSet::with_capacity(anchor_ready.len());
                for note in &anchor_ready {
                    let note_pool = match note.note_type {
                        pirate_core::selection::NoteType::Sapling => 0u8,
                        pirate_core::selection::NoteType::Orchard => 1u8,
                    };
                    ready_keys.insert((note.txid.clone(), note.output_index, note_pool));
                }

                let mut missing_sapling = 0usize;
                let mut missing_orchard = 0usize;
                let mut earliest_missing_height = anchor_height;
                let mut derived_range_start: Option<u64> = None;
                let mut derived_range_end: Option<u64> = None;
                for (txid, output_index, note_pool, note_height, note_position_i64) in
                    &anchor_candidates
                {
                    let note_key = (txid.clone(), *output_index, *note_pool);
                    if !ready_keys.contains(&note_key) {
                        earliest_missing_height = earliest_missing_height.min(*note_height);
                        match *note_pool {
                            0 => {
                                missing_sapling = missing_sapling.saturating_add(1);
                            }
                            _ => {
                                missing_orchard = missing_orchard.saturating_add(1);
                            }
                        }
                        if let Some(position_i64) = note_position_i64 {
                            let note_type = match *note_pool {
                                0 => crate::models::NoteType::Sapling,
                                _ => crate::models::NoteType::Orchard,
                            };
                            if let Some((range_start, range_end_exclusive)) =
                                self.tree_shard_height_range_for_position(note_type, *position_i64)?
                            {
                                derived_range_start = Some(
                                    derived_range_start
                                        .map_or(range_start, |current| current.min(range_start)),
                                );
                                derived_range_end = Some(
                                    derived_range_end.map_or(range_end_exclusive, |current| {
                                        current.max(range_end_exclusive)
                                    }),
                                );
                            }
                        }
                    }
                }

                if missing_sapling + missing_orchard > 0 {
                    result.sapling_missing = result.sapling_missing.saturating_add(missing_sapling);
                    result.orchard_missing = result.orchard_missing.saturating_add(missing_orchard);
                    let range_start = derived_range_start
                        .unwrap_or(earliest_missing_height)
                        .max(birthday)
                        .max(1);
                    let range_end = derived_range_end
                        .unwrap_or_else(|| anchor_height.saturating_add(1))
                        .min(anchor_height.saturating_add(1))
                        .max(range_start.saturating_add(1));
                    pending_ranges.push((range_start, range_end));

                    // #region agent log
                    pirate_core::debug_log::with_locked_file(|file| {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_check_witnesses_anchor_state_gap","timestamp":{},"location":"repository.rs:check_witnesses","message":"anchor witness-state gap queued","data":{{"account_id":{},"anchor_height":{},"birthday":{},"candidates":{},"ready":{},"missing_sapling":{},"missing_orchard":{},"range_start":{},"range_end_exclusive":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            ts,
                            account_id,
                            anchor_height,
                            birthday,
                            anchor_candidates.len(),
                            anchor_ready.len(),
                            missing_sapling,
                            missing_orchard,
                            range_start,
                            range_end
                        );
                    });
                    // #endregion
                }
            }
        }

        if pending_ranges.is_empty() {
            result.repair_ranges = Vec::new();
            return Ok(result);
        }

        pending_ranges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut merged_ranges: Vec<(u64, u64)> = Vec::with_capacity(pending_ranges.len());
        for (start, end) in pending_ranges {
            if let Some((_, last_end)) = merged_ranges.last_mut() {
                if start <= *last_end {
                    *last_end = (*last_end).max(end);
                } else {
                    merged_ranges.push((start, end));
                }
            } else {
                merged_ranges.push((start, end));
            }
        }

        result.repair_ranges = merged_ranges;
        Ok(result)
    }

    /// Calculate balance from unspent notes (both Sapling and Orchard)
    ///
    /// Returns (spendable, pending, total) where:
    /// - spendable: Confirmed unspent notes (with minDepth confirmations)
    /// - pending: Unconfirmed unspent notes
    /// - total: spendable + pending
    ///
    /// Includes both Sapling and Orchard notes in the balance calculation.
    pub fn calculate_balance(
        &self,
        account_id: i64,
        current_height: u64,
        min_depth: u64,
    ) -> Result<(u64, u64, u64)> {
        // Get all unspent notes for this account (both Sapling and Orchard)
        let notes = self.get_unspent_notes(account_id)?;

        let mut spendable = 0u64;
        let mut pending = 0u64;

        // Note confirmation depth is defined as the number of blocks since and including
        // the block a note was produced in. This matches the UI confirmation display and
        // wallet summary semantics.
        //
        // confirmations = (current_height - note_height) + 1
        // confirmations >= min_depth  <=>  note_height <= current_height - (min_depth - 1)
        let min_depth = min_depth.max(1);
        let confirmation_threshold = current_height.saturating_sub(min_depth.saturating_sub(1));

        for note in notes {
            let note_height = note.height as u64;
            let note_value = note.value as u64;

            // Note is confirmed if it has at least min_depth confirmations
            // (i.e., note_height <= current_height - (min_depth - 1)).
            // This applies to both Sapling and Orchard notes
            if note_height > 0 && note_height <= confirmation_threshold {
                spendable = spendable
                    .checked_add(note_value)
                    .ok_or_else(|| crate::error::Error::Storage("Balance overflow".to_string()))?;
            } else {
                // Unconfirmed (height = 0 or not enough confirmations yet)
                pending = pending
                    .checked_add(note_value)
                    .ok_or_else(|| crate::error::Error::Storage("Balance overflow".to_string()))?;
            }
        }

        let total = spendable
            .checked_add(pending)
            .ok_or_else(|| crate::error::Error::Storage("Balance overflow".to_string()))?;

        Ok((spendable, pending, total))
    }

    /// Get the current diversifier index for a key group and scope.
    ///
    /// Returns the maximum diversifier index used, or 0 if no addresses exist.
    pub fn get_current_diversifier_index_for_scope(
        &self,
        account_id: i64,
        key_id: i64,
        scope: crate::models::AddressScope,
    ) -> Result<u32> {
        address::get_current_diversifier_index_for_scope(self, account_id, key_id, scope)
    }

    /// Get the current diversifier index for external (receive) addresses.
    pub fn get_current_diversifier_index(&self, account_id: i64, key_id: i64) -> Result<u32> {
        self.get_current_diversifier_index_for_scope(
            account_id,
            key_id,
            crate::models::AddressScope::External,
        )
    }

    /// Get the next diversifier index for a key group and scope.
    ///
    /// Returns the maximum diversifier index used + 1, or 0 if no addresses exist.
    pub fn get_next_diversifier_index_for_scope(
        &self,
        account_id: i64,
        key_id: i64,
        scope: crate::models::AddressScope,
    ) -> Result<u32> {
        let current = self.get_current_diversifier_index_for_scope(account_id, key_id, scope)?;
        Ok(current.saturating_add(1))
    }

    /// Get the next diversifier index for external (receive) addresses.
    pub fn get_next_diversifier_index(&self, account_id: i64, key_id: i64) -> Result<u32> {
        self.get_next_diversifier_index_for_scope(
            account_id,
            key_id,
            crate::models::AddressScope::External,
        )
    }

    /// Backfill missing key_id on legacy addresses (created before key management).
    pub fn backfill_address_key_id(&self, account_id: i64, key_id: i64) -> Result<usize> {
        address::backfill_address_key_id(self, account_id, key_id)
    }

    /// Backfill missing key_id on legacy notes (created before key management).
    pub fn backfill_note_key_id(&self, key_id: i64) -> Result<usize> {
        let encrypted_key_id = self.encrypt_optional_int64(Some(key_id))?;
        let rows = self.db.conn().execute(
            "UPDATE notes SET key_id = ?1 WHERE key_id IS NULL",
            params![encrypted_key_id],
        )?;
        Ok(rows)
    }

    /// Insert or update address with diversifier index
    pub fn upsert_address(&self, address: &Address) -> Result<()> {
        address::upsert_address(self, address)
    }

    /// Get address by address string.
    pub fn get_address_by_string(&self, account_id: i64, address: &str) -> Result<Option<Address>> {
        address::get_address_by_string(self, account_id, address)
    }

    /// Get address by diversifier index for a key group and scope.
    pub fn get_address_by_index_for_scope(
        &self,
        account_id: i64,
        key_id: i64,
        diversifier_index: u32,
        scope: crate::models::AddressScope,
    ) -> Result<Option<Address>> {
        address::get_address_by_index_for_scope(self, account_id, key_id, diversifier_index, scope)
    }

    /// Get external address by diversifier index for a key group.
    pub fn get_address_by_index(
        &self,
        account_id: i64,
        key_id: i64,
        diversifier_index: u32,
    ) -> Result<Option<Address>> {
        self.get_address_by_index_for_scope(
            account_id,
            key_id,
            diversifier_index,
            crate::models::AddressScope::External,
        )
    }

    /// Get all addresses for an account
    pub fn get_all_addresses(&self, account_id: i64) -> Result<Vec<Address>> {
        address::get_all_addresses(self, account_id)
    }

    /// Get all addresses for a key group
    pub fn get_addresses_by_key(&self, account_id: i64, key_id: i64) -> Result<Vec<Address>> {
        address::get_addresses_by_key(self, account_id, key_id)
    }

    /// Update address label
    pub fn update_address_label(
        &self,
        account_id: i64,
        address: &str,
        label: Option<String>,
    ) -> Result<()> {
        address::update_address_label(self, account_id, address, label)
    }

    /// Update address color tag
    pub fn update_address_color_tag(
        &self,
        account_id: i64,
        address: &str,
        color_tag: ColorTag,
    ) -> Result<()> {
        address::update_address_color_tag(self, account_id, address, color_tag)
    }

    /// Get transaction history for an account
    ///
    /// Aggregates notes by transaction to determine send/receive and net amounts.
    /// Returns transactions sorted by height descending (newest first).
    pub fn get_transactions(
        &self,
        account_id: i64,
        limit: Option<u32>,
        current_height: u64,
        min_depth: u64,
    ) -> Result<Vec<TransactionRecord>> {
        self.get_transactions_with_options(account_id, limit, current_height, min_depth, true)
    }

    /// Get transaction history with optional split behavior.
    ///
    /// When `split_transfers` is true, transactions that include both external receives and
    /// internal change outputs can be split into separate send/receive entries.
    pub fn get_transactions_with_options(
        &self,
        account_id: i64,
        limit: Option<u32>,
        _current_height: u64,
        _min_depth: u64,
        split_transfers: bool,
    ) -> Result<Vec<TransactionRecord>> {
        use std::collections::HashMap;

        // Since account_id is encrypted, we need to decrypt all notes and filter
        // This is less efficient but necessary for maximum privacy
        let mut all_notes = self.db.conn().prepare(
            "SELECT account_id, note_type, txid, height, value, spent, spent_txid, output_index, memo, address_id, key_id FROM notes ORDER BY height DESC, id DESC"
        )?;

        type EncryptedNoteRow = (
            Vec<u8>,
            String,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            Option<Vec<u8>>,
            Vec<u8>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        );

        let notes: Vec<EncryptedNoteRow> = all_notes
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,          // encrypted account_id
                    row.get::<_, String>(1)?,           // note_type
                    row.get::<_, Vec<u8>>(2)?,          // encrypted txid
                    row.get::<_, Vec<u8>>(3)?,          // encrypted height
                    row.get::<_, Vec<u8>>(4)?,          // encrypted value
                    row.get::<_, Vec<u8>>(5)?,          // encrypted spent
                    row.get::<_, Option<Vec<u8>>>(6)?,  // encrypted spent_txid
                    row.get::<_, Vec<u8>>(7)?,          // encrypted output_index
                    row.get::<_, Option<Vec<u8>>>(8)?,  // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(9)?,  // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(10)?, // encrypted key_id
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let address_scopes: HashMap<i64, crate::models::AddressScope> = self
            .get_all_addresses(account_id)?
            .into_iter()
            .filter_map(|addr| addr.id.map(|id| (id, addr.address_scope)))
            .collect();

        // Group notes by transaction ID (after decrypting and filtering by account_id)
        struct TxAggregate {
            height: i64,
            received_external: i64,
            received_internal: i64,
            sent: i64,
            memo: Option<Vec<u8>>,
            saw_internal: bool,
            saw_unknown_scope: bool,
        }

        impl TxAggregate {
            fn new(height: i64) -> Self {
                Self {
                    height,
                    received_external: 0,
                    received_internal: 0,
                    sent: 0,
                    memo: None,
                    saw_internal: false,
                    saw_unknown_scope: false,
                }
            }
        }

        let mut tx_map: HashMap<String, TxAggregate> = HashMap::new();
        let mut spent_missing_by_txid: HashMap<String, u64> = HashMap::new();
        let mut note_counts_by_txid: HashMap<String, u64> = HashMap::new();
        let mut spent_target_counts: HashMap<String, u64> = HashMap::new();

        let mut seen: HashMap<(Vec<u8>, i64, crate::models::NoteType), bool> = HashMap::new();
        for (
            enc_account_id,
            note_type_str,
            enc_txid,
            enc_height,
            enc_value,
            enc_spent,
            enc_spent_txid,
            enc_output_index,
            encrypted_memo,
            enc_address_id,
            _enc_key_id,
        ) in notes
        {
            // Decrypt all fields
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;

            // Filter by account_id
            if decrypted_account_id != account_id {
                continue;
            }

            let note_type = match note_type_str.as_str() {
                "Orchard" => crate::models::NoteType::Orchard,
                _ => crate::models::NoteType::Sapling,
            };
            let txid_bytes = self.decrypt_blob(&enc_txid)?;
            let txid_hex = txid_hex_from_bytes(&txid_bytes);
            let output_index = self.decrypt_int64(&enc_output_index)?;
            let height = self.decrypt_int64(&enc_height)?;
            let value = self.decrypt_int64(&enc_value)?;
            if !note_value_is_valid(value) {
                continue;
            }
            let spent = self.decrypt_bool(&enc_spent)?;
            let memo = self
                .decrypt_optional_blob(encrypted_memo)?
                .filter(|m| !memo_bytes_are_effectively_empty(m));
            let spent_txid = self.decrypt_optional_blob(enc_spent_txid)?;
            let address_id = self.decrypt_optional_int64(enc_address_id)?;
            let address_scope = address_id
                .and_then(|id| address_scopes.get(&id).copied())
                .unwrap_or(crate::models::AddressScope::External);

            *note_counts_by_txid.entry(txid_hex.clone()).or_insert(0) += 1;
            if spent && spent_txid.is_none() {
                *spent_missing_by_txid.entry(txid_hex.clone()).or_insert(0) += 1;
            }
            if let Some(spent_txid_bytes) = spent_txid.as_ref() {
                let spent_txid_hex = txid_hex_from_bytes(spent_txid_bytes);
                *spent_target_counts.entry(spent_txid_hex).or_insert(0) += 1;
            }

            let key = (txid_bytes.clone(), output_index, note_type);
            let mut process_incoming = false;
            let mut process_outgoing = false;
            match seen.get(&key) {
                None => {
                    seen.insert(key, spent);
                    process_incoming = true;
                    if spent && spent_txid.is_some() {
                        process_outgoing = true;
                    }
                }
                Some(prev_spent) => {
                    if spent && !*prev_spent {
                        seen.insert(key, true);
                        if spent_txid.is_some() {
                            process_outgoing = true;
                        }
                    } else {
                        continue;
                    }
                }
            }

            if process_incoming {
                let entry = tx_map
                    .entry(txid_hex.clone())
                    .or_insert_with(|| TxAggregate::new(height));

                if height > entry.height {
                    entry.height = height;
                }

                if address_id.is_none() {
                    entry.saw_unknown_scope = true;
                }
                if address_scope == crate::models::AddressScope::Internal {
                    entry.saw_internal = true;
                }

                match address_scope {
                    crate::models::AddressScope::Internal => {
                        entry.received_internal = entry.received_internal.saturating_add(value);
                    }
                    crate::models::AddressScope::External => {
                        entry.received_external = entry.received_external.saturating_add(value);
                    }
                }

                if entry.memo.is_none() && memo.is_some() {
                    entry.memo = memo;
                }
            }

            if process_outgoing {
                let Some(spent_txid_bytes) = spent_txid.as_ref() else {
                    continue;
                };
                let spend_txid_hex = txid_hex_from_bytes(spent_txid_bytes);
                let entry_height = 0;
                let entry = tx_map
                    .entry(spend_txid_hex.clone())
                    .or_insert_with(|| TxAggregate::new(entry_height));

                if entry.height == 0 && entry_height > 0 {
                    entry.height = entry_height;
                }

                entry.sent = entry.sent.saturating_add(value); // total_sent
            }
        }

        let txid_keys: Vec<String> = tx_map.keys().cloned().collect();
        let mut txid_lookup_keys = txid_keys.clone();
        for txid in &txid_keys {
            if let Some(reversed) = reverse_txid_hex(txid) {
                if reversed != *txid {
                    txid_lookup_keys.push(reversed);
                }
            }
        }
        txid_lookup_keys.sort_unstable();
        txid_lookup_keys.dedup();

        let heights_map = self.get_transaction_heights(&txid_lookup_keys)?;
        let ts_map = self.get_transaction_timestamps(&txid_lookup_keys)?;
        let memo_map = self.get_transaction_memos(&txid_lookup_keys)?;
        let fee_map = self.get_transaction_fees(&txid_lookup_keys)?;
        struct TxDebugRow {
            txid: String,
            height: i64,
            received_external: i64,
            received_internal: i64,
            sent: i64,
            total_received: i64,
            net_amount: i64,
            outgoing_amount: i64,
            fee: i64,
            saw_internal: bool,
            saw_unknown_scope: bool,
            can_split: bool,
            self_transfer: bool,
            spent_missing: u64,
            note_count: u64,
            spent_targets: u64,
        }

        let mut debug_records: Vec<TxDebugRow> = Vec::new();

        for (txid, entry) in tx_map.iter_mut() {
            let alt_txid = reverse_txid_hex(txid);
            let direct_height = heights_map.get(txid).copied().unwrap_or(0);
            let alt_height = alt_txid
                .as_ref()
                .and_then(|alt| heights_map.get(alt).copied())
                .unwrap_or(0);
            let best_height = direct_height.max(alt_height);
            if best_height > 0 {
                entry.height = best_height;
            }
        }

        // Convert to TransactionRecord and calculate net amount
        let mut transactions: Vec<TransactionRecord> = Vec::new();
        for (txid, entry) in tx_map.into_iter() {
            let alt_txid = reverse_txid_hex(&txid);
            let direct_height = heights_map.get(&txid).copied().unwrap_or(0);
            let alt_height = alt_txid
                .as_ref()
                .and_then(|alt| heights_map.get(alt).copied())
                .unwrap_or(0);
            // Net amount: positive for receive, negative for send
            let total_received = entry
                .received_external
                .saturating_add(entry.received_internal);
            let net_amount = total_received.saturating_sub(entry.sent);
            let memo = entry
                .memo
                .or_else(|| {
                    memo_map.get(&txid).cloned().or_else(|| {
                        reverse_txid_hex(&txid).and_then(|alt| memo_map.get(&alt).cloned())
                    })
                })
                .filter(|m| !memo_bytes_are_effectively_empty(m));

            // Use stored transaction timestamp if available (first confirmation time).
            // Fallback: current time (unconfirmed or not yet populated).
            let direct_timestamp = ts_map.get(&txid).copied();
            let alt_timestamp = alt_txid.as_ref().and_then(|alt| ts_map.get(alt).copied());
            let timestamp = if alt_height > direct_height {
                alt_timestamp.or(direct_timestamp)
            } else {
                direct_timestamp.or(alt_timestamp)
            }
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

            let direct_fee = fee_map.get(&txid).copied().unwrap_or(0);
            let alt_fee = alt_txid
                .as_ref()
                .and_then(|alt| fee_map.get(alt).copied())
                .unwrap_or(0);
            let stored_fee = direct_fee.max(alt_fee);
            let fee = if stored_fee == 0 && entry.sent > 0 && net_amount < 0 {
                DEFAULT_FEE
            } else {
                stored_fee
            };
            let fee_i64 = i64::try_from(fee).unwrap_or(i64::MAX);
            let outgoing_amount = entry
                .sent
                .saturating_sub(entry.received_internal)
                .saturating_sub(fee_i64);

            let can_split = split_transfers
                && entry.received_external > 0
                && outgoing_amount > 0
                && entry.saw_internal
                && !entry.saw_unknown_scope;
            let self_transfer = split_transfers
                && entry.sent > 0
                && entry.received_external > 0
                && entry.received_internal == 0
                && !entry.saw_unknown_scope
                && net_amount.abs() <= 20_000;
            if debug_records.len() < 50 {
                let spent_missing = spent_missing_by_txid.get(&txid).copied().unwrap_or(0);
                let note_count = note_counts_by_txid.get(&txid).copied().unwrap_or(0);
                let spent_targets = spent_target_counts.get(&txid).copied().unwrap_or(0);
                if spent_missing > 0
                    || entry.sent > 0
                    || entry.received_internal > 0
                    || entry.received_external > 0
                {
                    debug_records.push(TxDebugRow {
                        txid: txid.clone(),
                        height: entry.height,
                        received_external: entry.received_external,
                        received_internal: entry.received_internal,
                        sent: entry.sent,
                        total_received,
                        net_amount,
                        outgoing_amount,
                        fee: fee_i64,
                        saw_internal: entry.saw_internal,
                        saw_unknown_scope: entry.saw_unknown_scope,
                        can_split,
                        self_transfer,
                        spent_missing,
                        note_count,
                        spent_targets,
                    });
                }
            }
            if can_split {
                transactions.push(TransactionRecord {
                    txid: txid.clone(),
                    height: entry.height,
                    timestamp,
                    amount: -outgoing_amount,
                    fee,
                    memo: memo.clone(),
                });
                transactions.push(TransactionRecord {
                    txid,
                    height: entry.height,
                    timestamp,
                    amount: entry.received_external,
                    fee: 0,
                    memo,
                });
            } else if self_transfer {
                let transfer_amount = entry.received_external;
                transactions.push(TransactionRecord {
                    txid: txid.clone(),
                    height: entry.height,
                    timestamp,
                    amount: -transfer_amount,
                    fee,
                    memo: memo.clone(),
                });
                transactions.push(TransactionRecord {
                    txid,
                    height: entry.height,
                    timestamp,
                    amount: transfer_amount,
                    fee: 0,
                    memo,
                });
            } else {
                transactions.push(TransactionRecord {
                    txid,
                    height: entry.height,
                    timestamp,
                    amount: net_amount,
                    fee,
                    memo,
                });
            }
        }

        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"repository.rs:get_transactions","message":"tx_aggregate summary","data":{{"tx_count":{},"log_count":{},"split_transfers":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                transactions.len(),
                debug_records.len(),
                split_transfers
            );
            for row in debug_records {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_{}","timestamp":{},"location":"repository.rs:get_transactions","message":"tx_aggregate entry","data":{{"txid":"{}","height":{},"received_external":{},"received_internal":{},"sent":{},"total_received":{},"net_amount":{},"outgoing_amount":{},"fee":{},"saw_internal":{},"saw_unknown_scope":{},"can_split":{},"self_transfer":{},"spent_missing_txid":{},"note_count":{},"spent_target_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    id,
                    ts,
                    row.txid,
                    row.height,
                    row.received_external,
                    row.received_internal,
                    row.sent,
                    row.total_received,
                    row.net_amount,
                    row.outgoing_amount,
                    row.fee,
                    row.saw_internal,
                    row.saw_unknown_scope,
                    row.can_split,
                    row.self_transfer,
                    row.spent_missing,
                    row.note_count,
                    row.spent_targets
                );
            }
        });

        // Sort by height descending (newest first), then by txid and amount
        transactions.sort_by(|a, b| {
            b.height
                .cmp(&a.height)
                .then_with(|| b.txid.cmp(&a.txid))
                .then_with(|| b.amount.cmp(&a.amount))
        });

        // Apply limit
        if let Some(limit) = limit {
            transactions.truncate(limit as usize);
        }

        Ok(transactions)
    }

    /// Get Orchard note references for validation.
    /// Note: Since fields are encrypted, we decrypt and filter in memory.
    pub fn get_orchard_note_refs(&self, account_id: i64) -> Result<Vec<OrchardNoteRef>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT account_id, note_type, txid, output_index, commitment, height FROM notes",
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?, // encrypted account_id
                    row.get::<_, String>(1)?,  // note_type
                    row.get::<_, Vec<u8>>(2)?, // encrypted txid
                    row.get::<_, Vec<u8>>(3)?, // encrypted output_index
                    row.get::<_, Vec<u8>>(4)?, // encrypted commitment
                    row.get::<_, Vec<u8>>(5)?, // encrypted height
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut refs = Vec::new();
        for (
            enc_account_id,
            note_type_str,
            enc_txid,
            enc_output_index,
            enc_commitment,
            enc_height,
        ) in rows
        {
            if note_type_str.as_str() != "Orchard" {
                continue;
            }
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }
            let txid = self.decrypt_blob(&enc_txid)?;
            let output_index = self.decrypt_int64(&enc_output_index)?;
            let commitment_bytes = self.decrypt_blob(&enc_commitment)?;
            if commitment_bytes.len() != 32 {
                continue;
            }
            let mut commitment = [0u8; 32];
            commitment.copy_from_slice(&commitment_bytes[..32]);
            let height = self.decrypt_int64(&enc_height)?;

            refs.push(OrchardNoteRef {
                txid,
                output_index,
                commitment,
                height,
            });
        }

        Ok(refs)
    }

    /// Get note by transaction ID and output index with decrypted fields.
    ///
    /// If `note_type_filter` is provided, only rows in that pool are considered.
    /// This prevents Sapling/Orchard collisions on shared (txid, output_index) pairs.
    /// Note: Since all fields are encrypted, we decrypt rows and filter in memory.
    pub fn get_note_by_txid_and_index_with_type(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
        note_type_filter: Option<crate::models::NoteType>,
    ) -> Result<Option<NoteRecord>> {
        // Since fields are encrypted, we need to decrypt all and filter
        // For efficiency with large datasets, this could be optimized with an index table
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id FROM notes",
        )?;

        let notes = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get::<_, String>(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling,
                };

                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    note_type,
                    row.get::<_, Vec<u8>>(3)?,          // encrypted value
                    row.get::<_, Vec<u8>>(4)?,          // encrypted nullifier
                    row.get::<_, Vec<u8>>(5)?,          // encrypted commitment
                    row.get::<_, Vec<u8>>(6)?,          // encrypted spent
                    row.get::<_, Vec<u8>>(7)?,          // encrypted height
                    row.get::<_, Vec<u8>>(8)?,          // encrypted txid
                    row.get::<_, Vec<u8>>(9)?,          // encrypted output_index
                    row.get::<_, Option<Vec<u8>>>(10)?, // encrypted spent_txid
                    row.get::<_, Option<Vec<u8>>>(11)?, // encrypted diversifier
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted key_id
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Decrypt and filter in memory. If duplicates exist, choose latest row id.
        let mut best_match: Option<NoteRecord> = None;
        for (
            id,
            enc_account_id,
            note_type,
            enc_value,
            enc_nullifier,
            enc_commitment,
            enc_spent,
            enc_height,
            enc_txid,
            enc_output_index,
            enc_spent_txid,
            enc_diversifier,
            enc_note,
            enc_position,
            enc_memo,
            enc_address_id,
            enc_key_id,
        ) in notes
        {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            let decrypted_txid = self.decrypt_blob(&enc_txid)?;
            let decrypted_output_index = self.decrypt_int64(&enc_output_index)?;

            // Filter by search criteria
            if decrypted_account_id != account_id
                || decrypted_txid != txid
                || decrypted_output_index != output_index
            {
                continue;
            }
            if note_type_filter
                .as_ref()
                .is_some_and(|filter| filter != &note_type)
            {
                continue;
            }

            let candidate = NoteRecord {
                id: Some(id),
                account_id: decrypted_account_id,
                key_id: self.decrypt_optional_int64(enc_key_id)?,
                note_type,
                value: self.decrypt_int64(&enc_value)?,
                nullifier: self.decrypt_blob(&enc_nullifier)?,
                commitment: self.decrypt_blob(&enc_commitment)?,
                spent: self.decrypt_bool(&enc_spent)?,
                height: self.decrypt_int64(&enc_height)?,
                txid: decrypted_txid,
                output_index: decrypted_output_index,
                address_id: self.decrypt_optional_int64(enc_address_id)?,
                spent_txid: self.decrypt_optional_blob(enc_spent_txid)?,
                diversifier: self.decrypt_optional_blob(enc_diversifier)?,
                note: self.decrypt_optional_blob(enc_note)?,
                position: self.decrypt_optional_int64(enc_position)?,
                memo: self.decrypt_optional_blob(enc_memo)?,
            };
            let candidate_id = candidate.id.unwrap_or_default();
            let replace = best_match
                .as_ref()
                .map(|existing| candidate_id > existing.id.unwrap_or_default())
                .unwrap_or(true);
            if replace {
                best_match = Some(candidate);
            }
        }

        Ok(best_match)
    }

    /// Backward-compatible helper that searches by txid/output across all pools.
    pub fn get_note_by_txid_and_index(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
    ) -> Result<Option<NoteRecord>> {
        self.get_note_by_txid_and_index_with_type(account_id, txid, output_index, None)
    }

    /// Update memo for a note (encrypts before storage)
    /// Note: Since all fields are encrypted, we need to decrypt all notes and filter in memory
    pub fn update_note_memo_with_type(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
        note_type_filter: Option<crate::models::NoteType>,
        memo: Option<&[u8]>,
    ) -> Result<()> {
        // Encrypt memo for storage
        let encrypted_memo = self.encrypt_optional_blob(memo)?;

        // Since fields are encrypted, we need to find the note by decrypting all.
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, note_type, txid, output_index FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get::<_, String>(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling,
                };
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    note_type,
                    row.get::<_, Vec<u8>>(3)?, // encrypted txid
                    row.get::<_, Vec<u8>>(4)?, // encrypted output_index
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut best_id: Option<i64> = None;
        for (id, enc_acc_id, note_type, enc_tx, enc_out_idx) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_acc_id)?;
            if decrypted_account_id != account_id {
                continue;
            }
            let decrypted_txid = self.decrypt_blob(&enc_tx)?;
            let decrypted_output_index = self.decrypt_int64(&enc_out_idx)?;
            if decrypted_txid != txid || decrypted_output_index != output_index {
                continue;
            }
            if note_type_filter
                .as_ref()
                .is_some_and(|filter| filter != &note_type)
            {
                continue;
            }
            if best_id.map(|current| id > current).unwrap_or(true) {
                best_id = Some(id);
            }
        }

        if let Some(id) = best_id {
            self.db.conn().execute(
                "UPDATE notes SET memo = ?1 WHERE id = ?2",
                params![encrypted_memo, id],
            )?;
        }
        Ok(())
    }

    /// Update memo across all pools (legacy behavior).
    pub fn update_note_memo(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
        memo: Option<&[u8]>,
    ) -> Result<()> {
        self.update_note_memo_with_type(account_id, txid, output_index, None, memo)
    }
    /// Delete a note by transaction ID and output index (with encrypted fields).
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory for privacy.
    pub fn delete_note_by_txid_and_index(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
    ) -> Result<usize> {
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, txid, output_index FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, Vec<u8>>(2)?, // encrypted txid
                    row.get::<_, Vec<u8>>(3)?, // encrypted output_index
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut deleted = 0usize;
        for (id, enc_acc_id, enc_tx, enc_out_idx) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_acc_id)?;
            let decrypted_txid = self.decrypt_blob(&enc_tx)?;
            let decrypted_output_index = self.decrypt_int64(&enc_out_idx)?;

            if decrypted_account_id == account_id
                && decrypted_txid == txid
                && decrypted_output_index == output_index
            {
                self.db
                    .conn()
                    .execute("DELETE FROM notes WHERE id = ?1", params![id])?;
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// Mark a note as spent by nullifier.
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory.
    pub fn mark_note_spent_by_nullifier(&self, account_id: i64, nullifier: &[u8]) -> Result<bool> {
        let encrypted_spent = self.encrypt_bool(true)?;

        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, nullifier, spent FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?, // encrypted spent
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut updated = false;
        for (id, enc_account_id, enc_nullifier, enc_spent) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let decrypted_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if decrypted_nullifier != nullifier {
                continue;
            }

            let spent = self.decrypt_bool(&enc_spent)?;
            if spent {
                return Ok(true);
            }

            self.db.conn().execute(
                "UPDATE notes SET spent = ?1 WHERE id = ?2",
                params![encrypted_spent.clone(), id],
            )?;
            updated = true;
        }

        Ok(updated)
    }

    /// Mark a note as spent by nullifier and record the spending txid.
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory.
    pub fn mark_note_spent_by_nullifier_with_txid(
        &self,
        account_id: i64,
        nullifier: &[u8],
        spent_txid: &[u8],
    ) -> Result<bool> {
        let encrypted_spent = self.encrypt_bool(true)?;
        let encrypted_spent_txid = self.encrypt_blob(spent_txid)?;

        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, nullifier, spent FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?, // encrypted spent
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut updated = false;
        for (id, enc_account_id, enc_nullifier, enc_spent) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let decrypted_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if decrypted_nullifier != nullifier {
                continue;
            }

            let spent = self.decrypt_bool(&enc_spent)?;
            if spent {
                self.db.conn().execute(
                    "UPDATE notes SET spent_txid = ?1 WHERE id = ?2",
                    params![encrypted_spent_txid.clone(), id],
                )?;
                updated = true;
                continue;
            }

            self.db.conn().execute(
                "UPDATE notes SET spent = ?1, spent_txid = ?2 WHERE id = ?3",
                params![encrypted_spent.clone(), encrypted_spent_txid.clone(), id],
            )?;
            updated = true;
        }

        Ok(updated)
    }

    /// Persist spends that could not yet be linked to known notes.
    ///
    /// This follows an "unlinked nullifier map" concept:
    /// when a spend nullifier is observed before the corresponding note exists
    /// locally, we store it and reconcile later when the note arrives.
    pub fn upsert_unlinked_spend_nullifiers_with_txid(
        &self,
        account_id: i64,
        entries: &[(NoteType, [u8; 32], [u8; 32])],
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }

        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, nullifier, spending_txid
             FROM unlinked_spend_nullifiers",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, String>(2)?,  // note_type
                    row.get::<_, Vec<u8>>(3)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(4)?, // encrypted spending_txid
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut existing: HashMap<(NoteType, [u8; 32]), (i64, [u8; 32])> = HashMap::new();
        let mut duplicate_ids: Vec<i64> = Vec::new();

        for (id, enc_account_id, note_type_str, enc_nullifier, enc_spending_txid) in rows {
            let row_account_id = self.decrypt_int64(&enc_account_id)?;
            if row_account_id != account_id {
                continue;
            }

            let row_note_type = match note_type_str.as_str() {
                "Orchard" => NoteType::Orchard,
                _ => NoteType::Sapling,
            };

            let row_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if row_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&row_nullifier[..32]);

            let row_spending_txid = self.decrypt_blob(&enc_spending_txid)?;
            if row_spending_txid.len() != 32 {
                continue;
            }
            let mut txid = [0u8; 32];
            txid.copy_from_slice(&row_spending_txid[..32]);

            let key = (row_note_type, nf);
            if let std::collections::hash_map::Entry::Vacant(slot) = existing.entry(key) {
                slot.insert((id, txid));
            } else {
                duplicate_ids.push(id);
            }
        }

        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;

        let mut changed = 0u64;
        let now = chrono::Utc::now().timestamp();
        for duplicate_id in duplicate_ids {
            if let Err(e) = conn.execute(
                "DELETE FROM unlinked_spend_nullifiers WHERE id = ?1",
                params![duplicate_id],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
            changed += 1;
        }

        let encrypted_account_id = self.encrypt_int64(account_id)?;
        for (note_type, nullifier, spending_txid) in entries {
            if nullifier.iter().all(|b| *b == 0) {
                continue;
            }
            let key = (*note_type, *nullifier);
            if let Some((row_id, current_txid)) = existing.get(&key).copied() {
                if current_txid == *spending_txid {
                    continue;
                }
                let encrypted_spending_txid = self.encrypt_blob(spending_txid)?;
                if let Err(e) = conn.execute(
                    "UPDATE unlinked_spend_nullifiers
                     SET spending_txid = ?1, updated_at = ?2
                     WHERE id = ?3",
                    params![encrypted_spending_txid, now, row_id],
                ) {
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(e.into());
                }
                changed += 1;
                existing.insert(key, (row_id, *spending_txid));
                continue;
            }

            let note_type_str = match note_type {
                NoteType::Sapling => "Sapling",
                NoteType::Orchard => "Orchard",
            };
            let encrypted_nullifier = self.encrypt_blob(nullifier)?;
            let encrypted_spending_txid = self.encrypt_blob(spending_txid)?;
            if let Err(e) = conn.execute(
                "INSERT INTO unlinked_spend_nullifiers
                 (account_id, note_type, nullifier, spending_txid, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    encrypted_account_id.clone(),
                    note_type_str,
                    encrypted_nullifier,
                    encrypted_spending_txid,
                    now,
                    now
                ],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
            changed += 1;
        }

        conn.execute_batch("COMMIT;")?;
        Ok(changed)
    }

    /// List persisted unlinked spend nullifiers for an account.
    ///
    /// The returned list is deterministic (sorted + deduplicated), which allows
    /// sync reconciliation passes to behave consistently across rescans/devices.
    pub fn list_unlinked_spend_nullifiers_with_txid(
        &self,
        account_id: i64,
    ) -> Result<Vec<TypedUnlinkedSpend>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT account_id, note_type, nullifier, spending_txid
             FROM unlinked_spend_nullifiers",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?, // encrypted account_id
                    row.get::<_, String>(1)?,  // note_type
                    row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?, // encrypted spending_txid
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: Vec<TypedUnlinkedSpend> = Vec::new();
        for (enc_account_id, note_type_str, enc_nullifier, enc_spending_txid) in rows {
            let row_account_id = self.decrypt_int64(&enc_account_id)?;
            if row_account_id != account_id {
                continue;
            }

            let note_type = match note_type_str.as_str() {
                "Orchard" => NoteType::Orchard,
                _ => NoteType::Sapling,
            };

            let row_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if row_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&row_nullifier[..32]);
            if nf.iter().all(|b| *b == 0) {
                continue;
            }

            let row_spending_txid = self.decrypt_blob(&enc_spending_txid)?;
            if row_spending_txid.len() != 32 {
                continue;
            }
            let mut txid = [0u8; 32];
            txid.copy_from_slice(&row_spending_txid[..32]);

            out.push((note_type, nf, txid));
        }

        out.sort_by(|a, b| {
            let a_ty = match a.0 {
                NoteType::Sapling => 0u8,
                NoteType::Orchard => 1u8,
            };
            let b_ty = match b.0 {
                NoteType::Sapling => 0u8,
                NoteType::Orchard => 1u8,
            };
            a_ty.cmp(&b_ty)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        out.dedup();
        Ok(out)
    }

    /// Deterministically reconcile persisted unlinked spends against local notes.
    ///
    /// Returns `(updated_notes, deleted_unlinked_rows)`.
    pub fn reconcile_unlinked_spend_nullifiers_with_notes(
        &self,
        account_id: i64,
    ) -> Result<(u64, u64)> {
        let entries = self.list_unlinked_spend_nullifiers_with_txid(account_id)?;
        if entries.is_empty() {
            return Ok((0, 0));
        }

        let mark_entries: Vec<([u8; 32], [u8; 32])> =
            entries.iter().map(|(_, nf, txid)| (*nf, *txid)).collect();
        let updated_notes =
            self.mark_notes_spent_by_nullifiers_with_txid(account_id, &mark_entries)?;

        let mut expected: TypedUnlinkedSpendMap = HashMap::new();
        for (note_type, nf, txid) in &entries {
            expected.insert((*note_type, *nf), *txid);
        }

        let mut note_stmt = self
            .db
            .conn()
            .prepare("SELECT account_id, note_type, nullifier, spent, spent_txid FROM notes")?;
        let note_rows = note_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,         // encrypted account_id
                    row.get::<_, String>(1)?,          // note_type
                    row.get::<_, Vec<u8>>(2)?,         // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?,         // encrypted spent
                    row.get::<_, Option<Vec<u8>>>(4)?, // encrypted spent_txid
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut resolved: HashSet<TypedNullifier> = HashSet::new();
        for (enc_account_id, note_type_str, enc_nullifier, enc_spent, enc_spent_txid) in note_rows {
            let row_account_id = self.decrypt_int64(&enc_account_id)?;
            if row_account_id != account_id {
                continue;
            }

            let note_type = match note_type_str.as_str() {
                "Orchard" => NoteType::Orchard,
                _ => NoteType::Sapling,
            };

            let row_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if row_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&row_nullifier[..32]);

            let expected_txid = match expected.get(&(note_type, nf)) {
                Some(txid) => txid,
                None => continue,
            };

            let spent = self.decrypt_bool(&enc_spent)?;
            if !spent {
                continue;
            }

            let row_spent_txid = match self.decrypt_optional_blob(enc_spent_txid)? {
                Some(bytes) if bytes.len() == 32 => bytes,
                _ => continue,
            };
            if row_spent_txid.as_slice() == expected_txid {
                resolved.insert((note_type, nf));
            }
        }

        if resolved.is_empty() {
            return Ok((updated_notes, 0));
        }

        let mut unlinked_stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, nullifier
             FROM unlinked_spend_nullifiers",
        )?;
        let unlinked_rows = unlinked_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, String>(2)?,  // note_type
                    row.get::<_, Vec<u8>>(3)?, // encrypted nullifier
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut deleted = 0u64;
        for (id, enc_account_id, note_type_str, enc_nullifier) in unlinked_rows {
            let row_account_id = self.decrypt_int64(&enc_account_id)?;
            if row_account_id != account_id {
                continue;
            }

            let note_type = match note_type_str.as_str() {
                "Orchard" => NoteType::Orchard,
                _ => NoteType::Sapling,
            };

            let row_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if row_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&row_nullifier[..32]);
            if !resolved.contains(&(note_type, nf)) {
                continue;
            }

            if let Err(e) = conn.execute(
                "DELETE FROM unlinked_spend_nullifiers WHERE id = ?1",
                params![id],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
            deleted += 1;
        }
        conn.execute_batch("COMMIT;")?;
        Ok((updated_notes, deleted))
    }

    /// Consume (read + delete) previously stored unlinked spend nullifiers in one pass.
    ///
    /// This is used by note persistence to avoid per-note full-table scans.
    pub fn consume_unlinked_spends_for_nullifiers(
        &self,
        account_id: i64,
        nullifiers: &[TypedNullifier],
    ) -> Result<TypedUnlinkedSpendMap> {
        if nullifiers.is_empty() {
            return Ok(TypedUnlinkedSpendMap::new());
        }

        let wanted: HashSet<TypedNullifier> = nullifiers
            .iter()
            .copied()
            .filter(|(_, nf)| !nf.iter().all(|b| *b == 0))
            .collect();
        if wanted.is_empty() {
            return Ok(TypedUnlinkedSpendMap::new());
        }

        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, nullifier, spending_txid
             FROM unlinked_spend_nullifiers",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, String>(2)?,  // note_type
                    row.get::<_, Vec<u8>>(3)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(4)?, // encrypted spending_txid
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut matching_ids: Vec<i64> = Vec::new();
        let mut matched: TypedUnlinkedSpendMap = HashMap::new();

        for (id, enc_account_id, note_type_str, enc_nullifier, enc_spending_txid) in rows {
            let row_account_id = self.decrypt_int64(&enc_account_id)?;
            if row_account_id != account_id {
                continue;
            }

            let row_note_type = match note_type_str.as_str() {
                "Orchard" => NoteType::Orchard,
                _ => NoteType::Sapling,
            };
            let row_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if row_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&row_nullifier[..32]);
            if !wanted.contains(&(row_note_type, nf)) {
                continue;
            }

            let row_spending_txid = self.decrypt_blob(&enc_spending_txid)?;
            if row_spending_txid.len() != 32 {
                continue;
            }

            let mut txid = [0u8; 32];
            txid.copy_from_slice(&row_spending_txid[..32]);
            matched.entry((row_note_type, nf)).or_insert(txid);
            matching_ids.push(id);
        }

        if matching_ids.is_empty() {
            return Ok(matched);
        }

        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        for id in matching_ids {
            if let Err(e) = conn.execute(
                "DELETE FROM unlinked_spend_nullifiers WHERE id = ?1",
                params![id],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
        }
        conn.execute_batch("COMMIT;")?;

        Ok(matched)
    }

    /// Consume (read + delete) a previously stored unlinked spend nullifier.
    pub fn consume_unlinked_spend_for_nullifier(
        &self,
        account_id: i64,
        note_type: NoteType,
        nullifier: &[u8; 32],
    ) -> Result<Option<[u8; 32]>> {
        let entries = [(note_type, *nullifier)];
        let matched = self.consume_unlinked_spends_for_nullifiers(account_id, &entries)?;
        Ok(matched.get(&(note_type, *nullifier)).copied())
    }

    /// Mark notes as spent for any nullifier in the provided set.
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory.
    pub fn mark_notes_spent_by_nullifiers(
        &self,
        account_id: i64,
        nullifiers: &std::collections::HashSet<[u8; 32]>,
    ) -> Result<u64> {
        if nullifiers.is_empty() {
            return Ok(0);
        }

        let encrypted_spent = self.encrypt_bool(true)?;

        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, nullifier, spent FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?, // encrypted spent
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut updated_count = 0u64;
        for (id, enc_account_id, enc_nullifier, enc_spent) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let decrypted_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if decrypted_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&decrypted_nullifier[..32]);
            if !nullifiers.contains(&nf) {
                continue;
            }

            let spent = self.decrypt_bool(&enc_spent)?;
            if spent {
                continue;
            }

            self.db.conn().execute(
                "UPDATE notes SET spent = ?1 WHERE id = ?2",
                params![encrypted_spent.clone(), id],
            )?;
            updated_count += 1;
        }

        Ok(updated_count)
    }

    /// Mark notes as spent for any nullifier in the provided set and record the spending txid.
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory.
    pub fn mark_notes_spent_by_nullifiers_with_txid(
        &self,
        account_id: i64,
        entries: &[([u8; 32], [u8; 32])],
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }

        let mut spend_map: std::collections::HashMap<[u8; 32], [u8; 32]> =
            std::collections::HashMap::with_capacity(entries.len());
        for (nullifier, txid) in entries {
            spend_map.insert(*nullifier, *txid);
        }

        let encrypted_spent = self.encrypt_bool(true)?;

        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, account_id, nullifier, spent FROM notes")?;

        let notes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                    row.get::<_, Vec<u8>>(3)?, // encrypted spent
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut updated = 0u64;
        for (id, enc_account_id, enc_nullifier, enc_spent) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            if decrypted_account_id != account_id {
                continue;
            }

            let decrypted_nullifier = self.decrypt_blob(&enc_nullifier)?;
            if decrypted_nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&decrypted_nullifier[..32]);
            let spent_txid = match spend_map.get(&nf) {
                Some(txid) => txid,
                None => continue,
            };

            let spent = self.decrypt_bool(&enc_spent)?;
            let encrypted_spent_txid = self.encrypt_blob(spent_txid)?;
            if spent {
                if let Err(e) = conn.execute(
                    "UPDATE notes SET spent_txid = ?1 WHERE id = ?2",
                    params![encrypted_spent_txid, id],
                ) {
                    let _ = conn.execute_batch("ROLLBACK;");
                    return Err(e.into());
                }
            } else if let Err(e) = conn.execute(
                "UPDATE notes SET spent = ?1, spent_txid = ?2 WHERE id = ?3",
                params![encrypted_spent, encrypted_spent_txid, id],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
            updated += 1;
        }
        conn.execute_batch("COMMIT;")?;

        Ok(updated)
    }

    /// Mark notes as spent by row id and record the spending txid.
    pub fn mark_notes_spent_by_ids_with_txid(&self, entries: &[(i64, [u8; 32])]) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }

        let encrypted_spent = self.encrypt_bool(true)?;
        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut updated = 0u64;
        for (id, spent_txid) in entries {
            let encrypted_spent_txid = self.encrypt_blob(spent_txid)?;
            if let Err(e) = conn.execute(
                "UPDATE notes SET spent = ?1, spent_txid = ?2 WHERE id = ?3",
                params![encrypted_spent, encrypted_spent_txid, id],
            ) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e.into());
            }
            updated += 1;
        }
        conn.execute_batch("COMMIT;")?;
        Ok(updated)
    }

    /// Apply spend updates in a single DB transaction for sync hot path.
    ///
    /// This combines:
    /// - direct note-id spend marks
    /// - fallback nullifier-based spend marks
    /// - transaction metadata upserts for matched spending txids
    ///
    /// Returns `(updated_by_id, updated_by_nullifier)`.
    pub fn apply_spend_updates_with_txmeta(
        &self,
        account_id: i64,
        spend_updates: &[(i64, [u8; 32])],
        fallback_entries: &[([u8; 32], [u8; 32])],
        tx_meta: &[(String, i64, i64, i64)],
    ) -> Result<(u64, u64)> {
        if spend_updates.is_empty() && fallback_entries.is_empty() && tx_meta.is_empty() {
            return Ok((0, 0));
        }

        let encrypted_spent = self.encrypt_bool(true)?;
        let conn = self.db.conn();
        conn.execute_batch("BEGIN IMMEDIATE;")?;

        let result = (|| -> Result<(u64, u64)> {
            let mut updated_by_id = 0u64;
            if !spend_updates.is_empty() {
                let mut update_stmt =
                    conn.prepare("UPDATE notes SET spent = ?1, spent_txid = ?2 WHERE id = ?3")?;
                for (id, spent_txid) in spend_updates {
                    let encrypted_spent_txid = self.encrypt_blob(spent_txid)?;
                    let rows =
                        update_stmt.execute(params![&encrypted_spent, encrypted_spent_txid, id])?;
                    updated_by_id += rows as u64;
                }
            }

            let mut updated_by_nullifier = 0u64;
            if !fallback_entries.is_empty() {
                let mut spend_map: HashMap<[u8; 32], [u8; 32]> =
                    HashMap::with_capacity(fallback_entries.len());
                for (nullifier, txid) in fallback_entries {
                    spend_map.insert(*nullifier, *txid);
                }

                let mut stmt =
                    conn.prepare("SELECT id, account_id, nullifier, spent FROM notes")?;
                let notes = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,     // id
                            row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                            row.get::<_, Vec<u8>>(2)?, // encrypted nullifier
                            row.get::<_, Vec<u8>>(3)?, // encrypted spent
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                drop(stmt);

                let mut update_spent_txid_stmt =
                    conn.prepare("UPDATE notes SET spent_txid = ?1 WHERE id = ?2")?;
                let mut update_spent_with_txid_stmt =
                    conn.prepare("UPDATE notes SET spent = ?1, spent_txid = ?2 WHERE id = ?3")?;
                for (id, enc_account_id, enc_nullifier, enc_spent) in notes {
                    let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
                    if decrypted_account_id != account_id {
                        continue;
                    }

                    let decrypted_nullifier = self.decrypt_blob(&enc_nullifier)?;
                    if decrypted_nullifier.len() != 32 {
                        continue;
                    }
                    let mut nf = [0u8; 32];
                    nf.copy_from_slice(&decrypted_nullifier[..32]);
                    let spent_txid = match spend_map.get(&nf) {
                        Some(txid) => txid,
                        None => continue,
                    };

                    let spent = self.decrypt_bool(&enc_spent)?;
                    let encrypted_spent_txid = self.encrypt_blob(spent_txid)?;
                    let rows = if spent {
                        update_spent_txid_stmt.execute(params![encrypted_spent_txid, id])?
                    } else {
                        update_spent_with_txid_stmt.execute(params![
                            &encrypted_spent,
                            encrypted_spent_txid,
                            id
                        ])?
                    };
                    updated_by_nullifier += rows as u64;
                }
            }

            if !tx_meta.is_empty() {
                let mut tx_stmt = conn.prepare(
                    "INSERT INTO transactions (txid, height, timestamp, fee)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(txid) DO UPDATE SET
                       height=excluded.height,
                       timestamp=excluded.timestamp,
                       fee=excluded.fee",
                )?;
                for (txid_hex, height, timestamp, fee) in tx_meta {
                    tx_stmt.execute(params![txid_hex, height, timestamp, fee])?;
                }
            }

            Ok((updated_by_id, updated_by_nullifier))
        })();

        match result {
            Ok(v) => {
                conn.execute_batch("COMMIT;")?;
                Ok(v)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Get all notes for a transaction (by txid) with decrypted fields
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory for privacy
    pub fn get_notes_by_txid(&self, account_id: i64, txid: &[u8]) -> Result<Vec<NoteRecord>> {
        // Since fields are encrypted, we need to decrypt all and filter
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id FROM notes",
        )?;

        let notes_data = stmt
            .query_map([], |row| {
                let note_type_str: String = row.get::<_, String>(2)?;
                let note_type = match note_type_str.as_str() {
                    "Orchard" => crate::models::NoteType::Orchard,
                    _ => crate::models::NoteType::Sapling,
                };

                Ok((
                    row.get::<_, i64>(0)?,     // id
                    row.get::<_, Vec<u8>>(1)?, // encrypted account_id
                    note_type,
                    row.get::<_, Vec<u8>>(3)?,          // encrypted value
                    row.get::<_, Vec<u8>>(4)?,          // encrypted nullifier
                    row.get::<_, Vec<u8>>(5)?,          // encrypted commitment
                    row.get::<_, Vec<u8>>(6)?,          // encrypted spent
                    row.get::<_, Vec<u8>>(7)?,          // encrypted height
                    row.get::<_, Vec<u8>>(8)?,          // encrypted txid
                    row.get::<_, Vec<u8>>(9)?,          // encrypted output_index
                    row.get::<_, Option<Vec<u8>>>(10)?, // encrypted spent_txid
                    row.get::<_, Option<Vec<u8>>>(11)?, // encrypted diversifier
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted key_id
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Decrypt all notes and filter by account_id and txid
        let mut decrypted_notes = Vec::new();
        for (
            id,
            enc_account_id,
            note_type,
            enc_value,
            enc_nullifier,
            enc_commitment,
            enc_spent,
            enc_height,
            enc_txid,
            enc_output_index,
            enc_spent_txid,
            enc_diversifier,
            enc_note,
            enc_position,
            enc_memo,
            enc_address_id,
            enc_key_id,
        ) in notes_data
        {
            let decrypted_account_id = self.decrypt_int64(&enc_account_id)?;
            let decrypted_txid = self.decrypt_blob(&enc_txid)?;

            // Filter by search criteria
            if decrypted_account_id == account_id && decrypted_txid == txid {
                decrypted_notes.push(NoteRecord {
                    id: Some(id),
                    account_id: decrypted_account_id,
                    key_id: self.decrypt_optional_int64(enc_key_id)?,
                    note_type,
                    value: self.decrypt_int64(&enc_value)?,
                    nullifier: self.decrypt_blob(&enc_nullifier)?,
                    commitment: self.decrypt_blob(&enc_commitment)?,
                    spent: self.decrypt_bool(&enc_spent)?,
                    height: self.decrypt_int64(&enc_height)?,
                    txid: decrypted_txid,
                    output_index: self.decrypt_int64(&enc_output_index)?,
                    address_id: self.decrypt_optional_int64(enc_address_id)?,
                    spent_txid: self.decrypt_optional_blob(enc_spent_txid)?,
                    diversifier: self.decrypt_optional_blob(enc_diversifier)?,
                    note: self.decrypt_optional_blob(enc_note)?,
                    position: self.decrypt_optional_int64(enc_position)?,
                    memo: self.decrypt_optional_blob(enc_memo)?,
                });
            }
        }

        // Sort by output_index
        decrypted_notes.sort_by_key(|n| n.output_index);

        Ok(decrypted_notes)
    }

    /// Insert sync log entry
    pub fn insert_sync_log(
        &self,
        wallet_id: &str,
        level: &str,
        module: &str,
        message: &str,
    ) -> Result<i64> {
        let timestamp = chrono::Utc::now().timestamp();
        self.db.conn().execute(
            "INSERT INTO sync_logs (wallet_id, timestamp, level, module, message) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![wallet_id, timestamp, level, module, message],
        )?;
        Ok(self.db.conn().last_insert_rowid())
    }

    /// Get sync logs for a wallet
    pub fn get_sync_logs(
        &self,
        wallet_id: &str,
        limit: u32,
    ) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT timestamp, level, module, message FROM sync_logs WHERE wallet_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
        )?;

        let rows = stmt.query_map(params![wallet_id, limit as i64], |row| {
            Ok((
                row.get(0)?, // timestamp
                row.get(1)?, // level
                row.get(2)?, // module
                row.get(3)?, // message
            ))
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        encryption::EncryptionKey,
        security::{EncryptionAlgorithm, MasterKey},
        FrontierStorage,
    };
    use incrementalmerkletree::Retention;
    use shardtree::ShardTree;
    use tempfile::NamedTempFile;
    use zcash_primitives::consensus::BlockHeight;
    use zcash_primitives::sapling::note::ExtractedNoteCommitment as SaplingExtractedNoteCommitment;
    use zcash_primitives::sapling::value::NoteValue as SaplingNoteValue;
    use zcash_primitives::sapling::{
        Node as SaplingNode, Note as SaplingNote, Rseed, NOTE_COMMITMENT_TREE_DEPTH,
    };
    use zcash_primitives::zip32::sapling::ExtendedSpendingKey as SaplingExtendedSpendingKey;

    fn test_db() -> Database {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        Database::open(file.path(), &key, master_key).unwrap()
    }

    fn test_db_with_master_key(master_key: MasterKey) -> Database {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        Database::open(file.path(), &key, master_key).unwrap()
    }

    fn insert_spendable_account_key(
        repo: &Repository,
        account_id: i64,
        birthday_height: i64,
    ) -> i64 {
        let key = AccountKey {
            id: None,
            account_id,
            key_type: KeyType::Seed,
            key_scope: KeyScope::Account,
            label: Some("seed".to_string()),
            birthday_height,
            created_at: chrono::Utc::now().timestamp(),
            spendable: true,
            sapling_extsk: Some(vec![0x11; 169]),
            sapling_dfvk: None,
            orchard_extsk: None,
            orchard_fvk: None,
            encrypted_mnemonic: None,
        };
        let encrypted = repo.encrypt_account_key_fields(&key).unwrap();
        repo.upsert_account_key(&encrypted).unwrap()
    }

    fn make_sapling_note_blob_and_commitment(
        value_zat: u64,
        seed_tag: u8,
        rseed_tag: u8,
    ) -> (Vec<u8>, [u8; 32]) {
        let seed = [seed_tag.max(1); 32];
        let extsk = SaplingExtendedSpendingKey::master(&seed);
        let (_, address) = extsk.default_address();
        let rseed_bytes = [rseed_tag; 32];
        let note_value = SaplingNoteValue::from_raw(value_zat);
        let note = SaplingNote::from_parts(address, note_value, Rseed::AfterZip212(rseed_bytes));
        let commitment_bytes = note.cmu().to_bytes();
        let mut bytes = Vec::with_capacity(1 + 43 + 1 + 32);
        bytes.push(1); // version
        bytes.extend_from_slice(&address.to_bytes());
        bytes.push(0x02); // ZIP-212 rseed variant
        bytes.extend_from_slice(&rseed_bytes);
        (bytes, commitment_bytes)
    }

    fn make_sapling_note_blob(seed_tag: u8, rseed_tag: u8) -> Vec<u8> {
        make_sapling_note_blob_and_commitment(1, seed_tag, rseed_tag).0
    }

    fn seed_sapling_shardtree_checkpoint(
        db: &Database,
        checkpoint_height: u32,
        leaf_count: usize,
        default_cmu: [u8; 32],
        overrides: &[(usize, [u8; 32])],
    ) {
        const SAPLING_TABLE_PREFIX: &str = "sapling";
        const SHARDTREE_PRUNING_DEPTH: usize = 1000;
        const SAPLING_SHARD_HEIGHT: u8 = NOTE_COMMITMENT_TREE_DEPTH / 2;

        let mut override_map = std::collections::HashMap::<usize, [u8; 32]>::new();
        for (pos, cmu) in overrides {
            override_map.insert(*pos, *cmu);
        }

        let tx = db
            .conn()
            .unchecked_transaction()
            .expect("failed to open shardtree transaction");
        let store = crate::shardtree_store::SqliteShardStore::<
            _,
            SaplingNode,
            SAPLING_SHARD_HEIGHT,
        >::from_connection(&tx, SAPLING_TABLE_PREFIX)
        .expect("failed to open shardtree store");
        let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
            ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);

        for idx in 0..leaf_count {
            let cmu_bytes = override_map.get(&idx).copied().unwrap_or(default_cmu);
            let cmu_opt: Option<SaplingExtractedNoteCommitment> =
                SaplingExtractedNoteCommitment::from_bytes(&cmu_bytes).into();
            let cmu_value = cmu_opt.expect("test cmu must be valid");
            let node = SaplingNode::from_cmu(&cmu_value);
            tree.append(node, Retention::Marked)
                .expect("failed to append test commitment");
        }

        tree.checkpoint(BlockHeight::from(checkpoint_height))
            .expect("failed to checkpoint shardtree");
        tx.commit().expect("failed to commit shardtree seed");
    }

    #[test]
    fn test_txid_hex_helpers_roundtrip() {
        let txid = "4fbf4e590e65cadc7d46ad2221d3565fae4652135e0380db35207219f8bea833";
        let reversed = reverse_txid_hex(txid).expect("valid txid");
        assert_eq!(
            reversed,
            "33a8bef819722035db80035e135246ae5f56d32122ad467ddcca650e594ebf4f"
        );

        let raw = hex::decode(txid).expect("hex decode");
        let display = txid_hex_from_bytes(&raw.iter().rev().copied().collect::<Vec<u8>>());
        assert_eq!(display, txid);
    }

    #[test]
    fn test_wallet_secret_encryption() {
        let db = test_db();
        let repo = Repository::new(&db);

        // Create a wallet secret with plaintext data
        let secret = WalletSecret {
            wallet_id: "test-wallet".to_string(),
            account_id: 1,
            extsk: b"test_extended_spending_key_data_32_bytes!".to_vec(),
            dfvk: Some(b"test_dfvk".to_vec()),
            orchard_extsk: Some(b"test_orchard_extsk".to_vec()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(b"test mnemonic phrase".to_vec()),
            mnemonic_language: Some("spanish".to_string()),
            created_at: chrono::Utc::now().timestamp(),
        };

        // Encrypt before storage
        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret).unwrap();

        // Verify encrypted fields are different from plaintext
        assert_ne!(encrypted_secret.extsk, secret.extsk);
        assert_ne!(encrypted_secret.dfvk, secret.dfvk);
        assert_ne!(encrypted_secret.orchard_extsk, secret.orchard_extsk);
        assert_ne!(
            encrypted_secret.encrypted_mnemonic,
            secret.encrypted_mnemonic
        );

        // Store encrypted secret
        repo.upsert_wallet_secret(&encrypted_secret).unwrap();

        // Retrieve and decrypt
        let retrieved = repo.get_wallet_secret("test-wallet").unwrap().unwrap();

        // Verify decrypted data matches original
        assert_eq!(retrieved.extsk, secret.extsk);
        assert_eq!(retrieved.dfvk, secret.dfvk);
        assert_eq!(retrieved.orchard_extsk, secret.orchard_extsk);
        assert_eq!(retrieved.encrypted_mnemonic, secret.encrypted_mnemonic);
        assert_eq!(retrieved.mnemonic_language, secret.mnemonic_language);
    }

    #[test]
    fn test_wallet_secret_wrong_key_fails() {
        // Create database with one master key
        let master_key1 = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let db1 = test_db_with_master_key(master_key1.clone());
        let repo1 = Repository::new(&db1);

        let secret = WalletSecret {
            wallet_id: "test-wallet".to_string(),
            account_id: 1,
            extsk: b"test_extended_spending_key_data_32_bytes!".to_vec(),
            dfvk: Some(b"test_dfvk_viewing_key".to_vec()),
            orchard_extsk: None,
            sapling_ivk: Some(b"test_sapling_ivk_32_bytes_data!".to_vec()),
            orchard_ivk: Some(b"test_orchard_ivk_64_bytes_data_for_privacy_blockchain!".to_vec()),
            encrypted_mnemonic: None,
            mnemonic_language: None,
            created_at: chrono::Utc::now().timestamp(),
        };

        // Encrypt and store with key1
        let encrypted_secret = repo1.encrypt_wallet_secret_fields(&secret).unwrap();
        repo1.upsert_wallet_secret(&encrypted_secret).unwrap();

        // Get the encrypted data directly from DB
        let encrypted_extsk: Vec<u8> = db1
            .conn()
            .query_row("SELECT extsk FROM wallet_secrets LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();

        // Try to decrypt with wrong key - should fail
        let master_key2 = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let db2 = test_db_with_master_key(master_key2);
        let repo2 = Repository::new(&db2);
        let result = repo2.decrypt_blob(&encrypted_extsk);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_wallet_secret_upsert_keeps_single_row_per_wallet() {
        let db = test_db();
        let repo = Repository::new(&db);

        let secret_v1 = WalletSecret {
            wallet_id: "test-wallet".to_string(),
            account_id: 1,
            extsk: b"extsk-v1".to_vec(),
            dfvk: Some(b"dfvk-v1".to_vec()),
            orchard_extsk: None,
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(b"mnemonic-v1".to_vec()),
            mnemonic_language: Some("english".to_string()),
            created_at: 100,
        };
        let secret_v2 = WalletSecret {
            wallet_id: "test-wallet".to_string(),
            account_id: 1,
            extsk: b"extsk-v2".to_vec(),
            dfvk: Some(b"dfvk-v2".to_vec()),
            orchard_extsk: Some(b"orchard-v2".to_vec()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(b"mnemonic-v2".to_vec()),
            mnemonic_language: Some("japanese".to_string()),
            created_at: 100,
        };

        let encrypted_v1 = repo.encrypt_wallet_secret_fields(&secret_v1).unwrap();
        repo.upsert_wallet_secret(&encrypted_v1).unwrap();
        let encrypted_v2 = repo.encrypt_wallet_secret_fields(&secret_v2).unwrap();
        repo.upsert_wallet_secret(&encrypted_v2).unwrap();

        let row_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM wallet_secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 1);

        let stored_wallet_id: String = db
            .conn()
            .query_row("SELECT wallet_id FROM wallet_secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_wallet_id, "test-wallet");

        let retrieved = repo.get_wallet_secret("test-wallet").unwrap().unwrap();
        assert_eq!(retrieved.extsk, secret_v2.extsk);
        assert_eq!(retrieved.dfvk, secret_v2.dfvk);
        assert_eq!(retrieved.orchard_extsk, secret_v2.orchard_extsk);
        assert_eq!(retrieved.encrypted_mnemonic, secret_v2.encrypted_mnemonic);
        assert_eq!(retrieved.mnemonic_language, secret_v2.mnemonic_language);
    }

    #[test]
    fn test_normalize_wallet_secrets_storage_repairs_legacy_duplicates() {
        let db = test_db();
        let repo = Repository::new(&db);

        let legacy_v1 = WalletSecret {
            wallet_id: "legacy-wallet".to_string(),
            account_id: 7,
            extsk: b"legacy-extsk-v1".to_vec(),
            dfvk: Some(b"legacy-dfvk-v1".to_vec()),
            orchard_extsk: None,
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(b"legacy mnemonic v1".to_vec()),
            mnemonic_language: Some("english".to_string()),
            created_at: 50,
        };
        let legacy_v2 = WalletSecret {
            wallet_id: "legacy-wallet".to_string(),
            account_id: 7,
            extsk: b"legacy-extsk-v2".to_vec(),
            dfvk: None,
            orchard_extsk: Some(b"legacy-orchard-v2".to_vec()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: None,
            mnemonic_language: None,
            created_at: 75,
        };

        let encrypted_v1 = repo.encrypt_wallet_secret_fields(&legacy_v1).unwrap();
        let encrypted_v2 = repo.encrypt_wallet_secret_fields(&legacy_v2).unwrap();

        let encrypted_wallet_id_v1 = repo.encrypt_blob(legacy_v1.wallet_id.as_bytes()).unwrap();
        let encrypted_wallet_id_v2 = repo.encrypt_blob(legacy_v2.wallet_id.as_bytes()).unwrap();
        let encrypted_account_id = repo.encrypt_int64(legacy_v1.account_id).unwrap();
        let encrypted_created_at_v1 = repo.encrypt_int64(legacy_v1.created_at).unwrap();
        let encrypted_created_at_v2 = repo.encrypt_int64(legacy_v2.created_at).unwrap();

        db.conn()
            .execute(
                "INSERT INTO wallet_secrets (wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    encrypted_wallet_id_v1,
                    encrypted_account_id,
                    encrypted_v1.extsk,
                    encrypted_v1.dfvk,
                    encrypted_v1.orchard_extsk,
                    encrypted_v1.sapling_ivk,
                    encrypted_v1.orchard_ivk,
                    encrypted_v1.encrypted_mnemonic,
                    encrypted_created_at_v1,
                ],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO wallet_secrets (wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    encrypted_wallet_id_v2,
                    repo.encrypt_int64(legacy_v2.account_id).unwrap(),
                    encrypted_v2.extsk,
                    encrypted_v2.dfvk,
                    encrypted_v2.orchard_extsk,
                    encrypted_v2.sapling_ivk,
                    encrypted_v2.orchard_ivk,
                    encrypted_v2.encrypted_mnemonic,
                    encrypted_created_at_v2,
                ],
            )
            .unwrap();

        let row_count_before: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM wallet_secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count_before, 2);

        let normalized = repo.normalize_wallet_secrets_storage().unwrap();
        assert!(normalized);

        let row_count_after: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM wallet_secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count_after, 1);

        let stored_wallet_id: String = db
            .conn()
            .query_row("SELECT wallet_id FROM wallet_secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_wallet_id, "legacy-wallet");

        let repaired = repo.get_wallet_secret("legacy-wallet").unwrap().unwrap();
        assert_eq!(repaired.extsk, legacy_v2.extsk);
        assert_eq!(repaired.dfvk, legacy_v1.dfvk);
        assert_eq!(repaired.orchard_extsk, legacy_v2.orchard_extsk);
        assert_eq!(repaired.encrypted_mnemonic, legacy_v1.encrypted_mnemonic);
        assert_eq!(repaired.created_at, 50);
    }

    #[test]
    fn test_note_encryption() {
        let db = test_db();
        let repo = Repository::new(&db);

        // Create account first
        let account = Account {
            id: None,
            name: "Test Account".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account).unwrap();

        // Create note with sensitive fields
        let note = NoteRecord {
            id: None,
            account_id,
            key_id: None,
            note_type: NoteType::Sapling,
            value: 1000,
            nullifier: vec![1, 2, 3],
            commitment: vec![4, 5, 6],
            spent: false,
            height: 100,
            txid: vec![7, 8, 9],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: Some(b"diversifier11".to_vec()),
            note: Some(b"serialized_note_data".to_vec()),
            position: None,
            memo: Some(b"test memo".to_vec()),
        };

        // Insert note (encryption happens inside)
        repo.insert_note(&note).unwrap();

        // Retrieve note (decryption happens inside)
        let retrieved = repo
            .get_note_by_txid_and_index(account_id, &note.txid, note.output_index)
            .unwrap()
            .unwrap();

        // Verify decrypted fields match original
        assert_eq!(retrieved.nullifier, note.nullifier);
        assert_eq!(retrieved.commitment, note.commitment);
        assert_eq!(retrieved.diversifier, note.diversifier);
        assert_eq!(retrieved.note, note.note);
        assert_eq!(retrieved.memo, note.memo);
    }

    #[test]
    fn test_note_encryption_stored_as_encrypted() {
        let db = test_db();
        let repo = Repository::new(&db);

        // Create account
        let account = Account {
            id: None,
            name: "Test Account".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account).unwrap();

        let plaintext_memo = b"secret memo";
        let plaintext_nullifier = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let plaintext_commitment = vec![
            4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
            27, 28, 29, 30, 31, 32, 33, 34, 35,
        ];
        let note = NoteRecord {
            id: None,
            account_id,
            key_id: None,
            note_type: NoteType::Sapling,
            value: 1000,
            nullifier: plaintext_nullifier.clone(),
            commitment: plaintext_commitment.clone(),
            spent: false,
            height: 100,
            txid: vec![7, 8, 9],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: None,
            position: None,
            memo: Some(plaintext_memo.to_vec()),
        };

        repo.insert_note(&note).unwrap();

        // Check that all sensitive fields are stored encrypted (different from plaintext)
        let (stored_nullifier, stored_commitment, stored_memo): (
            Vec<u8>,
            Vec<u8>,
            Option<Vec<u8>>,
        ) = db
            .conn()
            .query_row(
                "SELECT nullifier, commitment, memo FROM notes LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        // Verify nullifier is encrypted
        assert_ne!(
            stored_nullifier, plaintext_nullifier,
            "Nullifier should be encrypted in database"
        );
        assert!(
            stored_nullifier.len() > plaintext_nullifier.len(),
            "Encrypted nullifier should be larger (includes metadata and nonce)"
        );

        // Verify commitment is encrypted
        assert_ne!(
            stored_commitment, plaintext_commitment,
            "Commitment should be encrypted in database"
        );
        assert!(
            stored_commitment.len() > plaintext_commitment.len(),
            "Encrypted commitment should be larger (includes metadata and nonce)"
        );

        // Verify memo is encrypted
        if let Some(encrypted_memo) = stored_memo {
            assert_ne!(
                encrypted_memo, plaintext_memo,
                "Memo should be encrypted in database"
            );
            assert!(
                encrypted_memo.len() > plaintext_memo.len(),
                "Encrypted memo should be larger (includes metadata and nonce)"
            );
        }
    }

    #[test]
    fn test_transactions_use_tx_memo_when_note_memo_is_empty_payload() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account = Account {
            id: None,
            name: "Memo fallback".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account).unwrap();

        let txid = vec![
            0x01, 0x10, 0x02, 0x20, 0x03, 0x30, 0x04, 0x40, 0x05, 0x50, 0x06, 0x60, 0x07, 0x70,
            0x08, 0x80, 0x09, 0x90, 0x0a, 0xa0, 0x0b, 0xb0, 0x0c, 0xc0, 0x0d, 0xd0, 0x0e, 0xe0,
            0x0f, 0xf0, 0xaa, 0xbb,
        ];
        let note = NoteRecord {
            id: None,
            account_id,
            key_id: None,
            note_type: NoteType::Sapling,
            value: 123_000,
            nullifier: vec![0x11; 32],
            commitment: vec![0x22; 32],
            spent: false,
            height: 77_777,
            txid: txid.clone(),
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: None,
            position: None,
            memo: Some(vec![0u8; 512]),
        };
        repo.insert_note(&note).unwrap();

        let txid_hex = txid_hex_from_bytes(&txid);
        repo.upsert_tx_memo(&txid_hex, b"real memo text").unwrap();

        let txs = repo
            .get_transactions_with_options(account_id, None, 80_000, 10, false)
            .unwrap();
        let tx = txs
            .into_iter()
            .find(|entry| entry.txid == txid_hex)
            .expect("expected transaction entry");

        assert_eq!(tx.memo, Some(b"real memo text".to_vec()));
    }

    #[test]
    fn test_get_address_by_index_scope_is_explicit() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account = Account {
            id: None,
            name: "Scope Test".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account).unwrap();
        let key_id = 42_i64;
        let created_at = chrono::Utc::now().timestamp();

        let external = Address {
            id: None,
            key_id: Some(key_id),
            account_id,
            diversifier_index: 7,
            address: "zs1scopeexternal000000000000000000000000000000000000000000".to_string(),
            address_type: AddressType::Sapling,
            label: None,
            created_at,
            color_tag: ColorTag::None,
            address_scope: AddressScope::External,
        };
        let internal = Address {
            id: None,
            key_id: Some(key_id),
            account_id,
            diversifier_index: 7,
            address: "zs1scopeinternal000000000000000000000000000000000000000000".to_string(),
            address_type: AddressType::Sapling,
            label: None,
            created_at,
            color_tag: ColorTag::None,
            address_scope: AddressScope::Internal,
        };

        repo.upsert_address(&external).unwrap();
        repo.upsert_address(&internal).unwrap();

        let external_lookup = repo
            .get_address_by_index_for_scope(account_id, key_id, 7, AddressScope::External)
            .unwrap()
            .unwrap();
        assert_eq!(external_lookup.address, external.address);
        assert_eq!(external_lookup.address_scope, AddressScope::External);

        let internal_lookup = repo
            .get_address_by_index_for_scope(account_id, key_id, 7, AddressScope::Internal)
            .unwrap()
            .unwrap();
        assert_eq!(internal_lookup.address, internal.address);
        assert_eq!(internal_lookup.address_scope, AddressScope::Internal);

        // Backward-compatible helper now resolves to external only.
        let default_lookup = repo
            .get_address_by_index(account_id, key_id, 7)
            .unwrap()
            .unwrap();
        assert_eq!(default_lookup.address, external.address);
        assert_eq!(default_lookup.address_scope, AddressScope::External);
    }

    #[test]
    fn test_frontier_encryption() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        let plaintext_frontier = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        storage
            .save_frontier_snapshot(100, &plaintext_frontier, "1.0.0")
            .unwrap();

        // Verify stored frontier is encrypted
        let stored_frontier: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT frontier FROM frontier_snapshots WHERE height = ?1",
                [100],
                |row| row.get(0),
            )
            .unwrap();

        assert_ne!(
            stored_frontier, plaintext_frontier,
            "Frontier should be encrypted in database"
        );
        assert!(
            stored_frontier.len() > plaintext_frontier.len(),
            "Encrypted data should be larger"
        );

        // Verify decryption works
        let loaded = storage.load_last_snapshot().unwrap().unwrap();
        assert_eq!(
            loaded.1, plaintext_frontier,
            "Decrypted frontier should match original"
        );
    }

    #[test]
    fn test_anchor_filtered_notes_respect_wallet_birthday_floor() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account_id = repo
            .insert_account(&Account {
                id: None,
                name: "Birthday Floor".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            })
            .unwrap();
        let key_id = insert_spendable_account_key(&repo, account_id, 110);

        let (old_note_blob, old_cmu) = make_sapling_note_blob_and_commitment(7_000, 0x41, 0x51);
        let (new_note_blob, new_cmu) = make_sapling_note_blob_and_commitment(9_000, 0x42, 0x52);
        let old_note = NoteRecord {
            id: None,
            account_id,
            key_id: Some(key_id),
            note_type: NoteType::Sapling,
            value: 7_000,
            nullifier: vec![0xD1; 32],
            commitment: old_cmu.to_vec(),
            spent: false,
            height: 100,
            txid: vec![0xF1; 32],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: Some(old_note_blob),
            position: Some(12),
            memo: None,
        };
        let new_note = NoteRecord {
            id: None,
            account_id,
            key_id: Some(key_id),
            note_type: NoteType::Sapling,
            value: 9_000,
            nullifier: vec![0xD2; 32],
            commitment: new_cmu.to_vec(),
            spent: false,
            height: 130,
            txid: vec![0xF2; 32],
            output_index: 1,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: Some(new_note_blob),
            position: Some(13),
            memo: None,
        };
        repo.insert_note(&old_note).unwrap();
        repo.insert_note(&new_note).unwrap();

        seed_sapling_shardtree_checkpoint(
            &db,
            200,
            (new_note.position.unwrap() as usize) + 1,
            new_cmu,
            &[(old_note.position.unwrap() as usize, old_cmu)],
        );

        let selectable = repo
            .get_unspent_selectable_notes_at_anchor_filtered(account_id, 200, 10, None, None)
            .unwrap();
        let values = selectable.iter().map(|n| n.value).collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![9_000],
            "notes below wallet birthday must not be spendable"
        );
    }

    #[test]
    fn test_anchor_filtered_notes_are_blocked_by_position_shard_ranges() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account_id = repo
            .insert_account(&Account {
                id: None,
                name: "Position Gate".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            })
            .unwrap();
        let key_id = insert_spendable_account_key(&repo, account_id, 1);

        let note = NoteRecord {
            id: None,
            account_id,
            key_id: Some(key_id),
            note_type: NoteType::Sapling,
            value: 33_000,
            nullifier: vec![0x91; 32],
            commitment: vec![0x92; 32],
            spent: false,
            height: 100,
            txid: vec![0x93; 32],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: Some(make_sapling_note_blob(0x71, 0x81)),
            position: Some(9), // shard 0
            memo: None,
        };
        repo.insert_note(&note).unwrap();

        // Force shard metadata to a range that differs from note.height so
        // position/shard-index gating is required.
        db.conn()
            .execute(
                "UPDATE sapling_note_shards SET subtree_start_height = 500, subtree_end_height = 520 WHERE shard_index = 0",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                r#"
                INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
                "#,
                params![500_i64, 521_i64, 60_i64, "pending", "position_gate_test"],
            )
            .unwrap();

        let selectable = repo
            .get_unspent_selectable_notes_at_anchor_filtered(account_id, 900, 10, None, None)
            .unwrap();
        assert!(
            selectable.is_empty(),
            "position/shard-index unscanned range should block note selection"
        );
    }

    #[test]
    fn test_check_witnesses_queues_subtree_ranges_from_note_position() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account_id = repo
            .insert_account(&Account {
                id: None,
                name: "Witness Position Queue".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            })
            .unwrap();
        let key_id = insert_spendable_account_key(&repo, account_id, 1);

        let note = NoteRecord {
            id: None,
            account_id,
            key_id: Some(key_id),
            note_type: NoteType::Sapling,
            value: 44_000,
            nullifier: vec![0xA3; 32],
            commitment: vec![0xA4; 32],
            spent: false,
            height: 120,
            txid: vec![0xA5; 32],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: None,         // force witness-material missing
            position: Some(11), // shard 0
            memo: None,
        };
        repo.insert_note(&note).unwrap();

        db.conn()
            .execute(
                "UPDATE sapling_note_shards SET subtree_start_height = 700, subtree_end_height = 709 WHERE shard_index = 0",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                r#"
                INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
                "#,
                params![700_i64, 710_i64, 75_i64, "pending", "witness_queue_test"],
            )
            .unwrap();

        let result = repo.check_witnesses(account_id, 900, 1).unwrap();
        assert_eq!(result.considered_notes, 1);
        assert_eq!(result.sapling_missing, 1);
        assert_eq!(
            result.repair_ranges,
            vec![(700, 710)],
            "check_witnesses must queue subtree-derived range from note position"
        );
    }

    #[test]
    fn test_check_witnesses_queues_anchor_hydration_gap_range() {
        let db = test_db();
        let repo = Repository::new(&db);

        let account_id = repo
            .insert_account(&Account {
                id: None,
                name: "Anchor Hydration Gap".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            })
            .unwrap();
        let key_id = insert_spendable_account_key(&repo, account_id, 1);

        let note = NoteRecord {
            id: None,
            account_id,
            key_id: Some(key_id),
            note_type: NoteType::Sapling,
            value: 55_000,
            nullifier: vec![0xB1; 32],
            commitment: vec![0xB2; 32],
            spent: false,
            height: 120,
            txid: vec![0xB3; 32],
            output_index: 0,
            address_id: None,
            spent_txid: None,
            diversifier: None,
            note: Some(make_sapling_note_blob(0x72, 0x82)),
            position: Some(9),
            memo: None,
        };
        repo.insert_note(&note).unwrap();

        // No shardtree checkpoints are present in this test DB, so anchored
        // hydration should fail and queue a deterministic replay range.
        let result = repo.check_witnesses(account_id, 900, 1).unwrap();
        assert_eq!(result.considered_notes, 1);
        assert_eq!(result.sapling_missing, 1);
        assert_eq!(result.orchard_missing, 0);
        assert_eq!(
            result.repair_ranges,
            vec![(120, 901)],
            "anchor hydration gaps must queue replay from earliest missing note to anchor+1"
        );
    }
}
