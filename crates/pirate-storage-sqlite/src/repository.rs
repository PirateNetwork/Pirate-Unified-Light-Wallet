//! Data access layer

use crate::address_book::ColorTag;
use crate::{models::*, Database, Result};
use directories::ProjectDirs;
use pirate_core::DEFAULT_FEE;
use pirate_params::consensus::ConsensusParams;
use rusqlite::params_from_iter;
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;

/// Repository for database operations
pub struct Repository<'a> {
    db: &'a Database,
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

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        ProjectDirs::from("com", "Pirate", "PirateWallet")
            .map(|dirs| dirs.data_local_dir().join("logs").join("debug.log"))
            .unwrap_or_else(|| {
                env::current_dir()
                    .map(|dir| dir.join(".cursor").join("debug.log"))
                    .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
            })
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

fn note_value_is_valid(value: i64) -> bool {
    if value <= 0 {
        return false;
    }
    value as u64 <= ConsensusParams::mainnet().max_money
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
        Self { db }
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

    /// Encrypt string as BLOB (for privacy)
    fn encrypt_string(&self, value: &str) -> Result<Vec<u8>> {
        self.encrypt_blob(value.as_bytes())
    }

    /// Decrypt string from BLOB
    fn decrypt_string(&self, encrypted: &[u8]) -> Result<String> {
        let decrypted = self.decrypt_blob(encrypted)?;
        String::from_utf8(decrypted)
            .map_err(|e| crate::Error::Encryption(format!("Failed to decode string: {}", e)))
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

    /// Insert note with encrypted sensitive fields
    pub fn insert_note(&self, note: &NoteRecord) -> Result<i64> {
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
        let encrypted_merkle_path =
            self.encrypt_blob(note.merkle_path.as_deref().unwrap_or(&[]))?;
        let encrypted_note = self.encrypt_blob(note.note.as_deref().unwrap_or(&[]))?;
        let encrypted_anchor = self.encrypt_optional_blob(note.anchor.as_deref())?;
        let encrypted_position = self.encrypt_optional_int64(note.position)?;
        let encrypted_memo = self.encrypt_optional_blob(note.memo.as_deref())?;
        let encrypted_spent_txid = self.encrypt_optional_blob(note.spent_txid.as_deref())?;
        let encrypted_address_id = self.encrypt_optional_int64(note.address_id)?;
        let encrypted_key_id = self.encrypt_optional_int64(note.key_id)?;

        self.db.conn().execute(
            "INSERT INTO notes (account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo, address_id, key_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
                encrypted_merkle_path,
                encrypted_note,
                encrypted_anchor,
                encrypted_position,
                encrypted_memo,
                encrypted_address_id,
                encrypted_key_id,
            ],
        )?;
        Ok(self.db.conn().last_insert_rowid())
    }

    /// Update an existing note by row id (encrypts before storage)
    pub fn update_note_by_id(&self, note: &NoteRecord) -> Result<()> {
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
        let encrypted_merkle_path =
            self.encrypt_blob(note.merkle_path.as_deref().unwrap_or(&[]))?;
        let encrypted_note = self.encrypt_blob(note.note.as_deref().unwrap_or(&[]))?;
        let encrypted_anchor = self.encrypt_optional_blob(note.anchor.as_deref())?;
        let encrypted_position = self.encrypt_optional_int64(note.position)?;
        let encrypted_memo = self.encrypt_optional_blob(note.memo.as_deref())?;
        let encrypted_spent_txid = self.encrypt_optional_blob(note.spent_txid.as_deref())?;
        let encrypted_address_id = self.encrypt_optional_int64(note.address_id)?;
        let encrypted_key_id = self.encrypt_optional_int64(note.key_id)?;

        self.db.conn().execute(
            "UPDATE notes SET account_id = ?1, note_type = ?2, value = ?3, nullifier = ?4, commitment = ?5, spent = ?6, height = ?7, txid = ?8, output_index = ?9, spent_txid = ?10, diversifier = ?11, merkle_path = ?12, note = ?13, anchor = ?14, position = ?15, memo = ?16, address_id = ?17, key_id = ?18 WHERE id = ?19",
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
                encrypted_merkle_path,
                encrypted_note,
                encrypted_anchor,
                encrypted_position,
                encrypted_memo,
                encrypted_address_id,
                encrypted_key_id,
                id,
            ],
        )?;
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
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo, address_id, key_id FROM notes",
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
                let encrypted_merkle_path: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(12)?;
                let encrypted_note: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(13)?;
                let encrypted_anchor: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(14)?;
                let encrypted_position: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(15)?;
                let encrypted_memo: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(16)?;
                let encrypted_address_id: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(17)?;
                let encrypted_key_id: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(18)?;

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
                    encrypted_merkle_path,
                    encrypted_note,
                    encrypted_anchor,
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
            enc_merkle_path,
            enc_note,
            enc_anchor,
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
                merkle_path: self.decrypt_optional_blob(enc_merkle_path)?,
                note: self.decrypt_optional_blob(enc_note)?,
                anchor: self.decrypt_optional_blob(enc_anchor)?,
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
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_unspent_notes","timestamp":{},"location":"repository.rs:344","message":"get_unspent_notes","data":{{"account_id":{},"rows":{},"matched":{},"invalid_values":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, account_id, total_rows, matched, invalid_values
            );
        }
        // #endregion

        Ok(decrypted_notes)
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
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo, address_id, key_id FROM notes",
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
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted merkle_path
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted anchor
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(17)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(18)?, // encrypted key_id
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
            enc_merkle_path,
            enc_note,
            enc_anchor,
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
                merkle_path: self.decrypt_optional_blob(enc_merkle_path)?,
                note: self.decrypt_optional_blob(enc_note)?,
                anchor: self.decrypt_optional_blob(enc_anchor)?,
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
        // Encrypt metadata fields for privacy
        let encrypted_wallet_id = self.encrypt_string(&secret.wallet_id)?;
        let encrypted_account_id = self.encrypt_int64(secret.account_id)?;
        let encrypted_created_at = self.encrypt_int64(secret.created_at)?;

        self.db.conn().execute(
            "INSERT INTO wallet_secrets (wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(wallet_id) DO UPDATE SET account_id=excluded.account_id, extsk=excluded.extsk, dfvk=excluded.dfvk, orchard_extsk=excluded.orchard_extsk, sapling_ivk=excluded.sapling_ivk, orchard_ivk=excluded.orchard_ivk, encrypted_mnemonic=excluded.encrypted_mnemonic, created_at=excluded.created_at",
            params![encrypted_wallet_id, encrypted_account_id, secret.extsk, secret.dfvk, secret.orchard_extsk, secret.sapling_ivk, secret.orchard_ivk, secret.encrypted_mnemonic, encrypted_created_at],
        )?;
        Ok(())
    }

    /// Encrypt wallet secret fields before storage
    /// Note: wallet_id, account_id, and created_at are encrypted in upsert_wallet_secret
    pub fn encrypt_wallet_secret_fields(&self, secret: &WalletSecret) -> Result<WalletSecret> {
        Ok(WalletSecret {
            wallet_id: secret.wallet_id.clone(), // Will be encrypted in upsert_wallet_secret
            account_id: secret.account_id,       // Will be encrypted in upsert_wallet_secret
            extsk: self.encrypt_blob(&secret.extsk)?,
            dfvk: self.encrypt_optional_blob(secret.dfvk.as_deref())?, // Encrypt viewing key for privacy
            orchard_extsk: self.encrypt_optional_blob(secret.orchard_extsk.as_deref())?,
            sapling_ivk: self.encrypt_optional_blob(secret.sapling_ivk.as_deref())?, // Encrypt viewing key for privacy
            orchard_ivk: self.encrypt_optional_blob(secret.orchard_ivk.as_deref())?, // Encrypt viewing key for privacy
            encrypted_mnemonic: self.encrypt_optional_blob(secret.encrypted_mnemonic.as_deref())?,
            created_at: secret.created_at, // Will be encrypted in upsert_wallet_secret
        })
    }

    /// Get wallet secret and decrypt encrypted fields
    /// Note: Since wallet_id is encrypted, we decrypt all wallet_secrets and filter in memory for privacy
    pub fn get_wallet_secret(&self, wallet_id: &str) -> Result<Option<WalletSecret>> {
        // Since wallet_id is encrypted, we need to decrypt all and filter
        let mut stmt = self.db.conn().prepare(
            "SELECT wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at FROM wallet_secrets",
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            // Decrypt all encrypted fields including metadata and viewing keys for privacy
            let encrypted_wallet_id_db: Vec<u8> = row.get::<_, Vec<u8>>(0)?;
            let encrypted_account_id: Vec<u8> = row.get::<_, Vec<u8>>(1)?;
            let encrypted_extsk: Vec<u8> = row.get::<_, Vec<u8>>(2)?;
            let encrypted_dfvk: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(3)?;
            let encrypted_orchard_extsk: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(4)?;
            let encrypted_sapling_ivk: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(5)?;
            let encrypted_orchard_ivk: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(6)?;
            let encrypted_mnemonic: Option<Vec<u8>> = row.get::<_, Option<Vec<u8>>>(7)?;
            let encrypted_created_at: Vec<u8> = row.get::<_, Vec<u8>>(8)?;

            let wallet_id_decrypted = self.decrypt_string(&encrypted_wallet_id_db)?;

            // Filter by wallet_id
            if wallet_id_decrypted == wallet_id {
                let account_id = self.decrypt_int64(&encrypted_account_id)?;
                let extsk = self.decrypt_blob(&encrypted_extsk)?;
                let dfvk = self.decrypt_optional_blob(encrypted_dfvk)?; // Decrypt viewing key for privacy
                let orchard_extsk = self.decrypt_optional_blob(encrypted_orchard_extsk)?;
                let sapling_ivk = self.decrypt_optional_blob(encrypted_sapling_ivk)?; // Decrypt viewing key for privacy
                let orchard_ivk = self.decrypt_optional_blob(encrypted_orchard_ivk)?; // Decrypt viewing key for privacy
                let encrypted_mnemonic = self.decrypt_optional_blob(encrypted_mnemonic)?;
                let created_at = self.decrypt_int64(&encrypted_created_at)?;

                return Ok(Some(WalletSecret {
                    wallet_id: wallet_id_decrypted,
                    account_id,
                    extsk,
                    dfvk,
                    orchard_extsk,
                    sapling_ivk,
                    orchard_ivk,
                    encrypted_mnemonic,
                    created_at,
                }));
            }
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
        use orchard::tree::{
            Anchor as OrchardAnchor, MerkleHashOrchard, MerklePath as OrchardMerklePath,
        };
        use orchard::value::NoteValue as OrchardNoteValue;
        use orchard::Address as OrchardAddress;
        use pirate_core::selection::SelectableNote;
        use zcash_primitives::merkle_tree::merkle_path_from_slice;
        use zcash_primitives::sapling::value::NoteValue as SaplingNoteValue;
        use zcash_primitives::sapling::{Node, Note as SaplingNote, PaymentAddress, Rseed};

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

        fn parse_orchard_merkle_path(bytes: &[u8]) -> Option<OrchardMerklePath> {
            const ORCHARD_PATH_LEN: usize = 4 + 32 * 32;
            if bytes.len() != ORCHARD_PATH_LEN {
                return None;
            }

            let position = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
            let mut auth = Vec::with_capacity(32);
            let mut offset = 4;
            for _ in 0..32 {
                let mut hash_bytes = [0u8; 32];
                hash_bytes.copy_from_slice(&bytes[offset..offset + 32]);
                offset += 32;
                let hash = Option::from(MerkleHashOrchard::from_bytes(&hash_bytes))?;
                auth.push(hash);
            }
            let auth_path: [MerkleHashOrchard; 32] = auth.try_into().ok()?;
            Some(OrchardMerklePath::from_parts(position, auth_path))
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
        let mut skipped_missing_merkle = 0usize;
        let mut skipped_missing_note = 0usize;
        let mut skipped_invalid_address = 0usize;
        let mut skipped_invalid_rseed = 0usize;
        let mut skipped_invalid_note = 0usize;

        for n in notes {
            match n.note_type {
                crate::models::NoteType::Sapling => {
                    let merkle_path = n.merkle_path.as_ref().and_then(|merkle_path| {
                        if merkle_path.is_empty() {
                            None
                        } else {
                            merkle_path_from_slice::<
                                Node,
                                { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
                            >(&merkle_path[..])
                            .ok()
                        }
                    });

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

                    let merkle_path = match merkle_path {
                        Some(path) => path,
                        None => {
                            skipped_missing_merkle += 1;
                            continue;
                        }
                    };

                    sn = sn.with_witness(merkle_path, *address.diversifier(), note);
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
                    let note = match Option::from(OrchardNote::from_parts(
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

                    let anchor = if let Some(ref anchor_bytes) = n.anchor {
                        if anchor_bytes.len() == 32 {
                            let mut anchor_array = [0u8; 32];
                            anchor_array.copy_from_slice(&anchor_bytes[..32]);
                            let ct_option = OrchardAnchor::from_bytes(anchor_array);
                            let opt: Option<OrchardAnchor> = ct_option.into();
                            opt
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let position = match n.position {
                        Some(pos) => pos as u64,
                        None => {
                            skipped_missing_merkle += 1;
                            continue;
                        }
                    };

                    let merkle_path = n.merkle_path.as_ref().and_then(|merkle_path| {
                        if merkle_path.is_empty() {
                            None
                        } else {
                            parse_orchard_merkle_path(merkle_path)
                        }
                    });

                    let merkle_path = match merkle_path {
                        Some(path) => path,
                        None => {
                            skipped_missing_merkle += 1;
                            continue;
                        }
                    };

                    let anchor = match anchor {
                        Some(value) => value,
                        None => {
                            skipped_missing_merkle += 1;
                            continue;
                        }
                    };

                    sn = sn.with_orchard_witness(anchor, position, merkle_path, note);
                    result.push(sn);
                }
            }
        }

        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_selectable_notes","timestamp":{},"location":"repository.rs:621","message":"get_unspent_selectable_notes","data":{{"account_id":{},"key_id":{},"address_id":{},"key_ids_count":{},"address_ids_count":{},"notes":{},"selectable":{},"missing_merkle":{},"missing_note":{},"invalid_address":{},"invalid_rseed":{},"invalid_note":{},"skipped_key_mismatch":{},"skipped_key_missing":{},"skipped_address_mismatch":{},"skipped_address_missing":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                account_id,
                key_id_log,
                address_id_log,
                key_ids_count,
                address_ids_count,
                total_notes,
                result.len(),
                skipped_missing_merkle,
                skipped_missing_note,
                skipped_invalid_address,
                skipped_invalid_rseed,
                skipped_invalid_note,
                skipped_key_mismatch,
                skipped_key_missing,
                skipped_address_mismatch,
                skipped_address_missing
            );
        }
        // #endregion

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

        // Calculate confirmation threshold
        let confirmation_threshold = current_height.saturating_sub(min_depth);

        for note in notes {
            let note_height = note.height as u64;
            let note_value = note.value as u64;

            // Note is confirmed if it has at least min_depth confirmations
            // (i.e., note_height <= current_height - min_depth)
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
        let scope_str = match scope {
            crate::models::AddressScope::External => "external",
            crate::models::AddressScope::Internal => "internal",
        };
        let max_index: Option<i64> = self.db.conn().query_row(
            "SELECT MAX(diversifier_index) FROM addresses WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3",
            params![account_id, key_id, scope_str],
            |row| row.get(0),
        )?;

        Ok(match max_index {
            Some(max) => max as u32,
            None => 0,
        })
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
        let rows = self.db.conn().execute(
            "UPDATE addresses SET key_id = ?1 WHERE account_id = ?2 AND key_id IS NULL",
            params![key_id, account_id],
        )?;
        Ok(rows)
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
        let address_type_str = match address.address_type {
            crate::models::AddressType::Sapling => "Sapling",
            crate::models::AddressType::Orchard => "Orchard",
        };
        self.db.conn().execute(
            "INSERT INTO addresses (account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(address) DO UPDATE SET
                 account_id = excluded.account_id,
                 key_id = COALESCE(excluded.key_id, addresses.key_id),
                 diversifier_index = CASE
                     WHEN excluded.diversifier_index = 0 THEN addresses.diversifier_index
                     ELSE excluded.diversifier_index
                 END,
                 address_type = excluded.address_type,
                 label = COALESCE(excluded.label, addresses.label),
                 created_at = addresses.created_at,
                 color_tag = addresses.color_tag,
                 address_scope = CASE
                     WHEN addresses.address_scope = 'internal' THEN addresses.address_scope
                     ELSE excluded.address_scope
                 END",
            params![
                address.account_id,
                address.key_id,
                address.diversifier_index as i64,
                address.address,
                address_type_str,
                address.label,
                address.created_at,
                address.color_tag.as_u8() as i64,
                match address.address_scope {
                    crate::models::AddressScope::External => "external",
                    crate::models::AddressScope::Internal => "internal",
                },
            ],
        )?;
        Ok(())
    }

    /// Get address by address string.
    pub fn get_address_by_string(&self, account_id: i64, address: &str) -> Result<Option<Address>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope FROM addresses              WHERE account_id = ?1 AND address = ?2",
        )?;

        let result = stmt
            .query_row(params![account_id, address], |row| {
                let address_type_str: String = row.get(5).unwrap_or_else(|_| "Sapling".to_string());
                let address_type = match address_type_str.as_str() {
                    "Orchard" => crate::models::AddressType::Orchard,
                    _ => crate::models::AddressType::Sapling,
                };
                let address_scope_str: String =
                    row.get(9).unwrap_or_else(|_| "external".to_string());
                let address_scope = match address_scope_str.as_str() {
                    "internal" => crate::models::AddressScope::Internal,
                    _ => crate::models::AddressScope::External,
                };
                Ok(Address {
                    id: Some(row.get(0)?),
                    account_id: row.get(1)?,
                    key_id: row.get(2)?,
                    diversifier_index: row.get::<_, i64>(3)? as u32,
                    address: row.get(4)?,
                    address_type,
                    label: row.get(6)?,
                    created_at: row.get(7)?,
                    color_tag: ColorTag::from_u8(row.get::<_, i64>(8)? as u8),
                    address_scope,
                })
            })
            .optional()?;

        Ok(result)
    }

    /// Get address by diversifier index for a key group and scope.
    pub fn get_address_by_index_for_scope(
        &self,
        account_id: i64,
        key_id: i64,
        diversifier_index: u32,
        scope: crate::models::AddressScope,
    ) -> Result<Option<Address>> {
        let scope_str = match scope {
            crate::models::AddressScope::External => "external",
            crate::models::AddressScope::Internal => "internal",
        };
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope FROM addresses 
             WHERE account_id = ?1 AND key_id = ?2 AND diversifier_index = ?3 AND address_scope = ?4",
        )?;

        let result = stmt
            .query_row(
                params![account_id, key_id, diversifier_index as i64, scope_str],
                |row| {
                    let address_type_str: String =
                        row.get(5).unwrap_or_else(|_| "Sapling".to_string());
                    let address_type = match address_type_str.as_str() {
                        "Orchard" => crate::models::AddressType::Orchard,
                        _ => crate::models::AddressType::Sapling, // Default to Sapling
                    };
                    let address_scope_str: String =
                        row.get(9).unwrap_or_else(|_| "external".to_string());
                    let address_scope = match address_scope_str.as_str() {
                        "internal" => crate::models::AddressScope::Internal,
                        _ => crate::models::AddressScope::External,
                    };
                    Ok(Address {
                        id: Some(row.get(0)?),
                        account_id: row.get(1)?,
                        key_id: row.get(2)?,
                        diversifier_index: row.get::<_, i64>(3)? as u32,
                        address: row.get(4)?,
                        address_type,
                        label: row.get(6)?,
                        created_at: row.get(7)?,
                        color_tag: ColorTag::from_u8(row.get::<_, i64>(8)? as u8),
                        address_scope,
                    })
                },
            )
            .optional()?;

        Ok(result)
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
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope FROM addresses 
             WHERE account_id = ?1 
             ORDER BY diversifier_index ASC",
        )?;

        let addresses = stmt
            .query_map([account_id], |row| {
                let address_type_str: String = row.get(5).unwrap_or_else(|_| "Sapling".to_string());
                let address_type = match address_type_str.as_str() {
                    "Orchard" => crate::models::AddressType::Orchard,
                    _ => crate::models::AddressType::Sapling, // Default to Sapling
                };
                let address_scope_str: String =
                    row.get(9).unwrap_or_else(|_| "external".to_string());
                let address_scope = match address_scope_str.as_str() {
                    "internal" => crate::models::AddressScope::Internal,
                    _ => crate::models::AddressScope::External,
                };
                Ok(Address {
                    id: Some(row.get(0)?),
                    account_id: row.get(1)?,
                    key_id: row.get(2)?,
                    diversifier_index: row.get::<_, i64>(3)? as u32,
                    address: row.get(4)?,
                    address_type,
                    label: row.get(6)?,
                    created_at: row.get(7)?,
                    color_tag: ColorTag::from_u8(row.get::<_, i64>(8)? as u8),
                    address_scope,
                })
            })?
            .collect::<std::result::Result<Vec<Address>, rusqlite::Error>>()?;

        Ok(addresses)
    }

    /// Get all addresses for a key group
    pub fn get_addresses_by_key(&self, account_id: i64, key_id: i64) -> Result<Vec<Address>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope FROM addresses 
             WHERE account_id = ?1 AND key_id = ?2
             ORDER BY diversifier_index ASC",
        )?;

        let addresses = stmt
            .query_map([account_id, key_id], |row| {
                let address_type_str: String = row.get(5).unwrap_or_else(|_| "Sapling".to_string());
                let address_type = match address_type_str.as_str() {
                    "Orchard" => crate::models::AddressType::Orchard,
                    _ => crate::models::AddressType::Sapling,
                };
                let address_scope_str: String =
                    row.get(9).unwrap_or_else(|_| "external".to_string());
                let address_scope = match address_scope_str.as_str() {
                    "internal" => crate::models::AddressScope::Internal,
                    _ => crate::models::AddressScope::External,
                };
                Ok(Address {
                    id: Some(row.get(0)?),
                    account_id: row.get(1)?,
                    key_id: row.get(2)?,
                    diversifier_index: row.get::<_, i64>(3)? as u32,
                    address: row.get(4)?,
                    address_type,
                    label: row.get(6)?,
                    created_at: row.get(7)?,
                    color_tag: ColorTag::from_u8(row.get::<_, i64>(8)? as u8),
                    address_scope,
                })
            })?
            .collect::<std::result::Result<Vec<Address>, rusqlite::Error>>()?;

        Ok(addresses)
    }

    /// Update address label
    pub fn update_address_label(
        &self,
        account_id: i64,
        address: &str,
        label: Option<String>,
    ) -> Result<()> {
        self.db.conn().execute(
            "UPDATE addresses SET label = ?1 WHERE account_id = ?2 AND address = ?3",
            params![label, account_id, address],
        )?;
        Ok(())
    }

    /// Update address color tag
    pub fn update_address_color_tag(
        &self,
        account_id: i64,
        address: &str,
        color_tag: ColorTag,
    ) -> Result<()> {
        self.db.conn().execute(
            "UPDATE addresses SET color_tag = ?1 WHERE account_id = ?2 AND address = ?3",
            params![color_tag.as_u8() as i64, account_id, address],
        )?;
        Ok(())
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
            let memo = self.decrypt_optional_blob(encrypted_memo)?;
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
            let stored_height = heights_map
                .get(txid)
                .copied()
                .or_else(|| reverse_txid_hex(txid).and_then(|alt| heights_map.get(&alt).copied()));
            if let Some(height) = stored_height {
                if height > 0 {
                    entry.height = height;
                }
            }
        }

        // Convert to TransactionRecord and calculate net amount
        let mut transactions: Vec<TransactionRecord> = Vec::new();
        for (txid, entry) in tx_map.into_iter() {
            // Net amount: positive for receive, negative for send
            let total_received = entry
                .received_external
                .saturating_add(entry.received_internal);
            let net_amount = total_received.saturating_sub(entry.sent);
            let memo = entry.memo.or_else(|| {
                memo_map
                    .get(&txid)
                    .cloned()
                    .or_else(|| reverse_txid_hex(&txid).and_then(|alt| memo_map.get(&alt).cloned()))
            });

            // Use stored transaction timestamp if available (first confirmation time).
            // Fallback: current time (unconfirmed or not yet populated).
            let timestamp = ts_map
                .get(&txid)
                .copied()
                .or_else(|| reverse_txid_hex(&txid).and_then(|alt| ts_map.get(&alt).copied()))
                .unwrap_or_else(|| chrono::Utc::now().timestamp());

            let stored_fee = fee_map
                .get(&txid)
                .copied()
                .or_else(|| reverse_txid_hex(&txid).and_then(|alt| fee_map.get(&alt).copied()))
                .unwrap_or(0);
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

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
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
        }

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

    /// Get note by transaction ID and output index with decrypted fields
    /// Note: Since all fields are encrypted, we decrypt all notes and filter in memory for privacy
    pub fn get_note_by_txid_and_index(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
    ) -> Result<Option<NoteRecord>> {
        // Since fields are encrypted, we need to decrypt all and filter
        // For efficiency with large datasets, this could be optimized with an index table
        let mut stmt = self.db.conn().prepare(
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo, address_id, key_id FROM notes",
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
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted merkle_path
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted anchor
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(17)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(18)?, // encrypted key_id
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Decrypt and filter in memory
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
            enc_merkle_path,
            enc_note,
            enc_anchor,
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
            if decrypted_account_id == account_id
                && decrypted_txid == txid
                && decrypted_output_index == output_index
            {
                return Ok(Some(NoteRecord {
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
                    merkle_path: self.decrypt_optional_blob(enc_merkle_path)?,
                    note: self.decrypt_optional_blob(enc_note)?,
                    anchor: self.decrypt_optional_blob(enc_anchor)?,
                    position: self.decrypt_optional_int64(enc_position)?,
                    memo: self.decrypt_optional_blob(enc_memo)?,
                }));
            }
        }

        Ok(None)
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

    /// Update memo for a note (encrypts before storage)
    /// Note: Since all fields are encrypted, we need to decrypt all notes and filter in memory
    pub fn update_note_memo(
        &self,
        account_id: i64,
        txid: &[u8],
        output_index: i64,
        memo: Option<&[u8]>,
    ) -> Result<()> {
        // Encrypt memo for storage
        let encrypted_memo = self.encrypt_optional_blob(memo)?;

        // Since fields are encrypted, we need to find the note by decrypting all
        // This is less efficient but necessary for maximum privacy
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

        // Find matching note by decrypting and comparing
        for (id, enc_acc_id, enc_tx, enc_out_idx) in notes {
            let decrypted_account_id = self.decrypt_int64(&enc_acc_id)?;
            let decrypted_txid = self.decrypt_blob(&enc_tx)?;
            let decrypted_output_index = self.decrypt_int64(&enc_out_idx)?;

            if decrypted_account_id == account_id
                && decrypted_txid == txid
                && decrypted_output_index == output_index
            {
                // Found the note, update it using the id
                self.db.conn().execute(
                    "UPDATE notes SET memo = ?1 WHERE id = ?2",
                    params![encrypted_memo, id],
                )?;
                return Ok(());
            }
        }

        // Note not found
        Ok(())
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
    /// This mirrors the upstream "unlinked nullifier map" concept:
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
    ) -> Result<Vec<(NoteType, [u8; 32], [u8; 32])>> {
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

        let mut out: Vec<(NoteType, [u8; 32], [u8; 32])> = Vec::new();
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

        let mut expected: HashMap<(NoteType, [u8; 32]), [u8; 32]> = HashMap::new();
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

        let mut resolved: HashSet<(NoteType, [u8; 32])> = HashSet::new();
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
        nullifiers: &[(NoteType, [u8; 32])],
    ) -> Result<HashMap<(NoteType, [u8; 32]), [u8; 32]>> {
        if nullifiers.is_empty() {
            return Ok(HashMap::new());
        }

        let wanted: HashSet<(NoteType, [u8; 32])> = nullifiers
            .iter()
            .copied()
            .filter(|(_, nf)| !nf.iter().all(|b| *b == 0))
            .collect();
        if wanted.is_empty() {
            return Ok(HashMap::new());
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
        let mut matched: HashMap<(NoteType, [u8; 32]), [u8; 32]> = HashMap::new();

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
            "SELECT id, account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, merkle_path, note, anchor, position, memo, address_id, key_id FROM notes",
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
                    row.get::<_, Option<Vec<u8>>>(12)?, // encrypted merkle_path
                    row.get::<_, Option<Vec<u8>>>(13)?, // encrypted note
                    row.get::<_, Option<Vec<u8>>>(14)?, // encrypted anchor
                    row.get::<_, Option<Vec<u8>>>(15)?, // encrypted position
                    row.get::<_, Option<Vec<u8>>>(16)?, // encrypted memo
                    row.get::<_, Option<Vec<u8>>>(17)?, // encrypted address_id
                    row.get::<_, Option<Vec<u8>>>(18)?, // encrypted key_id
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
            enc_merkle_path,
            enc_note,
            enc_anchor,
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
                    merkle_path: self.decrypt_optional_blob(enc_merkle_path)?,
                    note: self.decrypt_optional_blob(enc_note)?,
                    anchor: self.decrypt_optional_blob(enc_anchor)?,
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
    use tempfile::NamedTempFile;

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
            merkle_path: Some(b"merkle_path_data".to_vec()),
            note: Some(b"serialized_note_data".to_vec()),
            anchor: None,
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
        assert_eq!(retrieved.merkle_path, note.merkle_path);
        assert_eq!(retrieved.note, note.note);
        assert_eq!(retrieved.anchor, note.anchor);
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
            merkle_path: None,
            note: None,
            anchor: None,
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
}
