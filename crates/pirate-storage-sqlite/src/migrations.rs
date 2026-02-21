//! Database schema migrations

use crate::{Error, Result};
use rusqlite::Connection;

const SCHEMA_VERSION: i32 = 19;

/// Run all migrations
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current_version = get_schema_version(conn)?;

    tracing::debug!(
        "Running migrations: current_version={}, target_version={}",
        current_version,
        SCHEMA_VERSION
    );

    if current_version < 1 {
        migrate_v1(conn)?;
    }

    if current_version < 2 {
        migrate_v2(conn)?;
    }

    if current_version < 3 {
        migrate_v3(conn)?;
    }

    if current_version < 4 {
        migrate_v4(conn)?;
    }

    if current_version < 5 {
        migrate_v5(conn)?;
    }

    if current_version < 6 {
        migrate_v6(conn)?;
    }

    if current_version < 7 {
        migrate_v7(conn)?;
    }

    if current_version < 8 {
        migrate_v8(conn)?;
    }

    if current_version < 9 {
        migrate_v9(conn)?;
    }

    if current_version < 10 {
        migrate_v10(conn)?;
    }

    if current_version < 11 {
        migrate_v11(conn)?;
    }

    if current_version < 12 {
        migrate_v12(conn)?;
    }

    if current_version < 13 {
        migrate_v13(conn)?;
    }

    if current_version < 14 {
        migrate_v14(conn)?;
    }

    if current_version < 15 {
        migrate_v15(conn)?;
    }

    if current_version < 16 {
        migrate_v16(conn)?;
    }
    if current_version < 17 {
        migrate_v17(conn)?;
    }
    if current_version < 18 {
        migrate_v18(conn)?;
    }
    if current_version < 19 {
        migrate_v19(conn)?;
    }

    // Only set schema version if it changed (to avoid UNIQUE constraint errors)
    let final_version = get_schema_version(conn)?;
    if final_version != SCHEMA_VERSION {
        set_schema_version(conn, SCHEMA_VERSION)?;
    } else {
        tracing::debug!(
            "Schema version already at target {}, skipping set_schema_version",
            SCHEMA_VERSION
        );
    }

    Ok(())
}

fn get_schema_version(conn: &Connection) -> Result<i32> {
    let result = conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    );

    match result {
        Ok(v) => Ok(v),
        Err(_) => Ok(0),
    }
}

fn set_schema_version(conn: &Connection, version: i32) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
        [],
    )?;

    // Use INSERT OR IGNORE to safely handle case where version already exists
    // This is idempotent and prevents UNIQUE constraint errors
    match conn.execute(
        "INSERT OR IGNORE INTO schema_version (version) VALUES (?1)",
        [version],
    ) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                tracing::debug!("Inserted schema version {}", version);
            } else {
                tracing::debug!("Schema version {} already exists, skipped insert", version);
            }
            Ok(())
        }
        Err(e) => {
            // If INSERT OR IGNORE fails for some reason, try to check if it exists
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM schema_version WHERE version = ?1)",
                    [version],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            if exists {
                tracing::debug!(
                    "Schema version {} already exists (verified after insert failure)",
                    version
                );
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE accounts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE addresses (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            diversifier_index INTEGER NOT NULL,
            address TEXT NOT NULL UNIQUE,
            FOREIGN KEY (account_id) REFERENCES accounts(id)
        );

        CREATE TABLE notes (
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL,
            value INTEGER NOT NULL,
            nullifier BLOB NOT NULL UNIQUE,
            commitment BLOB NOT NULL,
            spent BOOLEAN NOT NULL DEFAULT 0,
            height INTEGER NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id)
        );

        CREATE TABLE transactions (
            id INTEGER PRIMARY KEY,
            txid TEXT NOT NULL UNIQUE,
            height INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            fee INTEGER NOT NULL
        );

        CREATE TABLE memos (
            id INTEGER PRIMARY KEY,
            tx_id INTEGER NOT NULL,
            memo BLOB NOT NULL,
            FOREIGN KEY (tx_id) REFERENCES transactions(id)
        );

        CREATE TABLE checkpoints (
            height INTEGER PRIMARY KEY,
            hash TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            sapling_tree_size INTEGER NOT NULL
        );

        CREATE INDEX idx_notes_account ON notes(account_id);
        CREATE INDEX idx_notes_spent ON notes(spent);
        CREATE INDEX idx_addresses_account ON addresses(account_id);
        CREATE INDEX idx_transactions_height ON transactions(height);
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Sapling commitment tree frontier snapshots
        CREATE TABLE frontier_snapshots (
            height INTEGER PRIMARY KEY,
            frontier BLOB NOT NULL,
            created_at TEXT NOT NULL,
            app_version TEXT NOT NULL
        );

        CREATE INDEX idx_frontier_snapshots_created ON frontier_snapshots(created_at);
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Sync state tracking
        CREATE TABLE sync_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            local_height INTEGER NOT NULL DEFAULT 0,
            target_height INTEGER NOT NULL DEFAULT 0,
            last_checkpoint_height INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );

        -- Initialize with default row
        INSERT INTO sync_state (id, local_height, target_height, last_checkpoint_height, updated_at)
        VALUES (1, 0, 0, 0, datetime('now'));
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Decoy vault configuration for panic PIN
        CREATE TABLE decoy_vault_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled BOOLEAN NOT NULL DEFAULT 0,
            panic_pin_hash TEXT,
            panic_pin_salt BLOB,
            decoy_wallet_name TEXT NOT NULL DEFAULT 'Wallet',
            created_at INTEGER NOT NULL,
            last_activated INTEGER,
            activation_count INTEGER NOT NULL DEFAULT 0
        );

        -- Address book for external contacts
        CREATE TABLE IF NOT EXISTS address_book (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            address TEXT NOT NULL UNIQUE,
            label TEXT NOT NULL,
            notes TEXT,
            color TEXT NOT NULL DEFAULT '"Grey"',
            created_at TEXT NOT NULL,
            last_used TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_address_book_label ON address_book(label);
        CREATE INDEX IF NOT EXISTS idx_address_book_address ON address_book(address);

        -- Wallet metadata with watch-only flag
        CREATE TABLE IF NOT EXISTS wallet_meta (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            watch_only BOOLEAN NOT NULL DEFAULT 0,
            birthday_height INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            ivk_fingerprint TEXT,
            encrypted_seed BLOB,
            encrypted_ivk BLOB
        );

        -- Seed export audit log
        CREATE TABLE seed_export_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallet_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            device_info TEXT,
            biometric_used BOOLEAN NOT NULL DEFAULT 0,
            result TEXT NOT NULL,
            FOREIGN KEY (wallet_id) REFERENCES wallet_meta(id)
        );

        CREATE INDEX idx_seed_export_log_wallet ON seed_export_log(wallet_id);
        CREATE INDEX idx_seed_export_log_timestamp ON seed_export_log(timestamp);
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v5(conn: &Connection) -> Result<()> {
    // Add Sapling spend metadata needed for building transactions:
    // txid, output index, diversifier, merkle path, and serialized note bytes.
    conn.execute_batch(
        r#"
        ALTER TABLE notes ADD COLUMN txid BLOB NOT NULL DEFAULT x'';
        ALTER TABLE notes ADD COLUMN output_index INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE notes ADD COLUMN diversifier BLOB NOT NULL DEFAULT x'';
        ALTER TABLE notes ADD COLUMN merkle_path BLOB NOT NULL DEFAULT x'';
        ALTER TABLE notes ADD COLUMN note BLOB NOT NULL DEFAULT x'';
        ALTER TABLE notes ADD COLUMN memo BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v6(conn: &Connection) -> Result<()> {
    // Wallet secrets (encrypted spending keys)
    conn.execute_batch(
        r#"
        CREATE TABLE wallet_secrets (
            wallet_id TEXT PRIMARY KEY,
            account_id INTEGER NOT NULL,
            extsk BLOB NOT NULL,
            dfvk BLOB,
            created_at INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v7(conn: &Connection) -> Result<()> {
    // Sync operation logs for diagnostics
    conn.execute_batch(
        r#"
        CREATE TABLE sync_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallet_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            level TEXT NOT NULL CHECK (level IN ('DEBUG', 'INFO', 'WARN', 'ERROR')),
            module TEXT NOT NULL,
            message TEXT NOT NULL
        );

        CREATE INDEX idx_sync_logs_wallet ON sync_logs(wallet_id);
        CREATE INDEX idx_sync_logs_timestamp ON sync_logs(timestamp DESC);
        CREATE INDEX idx_sync_logs_level ON sync_logs(level);
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v8(conn: &Connection) -> Result<()> {
    // Add address_type column to addresses table
    // Add orchard_extsk column to wallet_secrets table
    conn.execute_batch(
        r#"
        -- Add address_type column to addresses (default to Sapling for existing addresses)
        ALTER TABLE addresses ADD COLUMN address_type TEXT NOT NULL DEFAULT 'Sapling' CHECK (address_type IN ('Sapling', 'Orchard'));
        
        -- Add orchard_extsk column to wallet_secrets (optional, for Orchard keys)
        ALTER TABLE wallet_secrets ADD COLUMN orchard_extsk BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v9(conn: &Connection) -> Result<()> {
    // Add Orchard note support to notes table
    // SQLite doesn't support DROP COLUMN, so we'll make fields nullable and add new ones
    conn.execute_batch(
        r#"
        -- Add note_type column (default to Sapling for existing notes)
        ALTER TABLE notes ADD COLUMN note_type TEXT NOT NULL DEFAULT 'Sapling' CHECK (note_type IN ('Sapling', 'Orchard'));
        
        -- Make Sapling-specific fields nullable (they were NOT NULL before, but we'll allow NULL for Orchard notes)
        -- SQLite doesn't support changing NOT NULL constraint, so we'll handle this in application code
        -- The existing columns remain, but we'll allow NULL values for Orchard notes
        
        -- Add Orchard-specific fields
        ALTER TABLE notes ADD COLUMN anchor BLOB;
        ALTER TABLE notes ADD COLUMN position INTEGER;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v10(conn: &Connection) -> Result<()> {
    // Add IVK support for watch-only wallets
    conn.execute_batch(
        r#"
        -- Add Sapling IVK column (32 bytes) for watch-only wallets
        ALTER TABLE wallet_secrets ADD COLUMN sapling_ivk BLOB;
        
        -- Add Orchard IVK column (64 bytes) for watch-only wallets
        ALTER TABLE wallet_secrets ADD COLUMN orchard_ivk BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v11(conn: &Connection) -> Result<()> {
    // Add label column to addresses table for address book
    conn.execute_batch(
        r#"
        -- Add label column to addresses (optional, for address book)
        ALTER TABLE addresses ADD COLUMN label TEXT;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v12(conn: &Connection) -> Result<()> {
    // Add encrypted_mnemonic column to wallet_secrets table
    conn.execute_batch(
        r#"
        -- Add encrypted_mnemonic column (optional, only for wallets created/restored from seed)
        ALTER TABLE wallet_secrets ADD COLUMN encrypted_mnemonic BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v13(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(address_book)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // No address_book table yet; create the new schema.
    if columns.is_empty() {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS address_book (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                address TEXT NOT NULL,
                label TEXT NOT NULL,
                notes TEXT,
                color_tag INTEGER NOT NULL DEFAULT 0,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_used_at TEXT,
                use_count INTEGER NOT NULL DEFAULT 0,
                UNIQUE(wallet_id, address)
            );

            CREATE INDEX IF NOT EXISTS idx_address_book_wallet ON address_book(wallet_id);
            CREATE INDEX IF NOT EXISTS idx_address_book_label ON address_book(wallet_id, label);
            CREATE INDEX IF NOT EXISTS idx_address_book_favorite ON address_book(wallet_id, is_favorite);
            "#,
        )?;
        return Ok(());
    }

    // Already upgraded.
    if columns.iter().any(|c| c == "wallet_id") {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        ALTER TABLE address_book RENAME TO address_book_legacy;

        CREATE TABLE address_book (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallet_id TEXT NOT NULL,
            address TEXT NOT NULL,
            label TEXT NOT NULL,
            notes TEXT,
            color_tag INTEGER NOT NULL DEFAULT 0,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_used_at TEXT,
            use_count INTEGER NOT NULL DEFAULT 0,
            UNIQUE(wallet_id, address)
        );

        INSERT INTO address_book (
            wallet_id,
            address,
            label,
            notes,
            color_tag,
            is_favorite,
            created_at,
            updated_at,
            last_used_at,
            use_count
        )
        SELECT
            'legacy',
            address,
            label,
            notes,
            CASE LOWER(color)
                WHEN 'red' THEN 1
                WHEN 'orange' THEN 2
                WHEN 'yellow' THEN 3
                WHEN 'green' THEN 4
                WHEN 'blue' THEN 5
                WHEN 'purple' THEN 6
                WHEN 'pink' THEN 7
                WHEN 'gray' THEN 8
                WHEN 'grey' THEN 8
                ELSE 0
            END,
            0,
            created_at,
            created_at,
            last_used,
            0
        FROM address_book_legacy;

        DROP TABLE address_book_legacy;

        CREATE INDEX IF NOT EXISTS idx_address_book_wallet ON address_book(wallet_id);
        CREATE INDEX IF NOT EXISTS idx_address_book_label ON address_book(wallet_id, label);
        CREATE INDEX IF NOT EXISTS idx_address_book_favorite ON address_book(wallet_id, is_favorite);
        "#,
    )?;

    Ok(())
}

fn migrate_v14(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Track address creation time and optional color tags
        ALTER TABLE addresses ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE addresses ADD COLUMN color_tag INTEGER NOT NULL DEFAULT 0;

        UPDATE addresses
        SET created_at = strftime('%s','now')
        WHERE created_at = 0;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v15(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Track spending transaction for notes (encrypted bytes)
        ALTER TABLE notes ADD COLUMN spent_txid BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v16(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Store additional key sources per wallet account
        CREATE TABLE IF NOT EXISTS account_keys (
            account_id INTEGER PRIMARY KEY,
            key_type TEXT NOT NULL CHECK (key_type IN ('seed', 'import_spend', 'import_view')),
            key_scope TEXT NOT NULL CHECK (key_scope IN ('account', 'single_address')),
            label TEXT,
            birthday_height INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            spendable BOOLEAN NOT NULL DEFAULT 0,
            sapling_extsk BLOB,
            sapling_dfvk BLOB,
            orchard_extsk BLOB,
            orchard_fvk BLOB,
            encrypted_mnemonic BLOB
        );

        CREATE INDEX IF NOT EXISTS idx_account_keys_spendable ON account_keys(spendable);
        CREATE INDEX IF NOT EXISTS idx_account_keys_type ON account_keys(key_type);

        -- Track if addresses are external (receive) or internal (change/consolidation)
        ALTER TABLE addresses ADD COLUMN address_scope TEXT NOT NULL DEFAULT 'external'
            CHECK (address_scope IN ('external', 'internal'));

        -- Link notes to address rows (encrypted identifier)
        ALTER TABLE notes ADD COLUMN address_id BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v17(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Allow multiple key sources per account (add surrogate key)
        CREATE TABLE IF NOT EXISTS account_keys_v2 (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            key_type TEXT NOT NULL CHECK (key_type IN ('seed', 'import_spend', 'import_view')),
            key_scope TEXT NOT NULL CHECK (key_scope IN ('account', 'single_address')),
            label TEXT,
            birthday_height INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            spendable BOOLEAN NOT NULL DEFAULT 0,
            sapling_extsk BLOB,
            sapling_dfvk BLOB,
            orchard_extsk BLOB,
            orchard_fvk BLOB,
            encrypted_mnemonic BLOB
        );

        INSERT INTO account_keys_v2 (
            account_id,
            key_type,
            key_scope,
            label,
            birthday_height,
            created_at,
            spendable,
            sapling_extsk,
            sapling_dfvk,
            orchard_extsk,
            orchard_fvk,
            encrypted_mnemonic
        )
        SELECT
            account_id,
            key_type,
            key_scope,
            label,
            birthday_height,
            created_at,
            spendable,
            sapling_extsk,
            sapling_dfvk,
            orchard_extsk,
            orchard_fvk,
            encrypted_mnemonic
        FROM account_keys;

        DROP TABLE IF EXISTS account_keys;
        ALTER TABLE account_keys_v2 RENAME TO account_keys;

        CREATE INDEX IF NOT EXISTS idx_account_keys_account ON account_keys(account_id);
        CREATE INDEX IF NOT EXISTS idx_account_keys_spendable ON account_keys(spendable);
        CREATE INDEX IF NOT EXISTS idx_account_keys_type ON account_keys(key_type);

        -- Track which key group an address belongs to
        ALTER TABLE addresses ADD COLUMN key_id INTEGER;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v18(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Track which key group a note belongs to (encrypted)
        ALTER TABLE notes ADD COLUMN key_id BLOB;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v19(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Nullifier spends observed before corresponding notes are discovered.
        -- This mirrors upstream "unlinked nullifier" behavior for both Sapling and Orchard.
        CREATE TABLE IF NOT EXISTS unlinked_spend_nullifiers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id BLOB NOT NULL,
            note_type TEXT NOT NULL CHECK (note_type IN ('Sapling', 'Orchard')),
            nullifier BLOB NOT NULL,
            spending_txid BLOB NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_unlinked_spends_note_type
            ON unlinked_spend_nullifiers(note_type);
        CREATE INDEX IF NOT EXISTS idx_unlinked_spends_created_at
            ON unlinked_spend_nullifiers(created_at);
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}
