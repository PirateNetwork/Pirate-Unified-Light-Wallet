use std::fs;
use std::path::PathBuf;

fn sync_rs() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sync_path = manifest_dir.join("src/sync.rs");
    fs::read_to_string(sync_path).expect("read sync.rs")
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
        branch.contains("effective_start_height = end;"),
        "follow-tip start-ahead branch should clamp to tip and continue sync loop"
    );
}
