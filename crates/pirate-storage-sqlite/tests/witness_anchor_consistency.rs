use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn requires_v31_schema_target() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations.rs");
    let src = fs::read_to_string(path).expect("read migrations");
    assert!(
        src.contains("const SCHEMA_VERSION: i32 = 31;"),
        "schema target must remain v31"
    );
}

#[test]
fn requires_canonical_shard_views_in_migrations() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/migrations.rs");
    let src = fs::read_to_string(path).expect("read migrations");
    assert!(
        src.contains("CREATE VIEW v_sapling_shard_scan_ranges"),
        "missing canonical Sapling shard scan view migration"
    );
    assert!(
        src.contains("CREATE VIEW v_sapling_shard_unscanned_ranges"),
        "missing canonical Sapling shard unscanned view migration"
    );
    assert!(
        src.contains("CREATE VIEW v_orchard_shard_scan_ranges"),
        "missing canonical Orchard shard scan view migration"
    );
    assert!(
        src.contains("CREATE VIEW v_orchard_shard_unscanned_ranges"),
        "missing canonical Orchard shard unscanned view migration"
    );
    assert!(
        src.contains("start_position"),
        "canonical shard views must be position-backed"
    );
    assert!(
        src.contains("end_position_exclusive"),
        "canonical shard views must expose end_position_exclusive"
    );
}

#[test]
fn forbids_temp_shard_views_in_selection_query() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/repository.rs");
    let src = fs::read_to_string(path).expect("read repository");
    assert!(
        !src.contains("temp_v_sapling_shard_unscanned_ranges"),
        "selection path still uses temp sapling shard view"
    );
    assert!(
        !src.contains("temp_v_orchard_shard_unscanned_ranges"),
        "selection path still uses temp orchard shard view"
    );
    assert!(
        !src.contains("temp_candidate_note_scope"),
        "selection path still uses temp candidate scope table"
    );
    assert!(
        !src.contains("sapling_tip_unscanned"),
        "selection path still uses coarse sapling tip-level unscanned gate"
    );
    assert!(
        !src.contains("orchard_tip_unscanned"),
        "selection path still uses coarse orchard tip-level unscanned gate"
    );
}

#[test]
fn requires_check_witnesses_entrypoint() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/repository.rs");
    let src = fs::read_to_string(path).expect("read repository");
    assert!(
        src.contains("fn check_witnesses("),
        "repository requires explicit check_witnesses entrypoint"
    );
    assert!(
        src.contains("subtree-derived"),
        "check_witnesses should use subtree-derived repair queuing"
    );
    assert!(
        !src.contains("queue only the note's own height window"),
        "height-only fallback windows must be removed from check_witnesses queuing"
    );
}

#[test]
fn requires_scan_queue_extrema_anchor_target_derivation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/spendability_state.rs");
    let src = fs::read_to_string(path).expect("read spendability_state");
    let start = src
        .find("pub fn get_target_and_anchor_heights(")
        .expect("target/anchor function exists");
    let end = src[start..]
        .find("pub fn save_state(")
        .map(|idx| start + idx)
        .expect("save_state exists");
    let fn_body = &src[start..end];
    assert!(
        fn_body.contains("self.scan_queue_extrema()?"),
        "target/anchor derivation must use canonical scan_queue_extrema source"
    );
    assert!(
        !fn_body.contains("derive_chain_tip_height("),
        "target/anchor derivation must not use local tip shortcut fallbacks"
    );
}

#[test]
fn forbidden_path_guard_script_exists() {
    let script = repo_root().join("scripts/witness_anchor_forbidden_paths.sh");
    assert!(
        script.exists(),
        "missing witness/anchor forbidden-paths guard script"
    );
}

#[test]
fn anchor_witness_hydration_uses_shardtree_caching() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/frontier_witness.rs");
    let src = fs::read_to_string(path).expect("read frontier_witness");
    assert!(
        src.contains("witness_caching("),
        "anchor witness construction should cache witness ommers during hydration"
    );
    assert!(
        src.contains("root_at_checkpoint_caching("),
        "anchor witness construction should cache checkpoint roots during hydration"
    );
}
