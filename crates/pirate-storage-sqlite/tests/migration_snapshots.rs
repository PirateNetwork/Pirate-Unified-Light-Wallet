//! Snapshot tests for database migrations
//!
//! Ensures migrations are idempotent and produce consistent schema

use pirate_storage_sqlite::{Database, Result};
use rusqlite::Connection;
use tempfile::TempDir;

/// Helper to get schema as sorted string for comparison
fn get_schema_snapshot(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, sql FROM sqlite_master 
         WHERE sql NOT NULL 
         ORDER BY type, name"
    )?;
    
    let rows = stmt.query_map([], |row| {
        Ok(format!(
            "{}: {} -- {}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?
        ))
    })?;
    
    let mut schema = Vec::new();
    for row in rows {
        schema.push(row?);
    }
    
    Ok(schema.join("\n"))
}

/// Helper to get table row counts
fn get_table_counts(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    )?;
    
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    
    let mut counts = Vec::new();
    for table in tables {
        let count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {}", table),
            [],
            |row| row.get(0),
        )?;
        counts.push(format!("{}: {}", table, count));
    }
    
    Ok(counts.join("\n"))
}

#[test]
fn test_fresh_migration_schema_snapshot() -> Result<()> {
    // Create fresh database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    let db = Database::open(&db_path)?;
    let conn = Connection::open(&db_path)?;
    
    // Get schema snapshot
    let schema = get_schema_snapshot(&conn)?;
    
    // Verify expected tables exist
    assert!(schema.contains("wallets"));
    assert!(schema.contains("transactions"));
    assert!(schema.contains("notes"));
    assert!(schema.contains("checkpoints"));
    
    // Print for manual inspection (can use insta crate for automated snapshots)
    println!("=== Fresh Migration Schema ===\n{}", schema);
    
    Ok(())
}

#[test]
fn test_migration_idempotency() -> Result<()> {
    // Create database and run migrations twice
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    // First migration
    let _db1 = Database::open(&db_path)?;
    let conn1 = Connection::open(&db_path)?;
    let schema1 = get_schema_snapshot(&conn1)?;
    drop(conn1);
    drop(_db1);
    
    // Second migration (reopen existing database)
    let _db2 = Database::open(&db_path)?;
    let conn2 = Connection::open(&db_path)?;
    let schema2 = get_schema_snapshot(&conn2)?;
    
    // Schema should be identical
    assert_eq!(schema1, schema2, "Migrations should be idempotent");
    
    Ok(())
}

#[test]
fn test_migration_preserves_data() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    // Create database and insert test data
    {
        let db = Database::open(&db_path)?;
        let conn = Connection::open(&db_path)?;
        
        // Insert test wallet
        conn.execute(
            "INSERT INTO wallets (id, name, created_at, watch_only, birthday_height) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("test-wallet-id", "Test Wallet", 1234567890i64, false, 1000000),
        )?;
        
        // Insert test checkpoint
        conn.execute(
            "INSERT INTO checkpoints (wallet_id, height, created_at) 
             VALUES (?1, ?2, ?3)",
            ("test-wallet-id", 1000000, 1234567890i64),
        )?;
    }
    
    // Reopen database (simulating migration on existing data)
    {
        let _db = Database::open(&db_path)?;
        let conn = Connection::open(&db_path)?;
        
        // Verify data preserved
        let wallet_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallets WHERE id = ?1",
            ["test-wallet-id"],
            |row| row.get(0),
        )?;
        assert_eq!(wallet_count, 1);
        
        let checkpoint_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM checkpoints WHERE wallet_id = ?1",
            ["test-wallet-id"],
            |row| row.get(0),
        )?;
        assert_eq!(checkpoint_count, 1);
        
        println!("=== Data Counts After Migration ===\n{}", get_table_counts(&conn)?);
    }
    
    Ok(())
}

#[test]
fn test_migration_version_tracking() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    let _db = Database::open(&db_path)?;
    let conn = Connection::open(&db_path)?;
    
    // Check if migration version tracking exists
    let version: i64 = conn.query_row(
        "SELECT user_version FROM pragma_user_version",
        [],
        |row| row.get(0),
    )?;
    
    // Should have version > 0 after migrations
    assert!(version > 0, "Migration version should be tracked");
    
    println!("Current migration version: {}", version);
    
    Ok(())
}

#[test]
fn test_rollback_checkpoint_schema() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    let _db = Database::open(&db_path)?;
    let conn = Connection::open(&db_path)?;
    
    // Verify checkpoint table has correct schema
    let schema: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='checkpoints'",
        [],
        |row| row.get(0),
    )?;
    
    // Should have required columns
    assert!(schema.contains("wallet_id"));
    assert!(schema.contains("height"));
    assert!(schema.contains("created_at"));
    
    println!("=== Checkpoint Table Schema ===\n{}", schema);
    
    Ok(())
}

#[test]
fn test_wal_mode_enabled() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    let _db = Database::open(&db_path)?;
    let conn = Connection::open(&db_path)?;
    
    // Check if WAL mode is enabled
    let journal_mode: String = conn.query_row(
        "PRAGMA journal_mode",
        [],
        |row| row.get(0),
    )?;
    
    assert_eq!(journal_mode.to_uppercase(), "WAL", "Database should use WAL mode");
    
    Ok(())
}

#[test]
fn test_foreign_keys_enabled() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    let _db = Database::open(&db_path)?;
    let conn = Connection::open(&db_path)?;
    
    // Check if foreign keys are enabled
    let fk_enabled: bool = conn.query_row(
        "PRAGMA foreign_keys",
        [],
        |row| row.get(0),
    )?;
    
    assert!(fk_enabled, "Foreign keys should be enabled");
    
    Ok(())
}

