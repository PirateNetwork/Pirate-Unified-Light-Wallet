use std::fs;
use std::path::PathBuf;

fn api_rs() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let api_path = manifest_dir.join("src/api.rs");
    fs::read_to_string(api_path).expect("read api.rs")
}

#[test]
fn forbids_send_time_witness_hydration_call() {
    let src = api_rs();
    assert!(
        !src.contains("hydrate_selectable_note_witnesses_from_snapshot("),
        "send path still performs witness hydration from snapshot"
    );
}

#[test]
fn forbids_send_time_witness_hydration_helper() {
    let src = api_rs();
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
    let src = api_rs();
    assert!(
        !src.contains("materialize_selectable_note_witnesses_from_snapshot("),
        "legacy materialize helper symbol must be removed from api send/build path"
    );
}

#[test]
fn forbids_send_time_materialization_call_in_sign_path() {
    let src = api_rs();
    let start = src
        .find("fn sign_tx_internal(")
        .expect("sign_tx_internal exists");
    let end = src[start..]
        .find("pub fn sign_tx(")
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
    let src = api_rs();
    assert!(
        !src.contains("prepare_anchor_witness_ready_notes("),
        "send/build/balance paths must not call API-side witness preparation helper"
    );
}

#[test]
fn forbids_signing_active_sync_coupling() {
    let src = api_rs();
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
    let src = api_rs();
    assert!(
        !src.contains("queue_spendability_repair("),
        "send path should not enqueue repair directly; queue worker handles repair scheduling"
    );
}
