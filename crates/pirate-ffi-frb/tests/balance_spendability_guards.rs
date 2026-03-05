use std::fs;
use std::path::PathBuf;

#[test]
fn requires_anchor_eligible_spendable_basis() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let api_path = manifest_dir.join("src/api.rs");
    let src = fs::read_to_string(api_path).expect("read api.rs");

    // This is a strict guard that we don't compute spendable by taking all
    // unspent and capping/filtering later.
    let anchor_query_present = src.contains("get_unspent_selectable_notes_at_anchor_filtered(");
    assert!(
        anchor_query_present,
        "spendable basis must use anchor-eligible source query"
    );

    assert!(
        !src.contains("let spendable = 0u64;"),
        "fallback spendable init/cap model in get_balance must be removed"
    );
}
