use std::fs;
use std::path::PathBuf;

fn sync_rs() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sync_path = manifest_dir.join("src/sync.rs");
    fs::read_to_string(sync_path).expect("read sync.rs")
}

fn shardtree_support_rs() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let support_path = manifest_dir.join("src/sync/shardtree_support.rs");
    fs::read_to_string(support_path).expect("read shardtree_support.rs")
}

#[test]
fn forbids_custom_tip_loop_witness_repair_function() {
    let src = sync_rs();
    assert!(
        !src.contains("check_witness_integrity_and_queue_repair"),
        "custom tip-loop witness repair function still present"
    );
    assert!(
        !src.contains("frontier_integrity_guard"),
        "custom frontier integrity guard branch should be removed"
    );
    assert!(
        !src.contains("spawn_frontier_integrity_check"),
        "frontier integrity spawn branch should be removed"
    );
    assert!(
        !src.contains("reset_frontiers_for_replay"),
        "frontier replay reset helper should be removed from canonical queue-first flow"
    );
}

#[test]
fn requires_queue_based_check_witnesses_bridge() {
    let src = sync_rs();
    assert!(
        src.contains("check_witnesses("),
        "sync flow requires explicit queue-first check_witnesses bridge"
    );
}

#[test]
fn forbids_inline_activation_side_effects_in_check_witnesses_pass() {
    let src = sync_rs();
    let start = src
        .find("async fn check_witnesses_and_queue_rescans(")
        .expect("check_witnesses_and_queue_rescans exists");
    let end = src[start..]
        .find("async fn activate_queued_found_note_range(")
        .map(|idx| start + idx)
        .expect("activate_queued_found_note_range exists");
    let body = &src[start..end];

    assert!(
        !body.contains("mark_in_progress("),
        "queue-first flow forbids activating queue rows inside witness check pass"
    );
}

#[test]
fn has_single_queue_activation_site_in_sync_loop() {
    let src = sync_rs();
    let activation_calls = src
        .matches("activate_queued_found_note_range().await?")
        .count();
    assert_eq!(
        activation_calls, 1,
        "sync loop should have a single queue-activation site (regular queue worker path)"
    );
    assert!(
        !src.contains("while at tip"),
        "tip-loop specific FoundNote replay branch should be removed"
    );
}

#[test]
fn check_pass_does_not_run_remote_root_mismatch_reseed_logic() {
    let src = sync_rs();
    let start = src
        .find("async fn check_witnesses_and_queue_rescans(")
        .expect("check_witnesses_and_queue_rescans exists");
    let end = src[start..]
        .find("async fn activate_queued_found_note_range(")
        .map(|idx| start + idx)
        .expect("activate_queued_found_note_range exists");
    let body = &src[start..end];

    assert!(
        !body.contains("fetch_tree_state_with_retry("),
        "queue-first witness check should not fetch remote tree states"
    );
    assert!(
        !body.contains("reseed_orchard_shardtree_from_remote("),
        "queue-first witness check should not perform reseed operations"
    );
    assert!(
        !body.contains("log_anchor_root_mismatch"),
        "queue-first witness check should not run anchor-root mismatch logic"
    );
}

#[test]
fn check_pass_queues_repairs_directly_from_repository_ranges() {
    let src = sync_rs();
    let start = src
        .find("async fn check_witnesses_and_queue_rescans(")
        .expect("check_witnesses_and_queue_rescans exists");
    let end = src[start..]
        .find("async fn activate_queued_found_note_range(")
        .map(|idx| start + idx)
        .expect("activate_queued_found_note_range exists");
    let body = &src[start..end];

    assert!(
        body.contains("spendability.queue_repair_range("),
        "witness check should enqueue FoundNote replay ranges directly"
    );
    assert!(
        body.contains("mark_found_note_done_through"),
        "witness check should retire completed in-progress queue rows"
    );
}

#[test]
fn follow_tip_start_ahead_keeps_queue_worker_path_active() {
    let src = sync_rs();
    let start = src
        .find("if end < start_height {")
        .expect("start-ahead branch exists");
    let window_end = std::cmp::min(src.len(), start + 1800);
    let branch = &src[start..window_end];

    assert!(
        branch.contains("if follow_tip {"),
        "start-ahead handling must special-case follow-tip mode"
    );
    assert!(
        branch.contains("do NOT clamp start down to `end`"),
        "follow-tip start-ahead branch must avoid clamping to tip to prevent re-fetching the last processed block"
    );
    assert!(
        branch.contains("Keep effective_start_height at resume height"),
        "follow-tip start-ahead branch should preserve resume height semantics so fetch loop no-ops and queue worker/monitor path remains active"
    );
}

#[test]
fn shardtree_persistence_uses_batched_insertion_not_leaf_append() {
    let sync_src = sync_rs();
    let support_src = shardtree_support_rs();
    let persist_start = sync_src
        .find("fn persist_shardtree_batches(")
        .expect("persist_shardtree_batches exists");
    let persist_end = sync_src[persist_start..]
        .find("async fn apply_positions(")
        .map(|idx| persist_start + idx)
        .expect("apply_positions section exists");
    let helper_start = support_src
        .find("fn apply_shardtree_batches_to_trees<")
        .expect("apply_shardtree_batches_to_trees exists");
    let helper_end = support_src[helper_start..]
        .find("pub(super) fn append_sapling_leaf(")
        .map(|idx| helper_start + idx)
        .expect("append_sapling_leaf exists");
    let persist_body = &sync_src[persist_start..persist_end];
    let helper_body = &support_src[helper_start..helper_end];

    assert!(
        persist_body.contains("apply_shardtree_batches_to_trees("),
        "frontier persistence should delegate to the shared batched shardtree helper"
    );
    assert!(
        helper_body.contains(".batch_insert("),
        "shared shardtree helper should use batched insertion"
    );
    assert!(
        !helper_body.contains("sapling_tree.append("),
        "shared shardtree helper should not append Sapling commitments leaf-by-leaf"
    );
    assert!(
        !helper_body.contains("orchard_tree.append("),
        "shared shardtree helper should not append Orchard commitments leaf-by-leaf"
    );
}
