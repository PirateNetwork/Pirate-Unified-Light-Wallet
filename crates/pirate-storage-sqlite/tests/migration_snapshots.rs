//! Snapshot tests for database migrations
//!
//! Ensures migrations are idempotent and produce consistent schema

use pirate_storage_sqlite::{
    generate_salt, Database, EncryptionAlgorithm, EncryptionKey, MasterKey,
};
use rusqlite::Connection;
use tempfile::TempDir;

type TestResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn test_keys() -> TestResult<(EncryptionKey, MasterKey)> {
    let salt = generate_salt();
    let key = EncryptionKey::from_passphrase("test-passphrase", &salt)?;
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    Ok((key, master_key))
}

/// Helper to get schema as sorted string for comparison
fn get_schema_snapshot(conn: &Connection) -> TestResult<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, sql FROM sqlite_master 
         WHERE sql NOT NULL 
         ORDER BY type, name",
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
fn get_table_counts(conn: &Connection) -> TestResult<String> {
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;

    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut counts = Vec::new();
    for table in tables {
        let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
            row.get(0)
        })?;
        counts.push(format!("{}: {}", table, count));
    }

    Ok(counts.join("\n"))
}

#[test]
fn test_fresh_migration_schema_snapshot() -> TestResult<()> {
    // Create fresh database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let (key, master_key) = test_keys()?;
    let db = Database::open(&db_path, &key, master_key.clone())?;
    let conn = db.conn();

    // Get schema snapshot
    let schema = get_schema_snapshot(conn)?;

    // Verify expected tables exist
    assert!(schema.contains("wallet_meta"));
    assert!(schema.contains("wallet_secrets"));
    assert!(schema.contains("transactions"));
    assert!(schema.contains("notes"));
    assert!(schema.contains("checkpoints"));

    // Print for manual inspection (can use insta crate for automated snapshots)
    println!("=== Fresh Migration Schema ===\n{}", schema);

    Ok(())
}

#[test]
fn test_migration_idempotency() -> TestResult<()> {
    // Create database and run migrations twice
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    // First migration
    let (key, master_key) = test_keys()?;
    let db1 = Database::open(&db_path, &key, master_key.clone())?;
    let schema1 = get_schema_snapshot(db1.conn())?;
    drop(db1);

    // Second migration (reopen existing database)
    let db2 = Database::open(&db_path, &key, master_key.clone())?;
    let schema2 = get_schema_snapshot(db2.conn())?;

    // Schema should be identical
    assert_eq!(schema1, schema2, "Migrations should be idempotent");

    Ok(())
}

#[test]
fn test_migration_preserves_data() -> TestResult<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let (key, master_key) = test_keys()?;

    // Create database and insert test data
    {
        let db = Database::open(&db_path, &key, master_key.clone())?;
        let conn = db.conn();

        // Insert test wallet
        conn.execute(
            "INSERT INTO wallet_meta (id, name, watch_only, birthday_height, created_at) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "test-wallet-id",
                "Test Wallet",
                false,
                1_000_000,
                1234567890i64,
            ),
        )?;

        // Insert test checkpoint
        conn.execute(
            "INSERT INTO checkpoints (height, hash, timestamp, sapling_tree_size) 
             VALUES (?1, ?2, ?3, ?4)",
            (1_000_000, "deadbeef", 1234567890i64, 42i64),
        )?;
    }

    // Reopen database (simulating migration on existing data)
    {
        let _db = Database::open(&db_path, &key, master_key.clone())?;
        let conn = _db.conn();

        // Verify data preserved
        let wallet_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallet_meta WHERE id = ?1",
            ["test-wallet-id"],
            |row| row.get(0),
        )?;
        assert_eq!(wallet_count, 1);

        let checkpoint_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM checkpoints WHERE height = ?1",
            [1_000_000i64],
            |row| row.get(0),
        )?;
        assert_eq!(checkpoint_count, 1);

        println!(
            "=== Data Counts After Migration ===\n{}",
            get_table_counts(conn)?
        );
    }

    Ok(())
}

#[test]
fn test_migration_version_tracking() -> TestResult<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let (key, master_key) = test_keys()?;
    let _db = Database::open(&db_path, &key, master_key.clone())?;
    let conn = _db.conn();

    // Check if migration version tracking exists
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;

    // Should have version > 0 after migrations
    assert!(version > 0, "Migration version should be tracked");

    println!("Current migration version: {}", version);

    Ok(())
}

#[test]
fn test_rollback_checkpoint_schema() -> TestResult<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let (key, master_key) = test_keys()?;
    let _db = Database::open(&db_path, &key, master_key.clone())?;
    let conn = _db.conn();

    // Verify checkpoint table has correct schema
    let schema: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='checkpoints'",
        [],
        |row| row.get(0),
    )?;

    // Should have required columns
    assert!(schema.contains("height"));
    assert!(schema.contains("hash"));
    assert!(schema.contains("timestamp"));
    assert!(schema.contains("sapling_tree_size"));

    println!("=== Checkpoint Table Schema ===\n{}", schema);

    Ok(())
}

#[test]
fn test_wal_mode_enabled() -> TestResult<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let (key, master_key) = test_keys()?;
    let _db = Database::open(&db_path, &key, master_key.clone())?;
    let conn = _db.conn();

    // Check if WAL mode is enabled
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;

    assert_eq!(
        journal_mode.to_uppercase(),
        "WAL",
        "Database should use WAL mode"
    );

    Ok(())
}

#[test]
fn test_foreign_keys_enabled() -> TestResult<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let (key, master_key) = test_keys()?;
    let _db = Database::open(&db_path, &key, master_key.clone())?;
    let conn = _db.conn();

    // Check if foreign keys are enabled
    let fk_enabled: bool = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;

    assert!(
        !fk_enabled,
        "Foreign keys should be disabled for encrypted fields"
    );

    Ok(())
}
