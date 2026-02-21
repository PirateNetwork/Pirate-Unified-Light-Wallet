//! Sync state storage with retry/backoff for SQLITE_BUSY
//!
//! Provides atomic operations for sync state tracking with
//! automatic retry logic for database contention.

use crate::{Database, Error, Result};
use rusqlite::{params, types::ValueRef, ErrorCode, OptionalExtension, Transaction};
use std::thread;
use std::time::Duration;

/// Maximum retry attempts for SQLITE_BUSY
pub const MAX_BUSY_RETRIES: u32 = 5;

/// Base backoff duration in milliseconds
pub const BASE_BACKOFF_MS: u64 = 50;

/// Maximum backoff duration in milliseconds
pub const MAX_BACKOFF_MS: u64 = 1000;

/// Sync state record
#[derive(Debug, Clone)]
pub struct SyncStateRow {
    /// Current local height
    pub local_height: u64,
    /// Target height from network
    pub target_height: u64,
    /// Last checkpoint height
    pub last_checkpoint_height: u64,
    /// Last update timestamp (ISO 8601)
    pub updated_at: String,
}

impl Default for SyncStateRow {
    fn default() -> Self {
        Self {
            local_height: 0,
            target_height: 0,
            last_checkpoint_height: 0,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Sync state storage operations with retry logic
pub struct SyncStateStorage<'a> {
    db: &'a Database,
}

impl<'a> SyncStateStorage<'a> {
    /// Create new sync state storage
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Save sync state with retry on SQLITE_BUSY
    pub fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
        let local_height = to_sql_i64(local_height)?;
        let target_height = to_sql_i64(target_height)?;
        let last_checkpoint_height = to_sql_i64(last_checkpoint_height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.execute_with_retry(|| {
            self.db.conn().execute(
                r#"
                UPDATE sync_state SET
                    local_height = ?1,
                    target_height = ?2,
                    last_checkpoint_height = ?3,
                    updated_at = ?4
                WHERE id = 1
                "#,
                params![
                    local_height,
                    target_height,
                    last_checkpoint_height,
                    updated_at
                ],
            )?;
            Ok(())
        })
    }

    /// Save sync state within a transaction
    pub fn save_sync_state_tx(
        tx: &Transaction<'_>,
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
        let local_height = to_sql_i64(local_height)?;
        let target_height = to_sql_i64(target_height)?;
        let last_checkpoint_height = to_sql_i64(last_checkpoint_height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        tx.execute(
            r#"
            UPDATE sync_state SET
                local_height = ?1,
                target_height = ?2,
                last_checkpoint_height = ?3,
                updated_at = ?4
            WHERE id = 1
            "#,
            params![
                local_height,
                target_height,
                last_checkpoint_height,
                updated_at
            ],
        )?;

        Ok(())
    }

    /// Load current sync state with retry on SQLITE_BUSY
    pub fn load_sync_state(&self) -> Result<SyncStateRow> {
        self.query_with_retry(|| {
            let row = self
                .db
                .conn()
                .query_row(
                    r#"
                    SELECT local_height, target_height, last_checkpoint_height, updated_at
                    FROM sync_state
                    WHERE id = 1
                    "#,
                    [],
                    |row| {
                        let local_height_i64: i64 = row.get(0)?;
                        let target_height_i64: i64 = row.get(1)?;
                        let last_checkpoint_height_i64: i64 = row.get(2)?;
                        Ok(SyncStateRow {
                            local_height: u64::try_from(local_height_i64).map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(0, local_height_i64)
                            })?,
                            target_height: u64::try_from(target_height_i64).map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(1, target_height_i64)
                            })?,
                            last_checkpoint_height: u64::try_from(last_checkpoint_height_i64)
                                .map_err(|_| {
                                    rusqlite::Error::IntegralValueOutOfRange(
                                        2,
                                        last_checkpoint_height_i64,
                                    )
                                })?,
                            updated_at: row.get(3)?,
                        })
                    },
                )
                .optional()?;

            Ok(row.unwrap_or_default())
        })
    }

    /// Reset sync state (for rescan)
    pub fn reset_sync_state(&self, start_height: u64) -> Result<()> {
        let start_height = to_sql_i64(start_height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.execute_with_retry(|| {
            self.db.conn().execute(
                r#"
                UPDATE sync_state SET
                    local_height = ?1,
                    target_height = 0,
                    last_checkpoint_height = ?1,
                    updated_at = ?2
                WHERE id = 1
                "#,
                params![start_height, updated_at],
            )?;
            Ok(())
        })
    }

    /// Update only local height (for incremental progress)
    pub fn update_local_height(&self, height: u64) -> Result<()> {
        let height = to_sql_i64(height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.execute_with_retry(|| {
            self.db.conn().execute(
                "UPDATE sync_state SET local_height = ?1, updated_at = ?2 WHERE id = 1",
                params![height, updated_at],
            )?;
            Ok(())
        })
    }

    /// Update target height
    pub fn update_target_height(&self, height: u64) -> Result<()> {
        let height = to_sql_i64(height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.execute_with_retry(|| {
            self.db.conn().execute(
                "UPDATE sync_state SET target_height = ?1, updated_at = ?2 WHERE id = 1",
                params![height, updated_at],
            )?;
            Ok(())
        })
    }

    /// Update last checkpoint height
    pub fn update_checkpoint_height(&self, height: u64) -> Result<()> {
        let height = to_sql_i64(height)?;
        let updated_at = chrono::Utc::now().to_rfc3339();

        self.execute_with_retry(|| {
            self.db.conn().execute(
                "UPDATE sync_state SET last_checkpoint_height = ?1, updated_at = ?2 WHERE id = 1",
                params![height, updated_at],
            )?;
            Ok(())
        })
    }

    /// Execute with retry logic for SQLITE_BUSY
    fn execute_with_retry<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let mut attempts = 0;

        loop {
            match f() {
                Ok(()) => return Ok(()),
                Err(Error::Database(ref e)) if is_busy_error(e) && attempts < MAX_BUSY_RETRIES => {
                    attempts += 1;
                    let backoff = calculate_backoff(attempts);
                    tracing::debug!(
                        "SQLITE_BUSY (attempt {}/{}), retrying in {}ms",
                        attempts,
                        MAX_BUSY_RETRIES,
                        backoff
                    );
                    thread::sleep(Duration::from_millis(backoff));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Query with retry logic for SQLITE_BUSY
    fn query_with_retry<F, T>(&self, mut f: F) -> Result<T>
    where
        F: FnMut() -> Result<T>,
    {
        let mut attempts = 0;

        loop {
            match f() {
                Ok(result) => return Ok(result),
                Err(Error::Database(ref e)) if is_busy_error(e) && attempts < MAX_BUSY_RETRIES => {
                    attempts += 1;
                    let backoff = calculate_backoff(attempts);
                    tracing::debug!(
                        "SQLITE_BUSY (attempt {}/{}), retrying in {}ms",
                        attempts,
                        MAX_BUSY_RETRIES,
                        backoff
                    );
                    thread::sleep(Duration::from_millis(backoff));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Check if error is SQLITE_BUSY
fn is_busy_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseBusy,
                ..
            },
            _
        )
    )
}

/// Calculate exponential backoff with jitter
fn calculate_backoff(attempt: u32) -> u64 {
    let base = BASE_BACKOFF_MS * (1 << attempt.min(6));
    let jitter = rand::random::<u64>() % (base / 4 + 1);
    (base + jitter).min(MAX_BACKOFF_MS)
}

/// Atomic sync state update (combines multiple fields in one transaction)
pub fn atomic_sync_update(
    db: &mut Database,
    local_height: u64,
    target_height: u64,
    last_checkpoint_height: u64,
    frontier_bytes: Option<&[u8]>,
    app_version: &str,
) -> Result<()> {
    let tx = db.transaction()?;

    // Update sync state
    SyncStateStorage::save_sync_state_tx(&tx, local_height, target_height, last_checkpoint_height)?;

    // Optionally save frontier snapshot
    if let Some(bytes) = frontier_bytes {
        crate::frontier::FrontierStorage::save_frontier_snapshot_tx(
            &tx,
            last_checkpoint_height as u32,
            bytes,
            app_version,
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Truncate data above a specific height (for rollback)
pub fn truncate_above_height(db: &mut Database, height: u64) -> Result<()> {
    let height = to_sql_i64(height)?;
    let master_key = db.master_key().clone();
    let tx = db.transaction()?;

    // Delete notes above height.
    //
    // NOTE: `notes.height` is stored encrypted in current schema. A direct
    // SQL comparison (`WHERE height > ?`) on encrypted bytes can behave
    // non-deterministically and wipe unrelated rows. We must decrypt each
    // height before deciding whether to prune.
    let mut note_ids_to_delete: Vec<i64> = Vec::new();
    {
        let mut stmt = tx.prepare("SELECT id, height FROM notes")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let note_id: i64 = row.get(0)?;
            let height_cell = row.get_ref(1)?;
            let note_height = match height_cell {
                ValueRef::Integer(v) => Some(v),
                ValueRef::Blob(bytes) => {
                    let decrypted = master_key.decrypt(bytes).map_err(|e| {
                        Error::Encryption(format!(
                            "Failed to decrypt note height for note id {}: {}",
                            note_id, e
                        ))
                    })?;
                    if decrypted.len() == 8 {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&decrypted);
                        Some(i64::from_le_bytes(arr))
                    } else {
                        tracing::warn!(
                            "Skipping note id {} during truncate: invalid decrypted height length {}",
                            note_id,
                            decrypted.len()
                        );
                        None
                    }
                }
                ValueRef::Null => None,
                ValueRef::Real(v) => Some(v as i64),
                ValueRef::Text(text) => std::str::from_utf8(text)
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok()),
            };

            if note_height.is_some_and(|h| h > height) {
                note_ids_to_delete.push(note_id);
            }
        }
    }
    if !note_ids_to_delete.is_empty() {
        let mut delete_stmt = tx.prepare("DELETE FROM notes WHERE id = ?1")?;
        for note_id in &note_ids_to_delete {
            delete_stmt.execute([note_id])?;
        }
    }

    // Delete transactions above height
    tx.execute("DELETE FROM transactions WHERE height > ?1", [height])?;

    // Delete checkpoints above height
    tx.execute("DELETE FROM checkpoints WHERE height > ?1", [height])?;

    // Delete frontier snapshots above height
    tx.execute("DELETE FROM frontier_snapshots WHERE height > ?1", [height])?;

    // Update sync state
    let updated_at = chrono::Utc::now().to_rfc3339();
    tx.execute(
        r#"
        UPDATE sync_state SET
            local_height = ?1,
            last_checkpoint_height = (
                SELECT COALESCE(MAX(height), 0) FROM frontier_snapshots WHERE height <= ?1
            ),
            updated_at = ?2
        WHERE id = 1
        "#,
        params![height, updated_at],
    )?;

    tx.commit()?;

    tracing::info!(
        "Truncated data above height {} (notes_deleted={})",
        height,
        note_ids_to_delete.len()
    );
    Ok(())
}

fn to_sql_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Storage(format!("Height exceeds i64 range: {}", value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::EncryptionKey;
    use tempfile::NamedTempFile;

    fn test_db() -> Database {
        use crate::security::{EncryptionAlgorithm, MasterKey};
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        Database::open(file.path(), &key, master_key).unwrap()
    }

    #[test]
    fn test_save_and_load_sync_state() {
        let db = test_db();
        let storage = SyncStateStorage::new(&db);

        storage.save_sync_state(1000, 2000, 500).unwrap();

        let state = storage.load_sync_state().unwrap();
        assert_eq!(state.local_height, 1000);
        assert_eq!(state.target_height, 2000);
        assert_eq!(state.last_checkpoint_height, 500);
    }

    #[test]
    fn test_reset_sync_state() {
        let db = test_db();
        let storage = SyncStateStorage::new(&db);

        storage.save_sync_state(1000, 2000, 500).unwrap();
        storage.reset_sync_state(100).unwrap();

        let state = storage.load_sync_state().unwrap();
        assert_eq!(state.local_height, 100);
        assert_eq!(state.target_height, 0);
        assert_eq!(state.last_checkpoint_height, 100);
    }

    #[test]
    fn test_update_individual_fields() {
        let db = test_db();
        let storage = SyncStateStorage::new(&db);

        storage.update_local_height(500).unwrap();
        storage.update_target_height(1000).unwrap();
        storage.update_checkpoint_height(400).unwrap();

        let state = storage.load_sync_state().unwrap();
        assert_eq!(state.local_height, 500);
        assert_eq!(state.target_height, 1000);
        assert_eq!(state.last_checkpoint_height, 400);
    }

    #[test]
    fn test_default_sync_state() {
        let db = test_db();
        let storage = SyncStateStorage::new(&db);

        let state = storage.load_sync_state().unwrap();
        assert_eq!(state.local_height, 0);
        assert_eq!(state.target_height, 0);
        assert_eq!(state.last_checkpoint_height, 0);
    }

    #[test]
    fn test_calculate_backoff() {
        let b1 = calculate_backoff(1);
        let b2 = calculate_backoff(2);
        let b3 = calculate_backoff(3);

        // Backoff should increase with attempts
        assert!(b1 < b2 || b2 < b3);
        // Should not exceed max
        assert!(calculate_backoff(10) <= MAX_BACKOFF_MS);
    }

    #[test]
    fn test_truncate_above_height() {
        let mut db = test_db();

        // The function should succeed even with empty tables
        truncate_above_height(&mut db, 1000).unwrap();

        let storage = SyncStateStorage::new(&db);
        let state = storage.load_sync_state().unwrap();
        assert_eq!(state.local_height, 1000);
    }
}
