use std::fs;
use std::path::PathBuf;

#[test]
fn requires_anchor_eligible_spendable_basis() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tx_flow_path = manifest_dir.join("src/api/tx_flow.rs");
    let src = fs::read_to_string(tx_flow_path).expect("read tx_flow.rs");

    // This is a strict guard that we don't compute spendable by taking all
    // unspent and capping/filtering later.
    let anchor_query_present = src.contains("get_unspent_selectable_notes_at_anchor_filtered(");
    assert!(
        anchor_query_present,
        "spendable basis must use anchor-eligible source query"
    );
}
