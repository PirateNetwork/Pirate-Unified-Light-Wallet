use std::fs;
use std::path::PathBuf;

fn tx_flow_rs() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tx_flow_path = manifest_dir.join("src/api/tx_flow.rs");
    fs::read_to_string(tx_flow_path).expect("read tx_flow.rs")
}

#[test]
fn forbids_send_time_witness_hydration_call() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("hydrate_selectable_note_witnesses_from_snapshot("),
        "send path still performs witness hydration from snapshot"
    );
}

#[test]
fn forbids_send_time_witness_hydration_helper() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("fn hydrate_selectable_note_witnesses_from_snapshot("),
        "send-time witness hydration helper still present"
    );
    assert!(
        !src.contains("fn prepare_anchor_witness_ready_notes("),
        "send-time anchor witness preparation helper still present"
    );
}

#[test]
fn forbids_legacy_materialize_helper_symbol() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("materialize_selectable_note_witnesses_from_snapshot("),
        "legacy materialize helper symbol must be removed from api send/build path"
    );
}

#[test]
fn forbids_send_time_materialization_call_in_sign_path() {
    let src = tx_flow_rs();
    let start = src
        .find("fn sign_tx_internal(")
        .expect("sign_tx_internal exists");
    let end = src[start..]
        .find("pub(super) fn sign_tx(")
        .map(|idx| start + idx)
        .expect("sign_tx exists");
    let sign_body = &src[start..end];
    assert!(
        !sign_body.contains("materialize_selectable_note_witnesses_from_snapshot("),
        "sign path must not materialize witnesses; build-time context must carry witness-ready notes"
    );
}

#[test]
fn forbids_prepare_anchor_witness_callsites() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("prepare_anchor_witness_ready_notes("),
        "send/build/balance paths must not call API-side witness preparation helper"
    );
}

#[test]
fn forbids_signing_active_sync_coupling() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("SIGNING_ACTIVE"),
        "send/sync flow forbids signing-active global coupling"
    );
    assert!(
        !src.contains("is_signing_active("),
        "send/sync flow forbids start_sync coupling to signing-active checks"
    );
}

#[test]
fn forbids_send_path_repair_queue_helper() {
    let src = tx_flow_rs();
    assert!(
        !src.contains("queue_spendability_repair("),
        "send path should not enqueue repair directly; queue worker handles repair scheduling"
    );
}

#[test]
fn build_sign_timeout_scales_with_input_count() {
    let src = tx_flow_rs();
    assert!(
        src.contains("BUILD_AND_SIGN_TIMEOUT_BASE_SECS"),
        "build/sign timeout should have an explicit base duration"
    );
    assert!(
        src.contains("BUILD_AND_SIGN_TIMEOUT_PER_INPUT_SECS"),
        "build/sign timeout should scale with selected input count"
    );
    assert!(
        src.contains("BUILD_AND_SIGN_TIMEOUT_MAX_SECS"),
        "build/sign timeout should keep a hard upper bound"
    );
    assert!(
        !src.contains("Build/sign timed out after 120s"),
        "large shielded sends must not use the old fixed 120s timeout"
    );
}
