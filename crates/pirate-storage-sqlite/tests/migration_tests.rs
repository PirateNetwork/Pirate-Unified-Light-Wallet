//! Migration snapshot tests
//!
//! Tests database schema migrations with snapshot verification.

use pirate_storage_sqlite::migrations;
use rusqlite::Connection;
use tempfile::NamedTempFile;

#[test]
fn test_fresh_migration() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    // Run migrations
    migrations::run_migrations(&conn).unwrap();

    // Verify schema
    verify_schema_v1(&conn);
}

#[test]
fn test_migration_idempotency() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    // Run migrations twice
    migrations::run_migrations(&conn).unwrap();
    migrations::run_migrations(&conn).unwrap();

    // Should still have correct schema
    verify_schema_v1(&conn);
}

#[test]
fn test_schema_version_tracking() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Verify version table exists and has correct version
    let version: i32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(version > 0, "Schema version should be tracked");
}

#[test]
fn test_accounts_table_structure() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Test table exists and can insert
    conn.execute(
        "INSERT INTO accounts (name, created_at) VALUES ('Test', 1234567890)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn test_notes_table_structure() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Create account first
    conn.execute(
        "INSERT INTO accounts (name, created_at) VALUES ('Test', 1234567890)",
        [],
    )
    .unwrap();

    // Insert note
    conn.execute(
        "INSERT INTO notes (account_id, value, nullifier, commitment, spent, height) VALUES (1, 100000000, X'00', X'00', 0, 1000)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn test_checkpoints_table_structure() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Insert checkpoint
    conn.execute(
        "INSERT INTO checkpoints (height, hash, timestamp, sapling_tree_size) VALUES (1000, 'hash123', 1234567890, 500)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM checkpoints", [], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn test_foreign_key_constraints() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Enable foreign keys
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

    // Try to insert note for non-existent account
    let result = conn.execute(
        "INSERT INTO notes (account_id, value, nullifier, commitment, spent, height) VALUES (999, 100000000, X'00', X'00', 0, 1000)",
        [],
    );

    // Should fail due to foreign key constraint
    assert!(result.is_err());
}

#[test]
fn test_unique_constraints() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Insert note with specific nullifier
    conn.execute(
        "INSERT INTO accounts (name, created_at) VALUES ('Test', 1234567890)",
        [],
    )
    .unwrap();

    let nullifier = vec![1u8; 32];
    conn.execute(
        "INSERT INTO notes (account_id, value, nullifier, commitment, spent, height) VALUES (1, 100000000, ?1, X'00', 0, 1000)",
        [&nullifier],
    )
    .unwrap();

    // Try to insert another note with same nullifier
    let result = conn.execute(
        "INSERT INTO notes (account_id, value, nullifier, commitment, spent, height) VALUES (1, 100000000, ?1, X'00', 0, 1001)",
        [&nullifier],
    );

    // Should fail due to unique constraint
    assert!(result.is_err());
}

#[test]
fn test_indexes_exist() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    // Check if indexes exist
    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(indexes.contains(&"idx_notes_account".to_string()));
    assert!(indexes.contains(&"idx_notes_spent".to_string()));
    assert!(indexes.contains(&"idx_addresses_account".to_string()));
    assert!(indexes.contains(&"idx_transactions_height".to_string()));
}

fn verify_schema_v1(conn: &Connection) {
    // Verify all tables exist
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(tables.contains(&"accounts".to_string()));
    assert!(tables.contains(&"addresses".to_string()));
    assert!(tables.contains(&"notes".to_string()));
    assert!(tables.contains(&"transactions".to_string()));
    assert!(tables.contains(&"memos".to_string()));
    assert!(tables.contains(&"checkpoints".to_string()));
    assert!(tables.contains(&"schema_version".to_string()));
}
