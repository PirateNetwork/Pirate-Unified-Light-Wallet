//! Rolling checkpoint system for corruption recovery
//!
//! Maintains checkpoints at regular intervals to allow automatic rollback
//! on database corruption or sync interruption.

use crate::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Checkpoint data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Checkpoint ID
    pub id: i64,
    /// Block height
    pub height: u32,
    /// Block hash
    pub hash: String,
    /// Timestamp
    pub timestamp: i64,
    /// Sapling tree size
    pub sapling_tree_size: u32,
    /// Created at
    pub created_at: i64,
}

/// Checkpoint manager
pub struct CheckpointManager<'a> {
    conn: &'a Connection,
}

impl<'a> CheckpointManager<'a> {
    /// Create new checkpoint manager
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create checkpoint tables if not exists
    pub fn init_tables(&self) -> Result<()> {
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                height INTEGER NOT NULL UNIQUE,
                hash TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                sapling_tree_size INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_checkpoints_height ON checkpoints(height)",
            [],
        )?;

        Ok(())
    }

    /// Create new checkpoint
    pub fn create_checkpoint(
        &self,
        height: u32,
        hash: String,
        sapling_tree_size: u32,
    ) -> Result<i64> {
        let timestamp = chrono::Utc::now().timestamp();
        
        self.conn.execute(
            r#"
            INSERT INTO checkpoints (height, hash, timestamp, sapling_tree_size, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            rusqlite::params![
                height as i64,
                hash.as_str(),
                timestamp.to_string(),
                sapling_tree_size as i64,
                timestamp.to_string(),
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get latest checkpoint
    pub fn get_latest(&self) -> Result<Option<Checkpoint>> {
        let result = self.conn.query_row(
            "SELECT id, height, hash, timestamp, sapling_tree_size, created_at FROM checkpoints ORDER BY height DESC LIMIT 1",
            [],
            |row| {
                Ok(Checkpoint {
                    id: row.get(0)?,
                    height: row.get::<_, i64>(1)? as u32,
                    hash: row.get(2)?,
                    timestamp: row.get(3)?,
                    sapling_tree_size: row.get::<_, i64>(4)? as u32,
                    created_at: row.get(5)?,
                })
            },
        );

        match result {
            Ok(cp) => Ok(Some(cp)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get checkpoint at or before height
    pub fn get_at_height(&self, height: u32) -> Result<Option<Checkpoint>> {
        let result = self.conn.query_row(
            "SELECT id, height, hash, timestamp, sapling_tree_size, created_at FROM checkpoints WHERE height <= ?1 ORDER BY height DESC LIMIT 1",
            [height as i64],
            |row| {
                Ok(Checkpoint {
                    id: row.get(0)?,
                    height: row.get::<_, i64>(1)? as u32,
                    hash: row.get(2)?,
                    timestamp: row.get(3)?,
                    sapling_tree_size: row.get::<_, i64>(4)? as u32,
                    created_at: row.get(5)?,
                })
            },
        );

        match result {
            Ok(cp) => Ok(Some(cp)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Rollback to checkpoint
    pub fn rollback_to_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        tracing::info!(
            "Rolling back to checkpoint at height {}",
            checkpoint.height
        );

        // Begin transaction
        let tx = self.conn.unchecked_transaction()?;

        // Delete all notes above checkpoint height
        tx.execute(
            "DELETE FROM notes WHERE height > ?1",
            [checkpoint.height as i64],
        )?;

        // Delete all transactions above checkpoint height
        tx.execute(
            "DELETE FROM transactions WHERE height > ?1",
            [checkpoint.height as i64],
        )?;

        // Delete checkpoints above this one
        tx.execute(
            "DELETE FROM checkpoints WHERE height > ?1",
            [checkpoint.height as i64],
        )?;

        tx.commit()?;

        tracing::info!("Rollback completed successfully");
        Ok(())
    }

    /// Prune old checkpoints (keep only last N)
    pub fn prune_old_checkpoints(&self, keep_count: u32) -> Result<usize> {
        let deleted = self.conn.execute(
            r#"
            DELETE FROM checkpoints
            WHERE id NOT IN (
                SELECT id FROM checkpoints
                ORDER BY height DESC
                LIMIT ?1
            )
            "#,
            [keep_count as i64],
        )?;

        Ok(deleted)
    }

    /// Count checkpoints
    pub fn count(&self) -> Result<u32> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM checkpoints",
            [],
            |row| row.get(0),
        )?;

        Ok(count as u32)
    }

    /// Auto-rollback on corruption detected
    pub fn auto_rollback_on_corruption(&self) -> Result<bool> {
        // Check for corruption indicators
        if self.detect_corruption()? {
            if let Some(checkpoint) = self.get_latest()? {
                self.rollback_to_checkpoint(&checkpoint)?;
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    /// Detect database corruption
    fn detect_corruption(&self) -> Result<bool> {
        // Run integrity check
        let result: String = self.conn.query_row(
            "PRAGMA integrity_check",
            [],
            |row| row.get(0),
        )?;

        Ok(result != "ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup_test_db() -> Connection {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        
        // Create required tables
        conn.execute(
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, height INTEGER)",
            [],
        ).unwrap();
        
        conn.execute(
            "CREATE TABLE transactions (id INTEGER PRIMARY KEY, height INTEGER)",
            [],
        ).unwrap();
        
        conn
    }

    #[test]
    fn test_checkpoint_creation() {
        let conn = setup_test_db();
        let manager = CheckpointManager::new(&conn);
        
        manager.init_tables().unwrap();
        
        let id = manager
            .create_checkpoint(1000, "hash123".to_string(), 500)
            .unwrap();
        
        assert!(id > 0);
    }

    #[test]
    fn test_get_latest_checkpoint() {
        let conn = setup_test_db();
        let manager = CheckpointManager::new(&conn);
        
        manager.init_tables().unwrap();
        
        manager.create_checkpoint(1000, "hash1".to_string(), 500).unwrap();
        manager.create_checkpoint(2000, "hash2".to_string(), 1000).unwrap();
        
        let latest = manager.get_latest().unwrap().unwrap();
        assert_eq!(latest.height, 2000);
    }

    #[test]
    fn test_rollback_to_checkpoint() {
        let conn = setup_test_db();
        let manager = CheckpointManager::new(&conn);
        
        manager.init_tables().unwrap();
        
        // Create checkpoints
        manager.create_checkpoint(1000, "hash1".to_string(), 500).unwrap();
        let cp2 = manager.create_checkpoint(2000, "hash2".to_string(), 1000).unwrap();
        
        // Add some data above checkpoint
        conn.execute("INSERT INTO notes (height) VALUES (2100)", []).unwrap();
        conn.execute("INSERT INTO notes (height) VALUES (2200)", []).unwrap();
        
        // Rollback
        let checkpoint = manager.get_at_height(2000).unwrap().unwrap();
        manager.rollback_to_checkpoint(&checkpoint).unwrap();
        
        // Verify data above checkpoint is deleted
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes WHERE height > 2000", [], |row| {
                row.get(0)
            })
            .unwrap();
        
        assert_eq!(count, 0);
    }

    #[test]
    fn test_prune_old_checkpoints() {
        let conn = setup_test_db();
        let manager = CheckpointManager::new(&conn);
        
        manager.init_tables().unwrap();
        
        // Create 5 checkpoints
        for i in 1..=5 {
            manager
                .create_checkpoint(i * 1000, format!("hash{}", i), i * 500)
                .unwrap();
        }
        
        // Keep only 3
        manager.prune_old_checkpoints(3).unwrap();
        
        let count = manager.count().unwrap();
        assert_eq!(count, 3);
        
        // Latest should still be there
        let latest = manager.get_latest().unwrap().unwrap();
        assert_eq!(latest.height, 5000);
    }

    #[test]
    fn test_get_at_height() {
        let conn = setup_test_db();
        let manager = CheckpointManager::new(&conn);
        
        manager.init_tables().unwrap();
        
        manager.create_checkpoint(1000, "hash1".to_string(), 500).unwrap();
        manager.create_checkpoint(2000, "hash2".to_string(), 1000).unwrap();
        manager.create_checkpoint(3000, "hash3".to_string(), 1500).unwrap();
        
        // Get at height 2500 should return checkpoint at 2000
        let cp = manager.get_at_height(2500).unwrap().unwrap();
        assert_eq!(cp.height, 2000);
    }
}

