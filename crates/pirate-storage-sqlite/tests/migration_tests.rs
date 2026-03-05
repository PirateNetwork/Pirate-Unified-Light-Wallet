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
        "INSERT INTO notes (account_id, note_type, value, nullifier, commitment, spent, height, txid, output_index) VALUES (1, 'Sapling', 100000000, X'00', X'00', 0, 1000, X'01', 0)",
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

    // Try to insert address for non-existent account
    let result = conn.execute(
        "INSERT INTO addresses (account_id, diversifier_index, address) VALUES (999, 0, 'zs_missing_account')",
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

    // Insert address with specific value
    conn.execute(
        "INSERT INTO accounts (name, created_at) VALUES ('Test', 1234567890)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO addresses (account_id, diversifier_index, address) VALUES (1, 0, 'zs_unique_addr')",
        [],
    )
    .unwrap();

    // Try to insert another row with same address
    let result = conn.execute(
        "INSERT INTO addresses (account_id, diversifier_index, address) VALUES (1, 1, 'zs_unique_addr')",
        [],
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

#[test]
fn test_v25_notes_table_drops_legacy_witness_columns() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    let mut stmt = conn.prepare("PRAGMA table_info(notes)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(
        !columns.iter().any(|col| col == "merkle_path"),
        "legacy Sapling witness column should not exist in canonical notes schema"
    );
    assert!(
        !columns.iter().any(|col| col == "anchor"),
        "legacy Orchard anchor column should not exist in canonical notes schema"
    );
}

#[test]
fn test_v28_spendability_state_forces_rescan() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    let row: (i64, i64, i64, i64, i64, i64, String) = conn
        .query_row(
            r#"
            SELECT
                spendable,
                rescan_required,
                target_height,
                anchor_height,
                validated_anchor_height,
                repair_queued,
                reason_code
            FROM spendability_state
            WHERE id = 1
            "#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, 0, "spendable must be false after canonical rewrite");
    assert_eq!(
        row.1, 1,
        "rescan_required must be true after canonical rewrite"
    );
    assert_eq!(row.2, 0, "target_height must reset to 0");
    assert_eq!(row.3, 0, "anchor_height must reset to 0");
    assert_eq!(row.4, 0, "validated_anchor_height must reset to 0");
    assert_eq!(row.5, 0, "repair_queued must reset to 0");
    assert_eq!(
        row.6, "ERR_RESCAN_REQUIRED",
        "reason_code must deterministically gate spending until rescan"
    );
}

#[test]
fn test_v28_migration_markers_record_completion() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    migrations::run_migrations(&conn).unwrap();

    let marker: String = conn
        .query_row(
            "SELECT value FROM migration_state WHERE key = 'v28_position_shard_views'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marker, "completed");
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
