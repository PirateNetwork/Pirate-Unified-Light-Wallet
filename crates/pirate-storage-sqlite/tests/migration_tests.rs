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
fn test_v32_adds_retained_checkpoint_tables_without_resetting_trees() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    conn.execute_batch(
        "DROP TABLE sapling_tree_retained_checkpoints;
         DROP TABLE orchard_tree_retained_checkpoints;
         DELETE FROM schema_version;
         INSERT INTO schema_version (version) VALUES (31);
         INSERT INTO sapling_tree_checkpoints (checkpoint_id, position)
         VALUES (4078000, 12345);
         INSERT INTO orchard_tree_checkpoints (checkpoint_id, position)
         VALUES (4078000, 67890);",
    )
    .unwrap();

    migrations::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    let sapling_checkpoint: i64 = conn
        .query_row(
            "SELECT position FROM sapling_tree_checkpoints WHERE checkpoint_id = 4078000",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let orchard_checkpoint: i64 = conn
        .query_row(
            "SELECT position FROM orchard_tree_checkpoints WHERE checkpoint_id = 4078000",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let retained_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN (
                   'sapling_tree_retained_checkpoints',
                   'orchard_tree_retained_checkpoints'
               )",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(version, 40);
    assert_eq!(sapling_checkpoint, 12345);
    assert_eq!(orchard_checkpoint, 67890);
    assert_eq!(retained_tables, 2);
}

#[test]
fn test_v33_adds_durable_outgoing_transaction_intents() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    conn.execute_batch(
        "DROP TABLE outgoing_transaction_intents;
         DELETE FROM schema_version;
         INSERT INTO schema_version (version) VALUES (32);",
    )
    .unwrap();

    migrations::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'outgoing_transaction_intents'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(version, 40);
    assert_eq!(table_count, 1);
}

#[test]
fn test_v34_adds_ironwood_activation_height_to_sync_state() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();

    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY);
         INSERT INTO schema_version (version) VALUES (33);
         CREATE TABLE migration_state (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE addresses (
             id INTEGER PRIMARY KEY,
             account_id INTEGER NOT NULL,
             key_id INTEGER,
             diversifier_index INTEGER NOT NULL,
             address TEXT NOT NULL UNIQUE,
             address_type TEXT NOT NULL,
             address_scope TEXT NOT NULL
         );
         CREATE TABLE sync_state (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             local_height INTEGER NOT NULL DEFAULT 0,
             target_height INTEGER NOT NULL DEFAULT 0,
             last_checkpoint_height INTEGER NOT NULL DEFAULT 0,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE spendability_state (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             spendable INTEGER NOT NULL DEFAULT 0,
             rescan_required INTEGER NOT NULL DEFAULT 1,
             target_height INTEGER NOT NULL DEFAULT 0,
             anchor_height INTEGER NOT NULL DEFAULT 0,
             validated_anchor_height INTEGER NOT NULL DEFAULT 0,
             repair_queued INTEGER NOT NULL DEFAULT 0,
             repair_from_height INTEGER NOT NULL DEFAULT 0,
             reason_code TEXT NOT NULL DEFAULT 'ERR_RESCAN_REQUIRED',
             updated_at TEXT NOT NULL
         );
         INSERT INTO spendability_state (id, updated_at)
         VALUES (1, datetime('now'));
         INSERT INTO sync_state (
             id, local_height, target_height, last_checkpoint_height, updated_at
         ) VALUES (1, 100, 200, 90, datetime('now'));",
    )
    .unwrap();

    migrations::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    let activation_height: Option<i64> = conn
        .query_row(
            "SELECT ironwood_activation_height FROM sync_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let migration_marker: String = conn
        .query_row(
            "SELECT value FROM migration_state WHERE key = 'v34_ironwood_activation_height'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(version, 40);
    assert_eq!(activation_height, None);
    assert_eq!(migration_marker, "completed");
}

#[test]
fn test_v35_adds_address_display_preferences_without_changing_addresses() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (name, created_at) VALUES ('Test', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO addresses (
             account_id, diversifier_index, address, address_type, created_at
         ) VALUES (1, 7, 'zs1-test-address', 'Sapling', 2)",
        [],
    )
    .unwrap();
    let address_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO address_display_preferences (
             address_id, is_pinned, is_archived
         ) VALUES (?1, 1, 0)",
        [address_id],
    )
    .unwrap();

    let address_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM addresses", [], |row| row.get(0))
        .unwrap();
    let preferences: (i64, i64) = conn
        .query_row(
            "SELECT is_pinned, is_archived
             FROM address_display_preferences
             WHERE address_id = ?1",
            [address_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(address_count, 1);
    assert_eq!(preferences, (1, 0));
}

#[test]
fn test_v36_adds_seed_derived_account_provenance() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'seed_derived_account_keys'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let marker: String = conn
        .query_row(
            "SELECT value FROM migration_state
             WHERE key = 'v36_seed_derived_account_keys'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table_count, 1);
    assert_eq!(marker, "completed");
}

#[test]
fn test_v37_adds_durable_verified_key_rescan_requirement() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    conn.execute(
        "UPDATE spendability_state SET required_rescan_from_height = 123, key_import_generation = 7 WHERE id = 1",
        [],
    )
    .unwrap();

    // A restart/idempotent migration pass must preserve the pending replay.
    migrations::run_migrations(&conn).unwrap();

    let state: (i64, i64) = conn
        .query_row(
            "SELECT required_rescan_from_height, key_import_generation FROM spendability_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let marker: String = conn
        .query_row(
            "SELECT value FROM migration_state WHERE key = 'v37_verified_key_rescan_requirement'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(state, (123, 7));
    assert_eq!(marker, "completed");
}

#[test]
fn test_v38_adds_ordered_full_diversifier_indices() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY);
         INSERT INTO schema_version (version) VALUES (37);
         CREATE TABLE migration_state (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE addresses (
             id INTEGER PRIMARY KEY,
             account_id INTEGER NOT NULL,
             key_id INTEGER,
             diversifier_index INTEGER NOT NULL,
             address TEXT NOT NULL UNIQUE,
             address_type TEXT NOT NULL,
             address_scope TEXT NOT NULL
         );",
    )
    .unwrap();

    migrations::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    let marker: String = conn
        .query_row(
            "SELECT value FROM migration_state WHERE key = 'v38_full_diversifier_indices'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 40);
    assert_eq!(marker, "completed");

    let lower = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let higher = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0];
    conn.execute(
        "INSERT INTO addresses (
             account_id, key_id, diversifier_index, address, address_type,
             address_scope, diversifier_index_be
         ) VALUES (1, 7, 0, 'zs1-v38-low', 'Sapling', 'external', ?1)",
        [&lower],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO addresses (
             account_id, key_id, diversifier_index, address, address_type,
             address_scope, diversifier_index_be
         ) VALUES (1, 7, 1, 'zs1-v38-high', 'Sapling', 'external', ?1)",
        [&higher],
    )
    .unwrap();
    let maximum: Vec<u8> = conn
        .query_row(
            "SELECT MAX(diversifier_index_be) FROM addresses",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(maximum, higher);

    assert!(conn
        .execute(
            "INSERT INTO addresses (
                 account_id, key_id, diversifier_index, address, address_type,
                 address_scope, diversifier_index_be
             ) VALUES (1, 7, 2, 'zs1-v38-short', 'Sapling', 'external', ?1)",
            [vec![0u8; 10]],
        )
        .is_err());
    assert!(conn
        .execute(
            "INSERT INTO addresses (
                 account_id, key_id, diversifier_index, address, address_type,
                 address_scope, diversifier_index_be
             ) VALUES (1, 7, 3, 'zs1-v38-duplicate', 'Sapling', 'external', ?1)",
            [&lower],
        )
        .is_err());
}

#[test]
fn test_v39_adds_outgoing_transaction_expiry_height() {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    migrations::run_migrations(&conn).unwrap();

    conn.execute_batch(
        "DROP TABLE outgoing_transaction_intents;
         CREATE TABLE outgoing_transaction_intents (
             txid TEXT PRIMARY KEY,
             account_id BLOB NOT NULL,
             amount BLOB NOT NULL,
             fee BLOB NOT NULL,
             broadcast_at INTEGER NOT NULL
         );
         DELETE FROM schema_version;
         INSERT INTO schema_version (version) VALUES (38);",
    )
    .unwrap();

    migrations::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    let expiry_column_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('outgoing_transaction_intents')
             WHERE name = 'expiry_height'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let marker: String = conn
        .query_row(
            "SELECT value FROM migration_state
             WHERE key = 'v39_outgoing_transaction_expiry'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(version, 40);
    assert_eq!(expiry_column_count, 1);
    assert_eq!(marker, "completed");
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
    assert!(tables.contains(&"signing_key_protection".to_string()));
    assert!(tables.contains(&"schema_version".to_string()));
}
