//! Frontier snapshot storage
//!
//! Provides atomic storage and retrieval of Sapling commitment tree
//! frontier snapshots for checkpoint/rollback support.

use crate::{Database, Result};
use rusqlite::{params, OptionalExtension, Transaction};

/// Frontier snapshot record from database
#[derive(Debug, Clone)]
pub struct FrontierSnapshotRow {
    /// Block height
    pub height: u32,
    /// Serialized frontier bytes
    pub frontier: Vec<u8>,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
    /// App version that created this snapshot
    pub app_version: String,
}

/// Frontier snapshot storage operations
pub struct FrontierStorage<'a> {
    db: &'a Database,
}

impl<'a> FrontierStorage<'a> {
    /// Create new frontier storage
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Save a frontier snapshot (atomic) with encryption
    ///
    /// Replaces any existing snapshot at the same height.
    pub fn save_frontier_snapshot(
        &self,
        height: u32,
        frontier_bytes: &[u8],
        app_version: &str,
    ) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();

        // Encrypt frontier before storage
        let encrypted_frontier = self.db.master_key().encrypt(frontier_bytes)
            .map_err(|e| crate::Error::Encryption(format!("Failed to encrypt frontier: {}", e)))?;

        self.db.conn().execute(
            r#"
            INSERT OR REPLACE INTO frontier_snapshots (height, frontier, created_at, app_version)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![height, encrypted_frontier, created_at, app_version],
        )?;

        tracing::debug!("Saved encrypted frontier snapshot at height {}", height);
        Ok(())
    }

    /// Save frontier snapshot within a transaction (for atomicity with other operations)
    /// Note: This function requires encrypted frontier bytes because Transaction doesn't expose master_key.
    pub fn save_frontier_snapshot_tx(
        tx: &Transaction<'_>,
        height: u32,
        encrypted_frontier_bytes: &[u8],
        app_version: &str,
    ) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();

        tx.execute(
            r#"
            INSERT OR REPLACE INTO frontier_snapshots (height, frontier, created_at, app_version)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![height, encrypted_frontier_bytes, created_at, app_version],
        )?;

        tracing::debug!("Saved encrypted frontier snapshot at height {} (in transaction)", height);
        Ok(())
    }

    /// Load the most recent frontier snapshot with decryption
    ///
    /// Returns the snapshot with the highest height, or None if no snapshots exist.
    pub fn load_last_snapshot(&self) -> Result<Option<(u32, Vec<u8>)>> {
        let result = self
            .db
            .conn()
            .query_row(
                r#"
                SELECT height, frontier
                FROM frontier_snapshots
                ORDER BY height DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    let height: u32 = row.get(0)?;
                    let encrypted_frontier: Vec<u8> = row.get(1)?;
                    Ok((height, encrypted_frontier))
                },
            )
            .optional()?;

        if let Some((height, encrypted_frontier)) = &result {
            // Decrypt frontier
            let frontier = self.db.master_key().decrypt(encrypted_frontier)
                .map_err(|e| crate::Error::Encryption(format!("Failed to decrypt frontier: {}", e)))?;
            
            tracing::debug!("Loaded and decrypted frontier snapshot from height {}", height);
            Ok(Some((*height, frontier)))
        } else {
            Ok(None)
        }
    }

    /// Load the most recent frontier snapshot (full record) with decryption
    pub fn load_last_snapshot_full(&self) -> Result<Option<FrontierSnapshotRow>> {
        let result = self
            .db
            .conn()
            .query_row(
                r#"
                SELECT height, frontier, created_at, app_version
                FROM frontier_snapshots
                ORDER BY height DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    let height: u32 = row.get(0)?;
                    let encrypted_frontier: Vec<u8> = row.get(1)?;
                    Ok((height, encrypted_frontier, row.get(2)?, row.get(3)?))
                },
            )
            .optional()?;

        if let Some((height, encrypted_frontier, created_at, app_version)) = result {
            // Decrypt frontier
            let frontier = self.db.master_key().decrypt(&encrypted_frontier)
                .map_err(|e| crate::Error::Encryption(format!("Failed to decrypt frontier: {}", e)))?;
            
            Ok(Some(FrontierSnapshotRow {
                height,
                frontier,
                created_at,
                app_version,
            }))
        } else {
            Ok(None)
        }
    }

    /// Load frontier snapshot at specific height with decryption
    pub fn load_snapshot_at_height(&self, height: u32) -> Result<Option<Vec<u8>>> {
        let result = self
            .db
            .conn()
            .query_row(
                "SELECT frontier FROM frontier_snapshots WHERE height = ?1",
                [height],
                |row| {
                    let encrypted: Vec<u8> = row.get(0)?;
                    Ok(encrypted)
                },
            )
            .optional()?;

        if let Some(encrypted) = result {
            let frontier = self.db.master_key().decrypt(&encrypted)
                .map_err(|e| crate::Error::Encryption(format!("Failed to decrypt frontier: {}", e)))?;
            Ok(Some(frontier))
        } else {
            Ok(None)
        }
    }

    /// Load the closest snapshot at or below the given height with decryption
    ///
    /// Useful for rollback operations.
    pub fn load_snapshot_at_or_below(&self, height: u32) -> Result<Option<(u32, Vec<u8>)>> {
        let result = self
            .db
            .conn()
            .query_row(
                r#"
                SELECT height, frontier
                FROM frontier_snapshots
                WHERE height <= ?1
                ORDER BY height DESC
                LIMIT 1
                "#,
                [height],
                |row| {
                    let h: u32 = row.get(0)?;
                    let encrypted_frontier: Vec<u8> = row.get(1)?;
                    Ok((h, encrypted_frontier))
                },
            )
            .optional()?;

        if let Some((h, encrypted_frontier)) = result {
            let frontier = self.db.master_key().decrypt(&encrypted_frontier)
                .map_err(|e| crate::Error::Encryption(format!("Failed to decrypt frontier: {}", e)))?;
            Ok(Some((h, frontier)))
        } else {
            Ok(None)
        }
    }

    /// Delete snapshots above a certain height
    ///
    /// Useful during rollback to remove invalidated snapshots.
    pub fn delete_snapshots_above(&self, height: u32) -> Result<usize> {
        let count = self.db.conn().execute(
            "DELETE FROM frontier_snapshots WHERE height > ?1",
            [height],
        )?;

        if count > 0 {
            tracing::info!("Deleted {} frontier snapshots above height {}", count, height);
        }

        Ok(count)
    }

    /// Delete snapshots above height within a transaction
    pub fn delete_snapshots_above_tx(tx: &Transaction<'_>, height: u32) -> Result<usize> {
        let count = tx.execute(
            "DELETE FROM frontier_snapshots WHERE height > ?1",
            [height],
        )?;

        if count > 0 {
            tracing::info!("Deleted {} frontier snapshots above height {} (in transaction)", count, height);
        }

        Ok(count)
    }

    /// Get all snapshot heights (for debugging/diagnostics)
    pub fn list_snapshot_heights(&self) -> Result<Vec<u32>> {
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT height FROM frontier_snapshots ORDER BY height ASC")?;

        let heights = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<u32>, _>>()?;

        Ok(heights)
    }

    /// Count total snapshots
    pub fn count_snapshots(&self) -> Result<u64> {
        let count: u64 = self.db.conn().query_row(
            "SELECT COUNT(*) FROM frontier_snapshots",
            [],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Prune old snapshots, keeping only the most recent N
    pub fn prune_old_snapshots(&self, keep_count: usize) -> Result<usize> {
        let count = self.db.conn().execute(
            r#"
            DELETE FROM frontier_snapshots
            WHERE height NOT IN (
                SELECT height FROM frontier_snapshots
                ORDER BY height DESC
                LIMIT ?1
            )
            "#,
            [keep_count as i64],
        )?;

        if count > 0 {
            tracing::info!("Pruned {} old frontier snapshots, kept {}", count, keep_count);
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::EncryptionKey;
    use tempfile::NamedTempFile;

    fn test_db() -> Database {
        use crate::security::{MasterKey, EncryptionAlgorithm};
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        Database::open(file.path(), &key, master_key).unwrap()
    }

    #[test]
    fn test_save_and_load_snapshot() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        let frontier_bytes = vec![1, 2, 3, 4, 5];
        storage.save_frontier_snapshot(100, &frontier_bytes, "1.0.0").unwrap();

        let result = storage.load_last_snapshot().unwrap();
        assert!(result.is_some());

        let (height, loaded_bytes) = result.unwrap();
        assert_eq!(height, 100);
        assert_eq!(loaded_bytes, frontier_bytes);
    }

    #[test]
    fn test_load_last_gets_highest() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        storage.save_frontier_snapshot(100, &[1], "1.0.0").unwrap();
        storage.save_frontier_snapshot(200, &[2], "1.0.0").unwrap();
        storage.save_frontier_snapshot(150, &[3], "1.0.0").unwrap();

        let (height, bytes) = storage.load_last_snapshot().unwrap().unwrap();
        assert_eq!(height, 200);
        assert_eq!(bytes, vec![2]);
    }

    #[test]
    fn test_load_at_or_below() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        storage.save_frontier_snapshot(100, &[1], "1.0.0").unwrap();
        storage.save_frontier_snapshot(200, &[2], "1.0.0").unwrap();
        storage.save_frontier_snapshot(300, &[3], "1.0.0").unwrap();

        // Should get 200 when asking for 250
        let (height, _) = storage.load_snapshot_at_or_below(250).unwrap().unwrap();
        assert_eq!(height, 200);

        // Should get 100 when asking for 100
        let (height, _) = storage.load_snapshot_at_or_below(100).unwrap().unwrap();
        assert_eq!(height, 100);

        // Should get None when asking for 50
        let result = storage.load_snapshot_at_or_below(50).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_above() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        storage.save_frontier_snapshot(100, &[1], "1.0.0").unwrap();
        storage.save_frontier_snapshot(200, &[2], "1.0.0").unwrap();
        storage.save_frontier_snapshot(300, &[3], "1.0.0").unwrap();

        let deleted = storage.delete_snapshots_above(150).unwrap();
        assert_eq!(deleted, 2);

        let heights = storage.list_snapshot_heights().unwrap();
        assert_eq!(heights, vec![100]);
    }

    #[test]
    fn test_replace_existing() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        storage.save_frontier_snapshot(100, &[1], "1.0.0").unwrap();
        storage.save_frontier_snapshot(100, &[2], "1.0.1").unwrap();

        let count = storage.count_snapshots().unwrap();
        assert_eq!(count, 1);

        let full = storage.load_last_snapshot_full().unwrap().unwrap();
        assert_eq!(full.frontier, vec![2]);
        assert_eq!(full.app_version, "1.0.1");
    }

    #[test]
    fn test_prune_old() {
        let db = test_db();
        let storage = FrontierStorage::new(&db);

        for i in 1..=10 {
            storage.save_frontier_snapshot(i * 100, &[i as u8], "1.0.0").unwrap();
        }

        assert_eq!(storage.count_snapshots().unwrap(), 10);

        storage.prune_old_snapshots(3).unwrap();

        let heights = storage.list_snapshot_heights().unwrap();
        assert_eq!(heights, vec![800, 900, 1000]);
    }
}

