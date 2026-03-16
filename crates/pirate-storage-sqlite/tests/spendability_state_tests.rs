use pirate_storage_sqlite::{
    generate_salt, Database, EncryptionAlgorithm, EncryptionKey, MasterKey, ScanQueueStorage,
    SpendabilityStateStorage, SCAN_PRIORITY_FOUND_NOTE,
};
use tempfile::NamedTempFile;

fn test_db() -> Database {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_path_buf();
    let _ = file.into_temp_path();
    let salt = generate_salt();
    let key = EncryptionKey::from_passphrase("spendability-test-passphrase", &salt).unwrap();
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    Database::open(path, &key, master_key).unwrap()
}

#[test]
fn test_anchor_target_heights_come_from_scan_queue_extrema() {
    let db = test_db();
    let conn = db.conn();

    // Set sync/checkpoint heights in legacy tables; these must not influence
    // anchor/target derivation.
    conn.execute(
        "UPDATE sync_state SET local_height = 120, target_height = 130, last_checkpoint_height = 125 WHERE id = 1",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO checkpoints (height, hash, timestamp, sapling_tree_size) VALUES (140, 'h', 1, 1)",
        [],
    )
    .unwrap();

    // Queue rows provide canonical target height.
    conn.execute(
        r#"
        INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
        VALUES (200, 240, 10, 'done', 'historic', datetime('now'), datetime('now'))
        "#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"
        INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
        VALUES (1, 10, 40, 'pending', 'repair', datetime('now'), datetime('now'))
        "#,
        [],
    )
    .unwrap();

    // Tree checkpoints pin the effective anchor by snapping to the highest
    // checkpoint at-or-below the ideal anchor, using the more conservative
    // (lower) pool checkpoint when both exist.
    conn.execute(
        "INSERT INTO sapling_tree_checkpoints (checkpoint_id, position) VALUES (220, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sapling_tree_checkpoints (checkpoint_id, position) VALUES (228, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO orchard_tree_checkpoints (checkpoint_id, position) VALUES (225, NULL)",
        [],
    )
    .unwrap();

    let spendability = SpendabilityStateStorage::new(&db);
    let (target_height, anchor_height) = spendability
        .get_target_and_anchor_heights(10)
        .unwrap()
        .unwrap();

    assert_eq!(
        target_height, 240,
        "target should derive from queue max + 1"
    );
    assert_eq!(
        anchor_height, 225,
        "anchor should snap to the conservative checkpoint at/below ideal anchor"
    );
}

#[test]
fn test_anchor_target_heights_do_not_require_checkpoints() {
    let db = test_db();
    let conn = db.conn();

    conn.execute(
        r#"
        INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
        VALUES (200, 240, 10, 'done', 'historic', datetime('now'), datetime('now'))
        "#,
        [],
    )
    .unwrap();

    // Checkpoint exists above target-min_confirmations (230), but anchor remains
    // chain-derived from queue extrema.
    conn.execute(
        "INSERT INTO orchard_tree_checkpoints (checkpoint_id, position) VALUES (231, NULL)",
        [],
    )
    .unwrap();

    let spendability = SpendabilityStateStorage::new(&db);
    let derived = spendability
        .get_target_and_anchor_heights(10)
        .unwrap()
        .unwrap();
    assert_eq!(
        derived,
        (240, 230),
        "anchor should be available from queue-derived target/min floor even when checkpoints are sparse"
    );
}

#[test]
fn test_anchor_derivation_ignores_stale_pre_birthday_pool_checkpoint_for_account() {
    let db = test_db();
    let conn = db.conn();

    conn.execute(
        r#"
        INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
        VALUES (200, 240, 10, 'done', 'historic', datetime('now'), datetime('now'))
        "#,
        [],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO account_keys (
            account_id, key_type, key_scope, label, birthday_height, created_at, spendable,
            sapling_extsk, sapling_dfvk, orchard_extsk, orchard_fvk, encrypted_mnemonic
        )
        VALUES (?1, 'seed', 'account', 'k', ?2, 0, 1, NULL, NULL, NULL, NULL, NULL)
        "#,
        [1i64, 61i64],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sapling_tree_checkpoints (checkpoint_id, position) VALUES (60, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO orchard_tree_checkpoints (checkpoint_id, position) VALUES (228, NULL)",
        [],
    )
    .unwrap();

    let spendability = SpendabilityStateStorage::new(&db);

    let (target_height, anchor_height) = spendability
        .get_target_and_anchor_heights_for_account(10, 1)
        .unwrap()
        .unwrap();

    assert_eq!(target_height, 240);
    assert_eq!(
        anchor_height, 228,
        "account-aware anchor should snap to checkpoint at/below ideal anchor with birthday floor"
    );
}

#[test]
fn test_anchor_target_heights_by_pool_for_account_are_independent() {
    let db = test_db();
    let conn = db.conn();

    conn.execute(
        r#"
        INSERT INTO scan_queue (range_start, range_end, priority, status, reason, created_at, updated_at)
        VALUES (200, 240, 10, 'done', 'historic', datetime('now'), datetime('now'))
        "#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"
        INSERT INTO account_keys (
            account_id, key_type, key_scope, label, birthday_height, created_at, spendable,
            sapling_extsk, sapling_dfvk, orchard_extsk, orchard_fvk, encrypted_mnemonic
        )
        VALUES (?1, 'seed', 'account', 'k', ?2, 0, 1, NULL, NULL, NULL, NULL, NULL)
        "#,
        [1i64, 61i64],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sapling_tree_checkpoints (checkpoint_id, position) VALUES (228, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO orchard_tree_checkpoints (checkpoint_id, position) VALUES (225, NULL)",
        [],
    )
    .unwrap();

    let spendability = SpendabilityStateStorage::new(&db);
    let anchors = spendability
        .get_target_and_anchor_heights_by_pool_for_account(10, 1)
        .unwrap()
        .unwrap();

    assert_eq!(anchors.target_height, 240);
    assert_eq!(anchors.sapling_anchor_height, 228);
    assert_eq!(anchors.orchard_anchor_height, 225);
    assert_eq!(
        anchors.conservative_anchor_height, 225,
        "conservative anchor should remain the lower pool checkpoint"
    );
}

#[test]
fn test_queue_repair_range_sets_deterministic_state_and_merges() {
    let db = test_db();
    let spendability = SpendabilityStateStorage::new(&db);
    let queue = ScanQueueStorage::new(&db);

    spendability.mark_validated(300, 290).unwrap();
    spendability
        .queue_repair_range(500, 550, "ERR_WITNESS_REPAIR_QUEUED")
        .unwrap();
    spendability
        .queue_repair_range(520, 560, "ERR_WITNESS_REPAIR_QUEUED")
        .unwrap();

    let state = spendability.load_state().unwrap();
    assert!(!state.spendable, "queueing repair must clear spendable");
    assert!(state.repair_queued, "repair_queued must be set");
    assert_eq!(
        state.repair_from_height, 500,
        "repair_from_height should keep earliest queued height"
    );
    assert_eq!(state.reason_code, "ERR_WITNESS_REPAIR_QUEUED");

    let row = queue.next_found_note_range().unwrap().unwrap();
    assert_eq!(row.priority, SCAN_PRIORITY_FOUND_NOTE);
    assert_eq!(row.range_start, 500);
    assert_eq!(row.range_end, 560);
    assert_eq!(row.status, "pending");
}

#[test]
fn test_mark_repair_pending_without_enqueue_does_not_insert_queue_rows() {
    let db = test_db();
    let spendability = SpendabilityStateStorage::new(&db);
    let queue = ScanQueueStorage::new(&db);

    spendability
        .mark_repair_pending_without_enqueue(777, "ERR_WITNESS_REPAIR_QUEUED")
        .unwrap();

    let state = spendability.load_state().unwrap();
    assert!(!state.spendable);
    assert!(state.repair_queued);
    assert_eq!(state.repair_from_height, 777);
    assert_eq!(state.reason_code, "ERR_WITNESS_REPAIR_QUEUED");

    assert!(
        queue.next_found_note_range().unwrap().is_none(),
        "state-only marker must not enqueue additional found-note rows"
    );
}

#[test]
fn test_mark_sync_finalizing_clears_stale_repair_flags() {
    let db = test_db();
    let spendability = SpendabilityStateStorage::new(&db);

    spendability
        .mark_repair_pending_without_enqueue(777, "ERR_WITNESS_REPAIR_QUEUED")
        .unwrap();
    spendability.mark_sync_finalizing(1000, 990).unwrap();

    let state = spendability.load_state().unwrap();
    assert!(!state.spendable);
    assert!(!state.rescan_required);
    assert!(
        !state.repair_queued,
        "sync-finalizing should clear queued flag"
    );
    assert_eq!(
        state.repair_from_height, 0,
        "repair_from_height should be reset"
    );
    assert_eq!(state.target_height, 1000);
    assert_eq!(state.anchor_height, 990);
    assert_eq!(state.reason_code, "ERR_SYNC_FINALIZING");
}

#[test]
fn test_mark_found_note_done_through_only_retires_in_progress_rows_when_complete() {
    let db = test_db();
    let queue = ScanQueueStorage::new(&db);

    queue
        .queue_found_note_range(100, 200, Some("repair"))
        .unwrap();
    let row = queue.next_found_note_range().unwrap().unwrap();
    assert_eq!(row.status, "pending");

    let changed = queue.mark_found_note_done_through(500).unwrap();
    assert_eq!(
        changed, 0,
        "pending rows must not be retired before replay starts"
    );
    let row = queue.next_found_note_range().unwrap().unwrap();
    assert_eq!(row.status, "pending");

    queue.mark_in_progress(row.id).unwrap();
    let changed = queue.mark_found_note_done_through(150).unwrap();
    assert_eq!(
        changed, 0,
        "in-progress row must not retire before range_end"
    );

    let changed = queue.mark_found_note_done_through(200).unwrap();
    assert_eq!(changed, 1, "in-progress row should retire at range_end");
    assert!(
        queue.next_found_note_range().unwrap().is_none(),
        "completed found-note row should no longer be active"
    );
}

#[test]
fn test_queue_range_preserves_in_progress_row_when_overlap_is_queued() {
    let db = test_db();
    let queue = ScanQueueStorage::new(&db);

    queue
        .queue_found_note_range(100, 200, Some("repair"))
        .unwrap();
    let active = queue.next_found_note_range().unwrap().unwrap();
    queue.mark_in_progress(active.id).unwrap();

    queue
        .queue_found_note_range(150, 240, Some("repair"))
        .unwrap();

    let selected = queue.next_found_note_range().unwrap().unwrap();
    assert_eq!(
        selected.id, active.id,
        "active in_progress row should never be replaced by overlap-merge"
    );
    assert_eq!(selected.status, "in_progress");
    assert_eq!(selected.range_start, 100);
    assert_eq!(selected.range_end, 200);

    let pending_count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM scan_queue WHERE priority = ?1 AND status = 'pending'",
            [SCAN_PRIORITY_FOUND_NOTE],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending_count, 1, "follow-up overlap should remain queued");
}

#[test]
fn test_queue_range_noops_when_fully_covered_by_in_progress_row() {
    let db = test_db();
    let queue = ScanQueueStorage::new(&db);

    queue
        .queue_found_note_range(100, 200, Some("repair"))
        .unwrap();
    let active = queue.next_found_note_range().unwrap().unwrap();
    queue.mark_in_progress(active.id).unwrap();

    queue
        .queue_found_note_range(120, 180, Some("repair"))
        .unwrap();

    let pending_count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM scan_queue WHERE priority = ?1 AND status = 'pending'",
            [SCAN_PRIORITY_FOUND_NOTE],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        pending_count, 0,
        "request fully covered by active row should not enqueue duplicate pending work"
    );
}
