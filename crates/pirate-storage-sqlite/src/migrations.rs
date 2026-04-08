//! Database schema migrations

use crate::{Error, Result};
use rusqlite::Connection;

const SCHEMA_VERSION: i32 = 30;

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
    if current_version < 20 {
        migrate_v20(conn)?;
    }
    if current_version < 21 {
        migrate_v21(conn)?;
    }
    if current_version < 22 {
        migrate_v22(conn)?;
    }
    if current_version < 23 {
        migrate_v23(conn)?;
    }
    if current_version < 24 {
        migrate_v24(conn)?;
    }
    if current_version < 25 {
        migrate_v25(conn)?;
    }
    if current_version < 26 {
        migrate_v26(conn)?;
    }
    if current_version < 27 {
        migrate_v27(conn)?;
    }
    if current_version < 28 {
        migrate_v28(conn)?;
    }
    if current_version < 29 {
        migrate_v29(conn)?;
    }
    if current_version < 30 {
        migrate_v30(conn)?;
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

fn migrate_v30(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE wallet_secrets ADD COLUMN mnemonic_language TEXT;

        UPDATE wallet_secrets
        SET mnemonic_language = 'english'
        WHERE encrypted_mnemonic IS NOT NULL
          AND mnemonic_language IS NULL;
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
        -- This tracks "unlinked nullifier" behavior for both Sapling and Orchard.
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

fn migrate_v20(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Deterministic spendability/anchor-validation state used by send prechecks.
        CREATE TABLE IF NOT EXISTS spendability_state (
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

        INSERT OR IGNORE INTO spendability_state (
            id,
            spendable,
            rescan_required,
            target_height,
            anchor_height,
            validated_anchor_height,
            repair_queued,
            repair_from_height,
            reason_code,
            updated_at
        )
        VALUES (
            1,
            0,
            1,
            0,
            0,
            0,
            0,
            0,
            'ERR_RESCAN_REQUIRED',
            datetime('now')
        );

        -- Migration marker to make upgrade state explicit for diagnostics.
        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v20_spendability_refactor', 'applied', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;
        "#,
    )
    .map_err(|e| Error::Migration(e.to_string()))?;

    Ok(())
}

fn migrate_v21(conn: &Connection) -> Result<()> {
    // Internal-testing destructive reset of chain-derived state so all wallets
    // move onto deterministic spendability/witness validation through a full rescan.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|e| Error::Migration(e.to_string()))?;

    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v21_spendability_refactor_state', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        -- Chain-derived data only; wallet metadata and key material are preserved.
        DELETE FROM notes;
        DELETE FROM transactions;
        DELETE FROM memos;
        DELETE FROM unlinked_spend_nullifiers;
        DELETE FROM checkpoints;
        DELETE FROM frontier_snapshots;
        DELETE FROM sync_logs;

        UPDATE sync_state SET
            local_height = 0,
            target_height = 0,
            last_checkpoint_height = 0,
            updated_at = datetime('now')
        WHERE id = 1;

        UPDATE spendability_state SET
            spendable = 0,
            rescan_required = 1,
            target_height = 0,
            anchor_height = 0,
            validated_anchor_height = 0,
            repair_queued = 0,
            repair_from_height = 0,
            reason_code = 'ERR_RESCAN_REQUIRED',
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v21_spendability_refactor_state', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v22(conn: &Connection) -> Result<()> {
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS scan_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            range_start INTEGER NOT NULL,
            range_end INTEGER NOT NULL,
            priority INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'done')),
            reason TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_scan_queue_status_priority_start
            ON scan_queue(status, priority DESC, range_start ASC);
        CREATE INDEX IF NOT EXISTS idx_scan_queue_priority_end
            ON scan_queue(priority, range_end);

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v22_scan_queue', 'applied', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        -- Carry forward any previously queued repair into the scan queue.
        INSERT INTO scan_queue (
            range_start,
            range_end,
            priority,
            status,
            reason,
            created_at,
            updated_at
        )
        SELECT
            CASE
                WHEN repair_from_height > 0 THEN repair_from_height
                ELSE 1
            END,
            CASE
                WHEN target_height > 0 THEN target_height + 1
                WHEN anchor_height > 0 THEN anchor_height + 1
                ELSE 2
            END,
            40,
            'pending',
            'legacy_spendability_repair',
            datetime('now'),
            datetime('now')
        FROM spendability_state
        WHERE id = 1
          AND repair_queued = 1;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v23(conn: &Connection) -> Result<()> {
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Legacy per-note witness blobs are no longer canonical spendability inputs.
        -- Keep columns for compatibility with existing encrypted schema, but clear data.
        UPDATE notes
        SET merkle_path = NULL,
            anchor = NULL;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v23_disable_legacy_witness_blobs', 'applied', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v24(conn: &Connection) -> Result<()> {
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Canonicalize note storage by removing legacy per-note witness blobs.
        CREATE TABLE notes_v24 (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id BLOB NOT NULL,
            note_type TEXT NOT NULL CHECK (note_type IN ('Sapling', 'Orchard')),
            value BLOB NOT NULL,
            nullifier BLOB NOT NULL,
            commitment BLOB NOT NULL,
            spent BLOB NOT NULL,
            height BLOB NOT NULL,
            txid BLOB NOT NULL,
            output_index BLOB NOT NULL,
            spent_txid BLOB,
            diversifier BLOB,
            note BLOB,
            position BLOB,
            memo BLOB,
            address_id BLOB,
            key_id BLOB
        );

        INSERT INTO notes_v24 (
            id,
            account_id,
            note_type,
            value,
            nullifier,
            commitment,
            spent,
            height,
            txid,
            output_index,
            spent_txid,
            diversifier,
            note,
            position,
            memo,
            address_id,
            key_id
        )
        SELECT
            id,
            account_id,
            note_type,
            value,
            nullifier,
            commitment,
            spent,
            height,
            txid,
            output_index,
            spent_txid,
            diversifier,
            note,
            position,
            memo,
            address_id,
            key_id
        FROM notes;

        DROP TABLE notes;
        ALTER TABLE notes_v24 RENAME TO notes;

        CREATE INDEX IF NOT EXISTS idx_notes_account ON notes(account_id);
        CREATE INDEX IF NOT EXISTS idx_notes_spent ON notes(spent);

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v24_drop_legacy_note_witness_columns', 'applied', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v25(conn: &Connection) -> Result<()> {
    // Internal-test destructive canonicalization:
    // keep wallet metadata + key material, reset chain-derived state and queue/spendability
    // tables to deterministic defaults requiring a full rescan.
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v25_canonical_refactor', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        -- Rebuild scan queue to canonical shape.
        DROP TABLE IF EXISTS scan_queue;
        CREATE TABLE scan_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            range_start INTEGER NOT NULL CHECK (range_start > 0),
            range_end INTEGER NOT NULL CHECK (range_end > range_start),
            priority INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'done')),
            reason TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scan_queue_status_priority_start
            ON scan_queue(status, priority DESC, range_start ASC);
        CREATE INDEX IF NOT EXISTS idx_scan_queue_priority_end
            ON scan_queue(priority, range_end);

        -- Rebuild spendability state to canonical shape.
        DROP TABLE IF EXISTS spendability_state;
        CREATE TABLE spendability_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            spendable INTEGER NOT NULL DEFAULT 0 CHECK (spendable IN (0,1)),
            rescan_required INTEGER NOT NULL DEFAULT 1 CHECK (rescan_required IN (0,1)),
            target_height INTEGER NOT NULL DEFAULT 0 CHECK (target_height >= 0),
            anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (anchor_height >= 0),
            validated_anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (validated_anchor_height >= 0),
            repair_queued INTEGER NOT NULL DEFAULT 0 CHECK (repair_queued IN (0,1)),
            repair_from_height INTEGER NOT NULL DEFAULT 0 CHECK (repair_from_height >= 0),
            reason_code TEXT NOT NULL DEFAULT 'ERR_RESCAN_REQUIRED',
            updated_at TEXT NOT NULL
        );
        INSERT INTO spendability_state (
            id,
            spendable,
            rescan_required,
            target_height,
            anchor_height,
            validated_anchor_height,
            repair_queued,
            repair_from_height,
            reason_code,
            updated_at
        )
        VALUES (
            1,
            0,
            1,
            0,
            0,
            0,
            0,
            0,
            'ERR_RESCAN_REQUIRED',
            datetime('now')
        );

        -- Reset chain-derived state; wallet metadata / keys are preserved.
        DELETE FROM notes;
        DELETE FROM transactions;
        DELETE FROM memos;
        DELETE FROM unlinked_spend_nullifiers;
        DELETE FROM checkpoints;
        DELETE FROM frontier_snapshots;
        DELETE FROM sync_logs;

        UPDATE sync_state SET
            local_height = 0,
            target_height = 0,
            last_checkpoint_height = 0,
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v25_canonical_refactor', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v26(conn: &Connection) -> Result<()> {
    // Canonical spendability migration:
    // - preserve wallet metadata/secrets/settings
    // - rebuild canonical shard scan views
    // - enforce deterministic post-migration spendability gate
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v26_spendability_state', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        -- Canonical shard scan views.
        DROP VIEW IF EXISTS v_sapling_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_sapling_shard_scan_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_scan_ranges;

        CREATE VIEW v_sapling_shard_scan_ranges AS
        SELECT
            range_start AS block_range_start,
            range_end AS block_range_end,
            range_start AS subtree_start_height,
            (range_end - 1) AS subtree_end_height,
            priority,
            status,
            reason
        FROM scan_queue;

        CREATE VIEW v_sapling_shard_unscanned_ranges AS
        SELECT
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            priority,
            status,
            reason
        FROM v_sapling_shard_scan_ranges
        WHERE status IN ('pending', 'in_progress');

        CREATE VIEW v_orchard_shard_scan_ranges AS
        SELECT
            range_start AS block_range_start,
            range_end AS block_range_end,
            range_start AS subtree_start_height,
            (range_end - 1) AS subtree_end_height,
            priority,
            status,
            reason
        FROM scan_queue;

        CREATE VIEW v_orchard_shard_unscanned_ranges AS
        SELECT
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            priority,
            status,
            reason
        FROM v_orchard_shard_scan_ranges
        WHERE status IN ('pending', 'in_progress');

        -- Deterministic post-migration gating.
        UPDATE spendability_state SET
            spendable = 0,
            rescan_required = 1,
            target_height = 0,
            anchor_height = 0,
            validated_anchor_height = 0,
            repair_queued = 0,
            repair_from_height = 0,
            reason_code = 'ERR_RESCAN_REQUIRED',
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v26_spendability_state', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v27(conn: &Connection) -> Result<()> {
    // Canonical destructive rewrite for internal testing:
    // - preserve wallet metadata/secrets/settings
    // - reset chain-derived state
    // - rebuild queue/spendability/view structures to deterministic defaults
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|e| Error::Migration(e.to_string()))?;

    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        CREATE TABLE IF NOT EXISTS migration_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v27_canonical_schema_rewrite', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        DROP VIEW IF EXISTS v_sapling_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_sapling_shard_scan_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_scan_ranges;

        DROP TABLE IF EXISTS sapling_note_shards;
        DROP TABLE IF EXISTS orchard_note_shards;
        CREATE TABLE sapling_note_shards (
            shard_index INTEGER PRIMARY KEY,
            subtree_start_height INTEGER NOT NULL,
            subtree_end_height INTEGER,
            contains_marked INTEGER NOT NULL DEFAULT 1 CHECK (contains_marked IN (0,1))
        );
        CREATE TABLE orchard_note_shards (
            shard_index INTEGER PRIMARY KEY,
            subtree_start_height INTEGER NOT NULL,
            subtree_end_height INTEGER,
            contains_marked INTEGER NOT NULL DEFAULT 1 CHECK (contains_marked IN (0,1))
        );
        CREATE INDEX IF NOT EXISTS idx_sapling_note_shards_height
            ON sapling_note_shards(subtree_start_height, subtree_end_height);
        CREATE INDEX IF NOT EXISTS idx_orchard_note_shards_height
            ON orchard_note_shards(subtree_start_height, subtree_end_height);

        DROP TABLE IF EXISTS scan_queue;
        CREATE TABLE scan_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            range_start INTEGER NOT NULL CHECK (range_start > 0),
            range_end INTEGER NOT NULL CHECK (range_end > range_start),
            priority INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'done')),
            reason TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scan_queue_status_priority_start
            ON scan_queue(status, priority DESC, range_start ASC);
        CREATE INDEX IF NOT EXISTS idx_scan_queue_priority_end
            ON scan_queue(priority, range_end);

        DROP TABLE IF EXISTS spendability_state;
        CREATE TABLE spendability_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            spendable INTEGER NOT NULL DEFAULT 0 CHECK (spendable IN (0,1)),
            rescan_required INTEGER NOT NULL DEFAULT 1 CHECK (rescan_required IN (0,1)),
            target_height INTEGER NOT NULL DEFAULT 0 CHECK (target_height >= 0),
            anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (anchor_height >= 0),
            validated_anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (validated_anchor_height >= 0),
            repair_queued INTEGER NOT NULL DEFAULT 0 CHECK (repair_queued IN (0,1)),
            repair_from_height INTEGER NOT NULL DEFAULT 0 CHECK (repair_from_height >= 0),
            reason_code TEXT NOT NULL DEFAULT 'ERR_RESCAN_REQUIRED',
            updated_at TEXT NOT NULL
        );
        INSERT INTO spendability_state (
            id,
            spendable,
            rescan_required,
            target_height,
            anchor_height,
            validated_anchor_height,
            repair_queued,
            repair_from_height,
            reason_code,
            updated_at
        )
        VALUES (
            1,
            0,
            1,
            0,
            0,
            0,
            0,
            0,
            'ERR_RESCAN_REQUIRED',
            datetime('now')
        );

        CREATE VIEW v_sapling_shard_scan_ranges AS
        SELECT
            shard.shard_index,
            (shard.shard_index << 16) AS start_position,
            ((shard.shard_index + 1) << 16) AS end_position_exclusive,
            shard.subtree_start_height,
            shard.subtree_end_height,
            shard.contains_marked,
            scan_queue.range_start AS block_range_start,
            scan_queue.range_end AS block_range_end,
            scan_queue.priority,
            scan_queue.status,
            scan_queue.reason
        FROM sapling_note_shards shard
        INNER JOIN scan_queue
            ON shard.subtree_start_height < scan_queue.range_end
           AND (
                scan_queue.range_start <= shard.subtree_end_height
                OR shard.subtree_end_height IS NULL
           );

        CREATE VIEW v_sapling_shard_unscanned_ranges AS
        WITH wallet_birthday AS (SELECT MIN(birthday_height) AS height FROM account_keys)
        SELECT
            shard_index,
            start_position,
            end_position_exclusive,
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            contains_marked,
            priority,
            status,
            reason
        FROM v_sapling_shard_scan_ranges
        INNER JOIN wallet_birthday
        WHERE status IN ('pending', 'in_progress')
          AND block_range_end > wallet_birthday.height;

        CREATE VIEW v_orchard_shard_scan_ranges AS
        SELECT
            shard.shard_index,
            (shard.shard_index << 16) AS start_position,
            ((shard.shard_index + 1) << 16) AS end_position_exclusive,
            shard.subtree_start_height,
            shard.subtree_end_height,
            shard.contains_marked,
            scan_queue.range_start AS block_range_start,
            scan_queue.range_end AS block_range_end,
            scan_queue.priority,
            scan_queue.status,
            scan_queue.reason
        FROM orchard_note_shards shard
        INNER JOIN scan_queue
            ON shard.subtree_start_height < scan_queue.range_end
           AND (
                scan_queue.range_start <= shard.subtree_end_height
                OR shard.subtree_end_height IS NULL
           );

        CREATE VIEW v_orchard_shard_unscanned_ranges AS
        WITH wallet_birthday AS (SELECT MIN(birthday_height) AS height FROM account_keys)
        SELECT
            shard_index,
            start_position,
            end_position_exclusive,
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            contains_marked,
            priority,
            status,
            reason
        FROM v_orchard_shard_scan_ranges
        INNER JOIN wallet_birthday
        WHERE status IN ('pending', 'in_progress')
          AND block_range_end > wallet_birthday.height;

        -- Reset chain-derived state; wallet metadata/key material remain intact.
        DELETE FROM notes;
        DELETE FROM transactions;
        DELETE FROM memos;
        DELETE FROM unlinked_spend_nullifiers;
        DELETE FROM checkpoints;
        DELETE FROM frontier_snapshots;
        DELETE FROM sync_logs;
        DELETE FROM sapling_note_shards;
        DELETE FROM orchard_note_shards;

        UPDATE sync_state SET
            local_height = 0,
            target_height = 0,
            last_checkpoint_height = 0,
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v27_canonical_schema_rewrite', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v28(conn: &Connection) -> Result<()> {
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v28_position_shard_views', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        DROP VIEW IF EXISTS v_sapling_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_sapling_shard_scan_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_unscanned_ranges;
        DROP VIEW IF EXISTS v_orchard_shard_scan_ranges;

        DROP TABLE IF EXISTS sapling_note_shards;
        DROP TABLE IF EXISTS orchard_note_shards;
        CREATE TABLE sapling_note_shards (
            shard_index INTEGER PRIMARY KEY,
            start_position INTEGER NOT NULL CHECK (start_position >= 0),
            end_position_exclusive INTEGER NOT NULL CHECK (end_position_exclusive > start_position),
            subtree_start_height INTEGER NOT NULL,
            subtree_end_height INTEGER,
            contains_marked INTEGER NOT NULL DEFAULT 1 CHECK (contains_marked IN (0,1))
        );
        CREATE TABLE orchard_note_shards (
            shard_index INTEGER PRIMARY KEY,
            start_position INTEGER NOT NULL CHECK (start_position >= 0),
            end_position_exclusive INTEGER NOT NULL CHECK (end_position_exclusive > start_position),
            subtree_start_height INTEGER NOT NULL,
            subtree_end_height INTEGER,
            contains_marked INTEGER NOT NULL DEFAULT 1 CHECK (contains_marked IN (0,1))
        );
        CREATE INDEX IF NOT EXISTS idx_sapling_note_shards_position
            ON sapling_note_shards(start_position, end_position_exclusive);
        CREATE INDEX IF NOT EXISTS idx_orchard_note_shards_position
            ON orchard_note_shards(start_position, end_position_exclusive);
        CREATE INDEX IF NOT EXISTS idx_sapling_note_shards_height
            ON sapling_note_shards(subtree_start_height, subtree_end_height);
        CREATE INDEX IF NOT EXISTS idx_orchard_note_shards_height
            ON orchard_note_shards(subtree_start_height, subtree_end_height);

        DROP TABLE IF EXISTS scan_queue;
        CREATE TABLE scan_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            range_start INTEGER NOT NULL CHECK (range_start > 0),
            range_end INTEGER NOT NULL CHECK (range_end > range_start),
            priority INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'done')),
            reason TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scan_queue_status_priority_start
            ON scan_queue(status, priority DESC, range_start ASC);
        CREATE INDEX IF NOT EXISTS idx_scan_queue_priority_end
            ON scan_queue(priority, range_end);

        DROP TABLE IF EXISTS spendability_state;
        CREATE TABLE spendability_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            spendable INTEGER NOT NULL DEFAULT 0 CHECK (spendable IN (0,1)),
            rescan_required INTEGER NOT NULL DEFAULT 1 CHECK (rescan_required IN (0,1)),
            target_height INTEGER NOT NULL DEFAULT 0 CHECK (target_height >= 0),
            anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (anchor_height >= 0),
            validated_anchor_height INTEGER NOT NULL DEFAULT 0 CHECK (validated_anchor_height >= 0),
            repair_queued INTEGER NOT NULL DEFAULT 0 CHECK (repair_queued IN (0,1)),
            repair_from_height INTEGER NOT NULL DEFAULT 0 CHECK (repair_from_height >= 0),
            reason_code TEXT NOT NULL DEFAULT 'ERR_RESCAN_REQUIRED',
            updated_at TEXT NOT NULL
        );
        INSERT INTO spendability_state (
            id,
            spendable,
            rescan_required,
            target_height,
            anchor_height,
            validated_anchor_height,
            repair_queued,
            repair_from_height,
            reason_code,
            updated_at
        )
        VALUES (
            1,
            0,
            1,
            0,
            0,
            0,
            0,
            0,
            'ERR_RESCAN_REQUIRED',
            datetime('now')
        );

        CREATE VIEW v_sapling_shard_scan_ranges AS
        SELECT
            shard.shard_index,
            shard.start_position,
            shard.end_position_exclusive,
            shard.subtree_start_height,
            shard.subtree_end_height,
            shard.contains_marked,
            scan_queue.range_start AS block_range_start,
            scan_queue.range_end AS block_range_end,
            scan_queue.priority,
            scan_queue.status,
            scan_queue.reason
        FROM sapling_note_shards shard
        INNER JOIN scan_queue
            ON shard.subtree_start_height < scan_queue.range_end
           AND (
                scan_queue.range_start <= shard.subtree_end_height
                OR shard.subtree_end_height IS NULL
           );

        CREATE VIEW v_sapling_shard_unscanned_ranges AS
        WITH wallet_birthday AS (
            SELECT IFNULL(MIN(birthday_height), 0) AS height FROM account_keys
        )
        SELECT
            shard_index,
            start_position,
            end_position_exclusive,
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            contains_marked,
            priority,
            status,
            reason
        FROM v_sapling_shard_scan_ranges
        INNER JOIN wallet_birthday
        WHERE status IN ('pending', 'in_progress')
          AND block_range_end > wallet_birthday.height;

        CREATE VIEW v_orchard_shard_scan_ranges AS
        SELECT
            shard.shard_index,
            shard.start_position,
            shard.end_position_exclusive,
            shard.subtree_start_height,
            shard.subtree_end_height,
            shard.contains_marked,
            scan_queue.range_start AS block_range_start,
            scan_queue.range_end AS block_range_end,
            scan_queue.priority,
            scan_queue.status,
            scan_queue.reason
        FROM orchard_note_shards shard
        INNER JOIN scan_queue
            ON shard.subtree_start_height < scan_queue.range_end
           AND (
                scan_queue.range_start <= shard.subtree_end_height
                OR shard.subtree_end_height IS NULL
           );

        CREATE VIEW v_orchard_shard_unscanned_ranges AS
        WITH wallet_birthday AS (
            SELECT IFNULL(MIN(birthday_height), 0) AS height FROM account_keys
        )
        SELECT
            shard_index,
            start_position,
            end_position_exclusive,
            block_range_start,
            block_range_end,
            subtree_start_height,
            subtree_end_height,
            contains_marked,
            priority,
            status,
            reason
        FROM v_orchard_shard_scan_ranges
        INNER JOIN wallet_birthday
        WHERE status IN ('pending', 'in_progress')
          AND block_range_end > wallet_birthday.height;

        -- Reset chain-derived state; wallet metadata/key material remain intact.
        DELETE FROM notes;
        DELETE FROM transactions;
        DELETE FROM memos;
        DELETE FROM unlinked_spend_nullifiers;
        DELETE FROM checkpoints;
        DELETE FROM frontier_snapshots;
        DELETE FROM sync_logs;
        DELETE FROM sapling_note_shards;
        DELETE FROM orchard_note_shards;

        UPDATE sync_state SET
            local_height = 0,
            target_height = 0,
            last_checkpoint_height = 0,
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v28_position_shard_views', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}

fn migrate_v29(conn: &Connection) -> Result<()> {
    let migration_result = conn.execute_batch(
        r#"
        BEGIN IMMEDIATE;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v29_shardtree_store_tables', 'started', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        -- Canonical shardtree persistence (Sapling + Orchard), aligned with
        -- SqliteShardStore table naming conventions.
        CREATE TABLE IF NOT EXISTS sapling_tree_cap (
            cap_id INTEGER PRIMARY KEY,
            cap_data BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sapling_tree_checkpoint_marks_removed (
            checkpoint_id INTEGER NOT NULL,
            mark_removed_position INTEGER NOT NULL,
            CONSTRAINT sapling_mark_removed_unique UNIQUE (checkpoint_id, mark_removed_position)
        );
        CREATE TABLE IF NOT EXISTS sapling_tree_checkpoints (
            checkpoint_id INTEGER PRIMARY KEY,
            position INTEGER
        );
        CREATE TABLE IF NOT EXISTS sapling_tree_shards (
            shard_index INTEGER PRIMARY KEY,
            subtree_end_height INTEGER,
            root_hash BLOB,
            shard_data BLOB,
            contains_marked INTEGER,
            CONSTRAINT sapling_root_unique UNIQUE (root_hash)
        );
        CREATE INDEX IF NOT EXISTS idx_sapling_tree_shards_subtree_end_height
            ON sapling_tree_shards(subtree_end_height);

        CREATE TABLE IF NOT EXISTS orchard_tree_cap (
            cap_id INTEGER PRIMARY KEY,
            cap_data BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS orchard_tree_checkpoint_marks_removed (
            checkpoint_id INTEGER NOT NULL,
            mark_removed_position INTEGER NOT NULL,
            CONSTRAINT orchard_mark_removed_unique UNIQUE (checkpoint_id, mark_removed_position)
        );
        CREATE TABLE IF NOT EXISTS orchard_tree_checkpoints (
            checkpoint_id INTEGER PRIMARY KEY,
            position INTEGER
        );
        CREATE TABLE IF NOT EXISTS orchard_tree_shards (
            shard_index INTEGER PRIMARY KEY,
            subtree_end_height INTEGER,
            root_hash BLOB,
            shard_data BLOB,
            contains_marked INTEGER,
            CONSTRAINT orchard_root_unique UNIQUE (root_hash)
        );
        CREATE INDEX IF NOT EXISTS idx_orchard_tree_shards_subtree_end_height
            ON orchard_tree_shards(subtree_end_height);

        -- Start from deterministic empty tree-store state.
        DELETE FROM sapling_tree_cap;
        DELETE FROM sapling_tree_checkpoint_marks_removed;
        DELETE FROM sapling_tree_checkpoints;
        DELETE FROM sapling_tree_shards;
        DELETE FROM orchard_tree_cap;
        DELETE FROM orchard_tree_checkpoint_marks_removed;
        DELETE FROM orchard_tree_checkpoints;
        DELETE FROM orchard_tree_shards;

        -- Require rescan so shardtree state is rebuilt from canonical compact blocks.
        UPDATE spendability_state
        SET
            spendable = 0,
            rescan_required = 1,
            target_height = 0,
            anchor_height = 0,
            validated_anchor_height = 0,
            repair_queued = 0,
            repair_from_height = 0,
            reason_code = 'ERR_RESCAN_REQUIRED',
            updated_at = datetime('now')
        WHERE id = 1;

        UPDATE sync_state SET
            local_height = 0,
            target_height = 0,
            last_checkpoint_height = 0,
            updated_at = datetime('now')
        WHERE id = 1;

        INSERT INTO migration_state (key, value, updated_at)
        VALUES ('v29_shardtree_store_tables', 'completed', datetime('now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at;

        COMMIT;
        "#,
    );

    if let Err(e) = migration_result {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(Error::Migration(e.to_string()));
    }

    Ok(())
}
