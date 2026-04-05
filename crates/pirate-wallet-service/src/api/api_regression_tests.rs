use super::tx_flow::{
    add_pending_change, auto_select_spend_key_id_for_amount, choose_multi_key_change_sink_key_id,
    clear_pending_changes, has_pending_changes, infer_contributing_key_ids_for_amount,
    normalize_filter_ids, note_balances_by_key_id, resolve_pending_change, resolve_spend_key_id,
    txid_hex_variants_from_bytes, SpendSelectionAnchors,
};
use super::*;
use incrementalmerkletree::Retention;
use pirate_core::selection::SelectableNote;
use pirate_storage_sqlite::{
    Account, AccountKey, Address, AddressScope, AddressType, ColorTag, Database,
    EncryptionAlgorithm, EncryptionKey, KeyScope, KeyType, MasterKey, NoteRecord,
    NoteType as DbNoteType, Repository,
};
use shardtree::ShardTree;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use zcash_primitives::{
    consensus::BlockHeight,
    sapling::value::NoteValue as SaplingNoteValue,
    sapling::{
        note::ExtractedNoteCommitment as SaplingExtractedNoteCommitment, Note as SaplingNote, Rseed,
    },
    zip32::sapling::ExtendedSpendingKey as SaplingExtendedSpendingKey,
};

fn test_db_path() -> PathBuf {
    std::env::temp_dir().join(format!("pirate-ffi-regression-{}.db", uuid::Uuid::new_v4()))
}

fn test_db() -> Database {
    let path = test_db_path();
    let salt = pirate_storage_sqlite::generate_salt();
    let key = EncryptionKey::from_passphrase("test-passphrase", &salt).unwrap();
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    Database::open(path, &key, master_key).unwrap()
}

fn setup_repo() -> (Database, i64) {
    let db = test_db();
    let repo = Repository::new(&db);
    let account_id = repo
        .insert_account(&Account {
            id: None,
            name: "test-account".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        })
        .unwrap();
    (db, account_id)
}

fn test_selection_anchors(anchor_height: u64) -> SpendSelectionAnchors {
    SpendSelectionAnchors {
        target_height: anchor_height.saturating_add(1),
        conservative_anchor_height: anchor_height,
        sapling_anchor_height: anchor_height,
        orchard_anchor_height: anchor_height,
    }
}

fn insert_account_key(
    repo: &Repository,
    account_id: i64,
    key_type: KeyType,
    spendable: bool,
    has_sapling_extsk: bool,
    has_orchard_extsk: bool,
    label: &str,
) -> i64 {
    let key = AccountKey {
        id: None,
        account_id,
        key_type,
        key_scope: KeyScope::Account,
        label: Some(label.to_string()),
        birthday_height: 1,
        created_at: chrono::Utc::now().timestamp(),
        spendable,
        sapling_extsk: has_sapling_extsk.then(|| vec![0x11; 169]),
        sapling_dfvk: None,
        orchard_extsk: has_orchard_extsk.then(|| vec![0x22; 96]),
        orchard_fvk: None,
        encrypted_mnemonic: None,
    };
    let encrypted = repo.encrypt_account_key_fields(&key).unwrap();
    repo.upsert_account_key(&encrypted).unwrap()
}

fn insert_address(repo: &Repository, account_id: i64, key_id: i64, tag: &str) -> i64 {
    let address = Address {
        id: None,
        key_id: Some(key_id),
        account_id,
        diversifier_index: 0,
        address: format!("test-{}-{}", key_id, tag),
        address_type: AddressType::Sapling,
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: ColorTag::None,
        address_scope: AddressScope::External,
    };
    repo.upsert_address(&address).unwrap();
    repo.get_all_addresses(account_id)
        .unwrap()
        .into_iter()
        .find(|a| a.address == address.address)
        .and_then(|a| a.id)
        .unwrap()
}

fn insert_sapling_note(
    repo: &Repository,
    account_id: i64,
    key_id: i64,
    value_zat: u64,
    tx_tag: u8,
    position: i64,
) -> [u8; 32] {
    let seed = [tx_tag.max(1); 32];
    let extsk = SaplingExtendedSpendingKey::master(&seed);
    let (_, address) = extsk.default_address();
    let note_value = SaplingNoteValue::from_raw(value_zat);
    let rseed_bytes = [tx_tag.wrapping_add(1); 32];
    let note = SaplingNote::from_parts(address, note_value, Rseed::AfterZip212(rseed_bytes));
    let commitment_bytes = note.cmu().to_bytes();
    let mut note_blob = Vec::with_capacity(1 + 43 + 1 + 32);
    note_blob.push(1); // version
    note_blob.extend_from_slice(&address.to_bytes());
    note_blob.push(0x02); // ZIP-212 Rseed
    note_blob.extend_from_slice(&rseed_bytes);
    let note = NoteRecord {
        id: None,
        account_id,
        key_id: Some(key_id),
        note_type: DbNoteType::Sapling,
        value: value_zat as i64,
        nullifier: vec![tx_tag; 32],
        commitment: commitment_bytes.to_vec(),
        spent: false,
        height: 1_000,
        txid: vec![tx_tag; 32],
        output_index: tx_tag as i64,
        address_id: None,
        spent_txid: None,
        diversifier: None,
        note: Some(note_blob),
        position: Some(position),
        memo: None,
    };
    repo.insert_note(&note).unwrap();
    commitment_bytes
}

fn seed_sapling_shardtree_checkpoint(db: &Database, checkpoint_height: u32, cmus: &[[u8; 32]]) {
    const SAPLING_TABLE_PREFIX: &str = "sapling";
    const SHARDTREE_PRUNING_DEPTH: usize = 1000;
    const SAPLING_SHARD_HEIGHT: u8 = zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH / 2;

    let tx = db
        .conn()
        .unchecked_transaction()
        .expect("failed to open shardtree transaction");
    let store = pirate_storage_sqlite::shardtree_store::SqliteShardStore::<
        _,
        zcash_primitives::sapling::Node,
        SAPLING_SHARD_HEIGHT,
    >::from_connection(&tx, SAPLING_TABLE_PREFIX)
    .expect("failed to open shardtree store");
    let mut tree: ShardTree<
        _,
        { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
        SAPLING_SHARD_HEIGHT,
    > = ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);

    for cmu in cmus {
        let cmu_opt: Option<SaplingExtractedNoteCommitment> =
            SaplingExtractedNoteCommitment::from_bytes(cmu).into();
        let cmu_value = cmu_opt.expect("test cmu must be valid");
        let node = zcash_primitives::sapling::Node::from_cmu(&cmu_value);
        tree.append(node, Retention::Marked)
            .expect("failed to append test commitment");
    }

    tree.checkpoint(BlockHeight::from(checkpoint_height))
        .expect("failed to checkpoint shardtree");
    tx.commit().expect("failed to commit shardtree seed");
}

#[test]
fn test_normalize_filter_ids_deduplicates_and_drops_empty() {
    assert_eq!(normalize_filter_ids(Some(vec![])), None);
    assert_eq!(
        normalize_filter_ids(Some(vec![4, 4, 1, 4, 1])),
        Some(vec![4, 1])
    );
}

#[test]
fn test_txid_hex_variants_cover_both_byte_orders() {
    let txid: Vec<u8> = (0u8..32u8).collect();
    let mut reversed = txid.clone();
    reversed.reverse();

    let variants = txid_hex_variants_from_bytes(&txid);
    assert!(variants.contains(&hex::encode(&txid)));
    assert!(variants.contains(&hex::encode(&reversed)));
}

#[test]
fn test_pending_change_clears_when_matching_txid_is_detected() {
    let wallet_id = format!("wallet-{}", uuid::Uuid::new_v4());
    clear_pending_changes(&wallet_id);

    let txid: Vec<u8> = (1u8..=32u8).collect();
    let txid_hex = hex::encode(&txid);
    add_pending_change(&wallet_id, &txid_hex, 42_000);

    // Still pending before the note's txid is observed.
    assert_eq!(resolve_pending_change(&wallet_id, &HashSet::new()), 42_000);

    // Notes are commonly stored in internal byte order; ensure detection still clears.
    let mut internal_order = txid.clone();
    internal_order.reverse();
    let known: HashSet<String> = txid_hex_variants_from_bytes(&internal_order)
        .into_iter()
        .collect();

    assert_eq!(resolve_pending_change(&wallet_id, &known), 0);
    assert!(!has_pending_changes(&wallet_id));
}

#[test]
fn test_resolve_spend_key_id_manual_and_address_filters() {
    let (db, account_id) = setup_repo();
    let repo = Repository::new(&db);

    let key_a = insert_account_key(&repo, account_id, KeyType::Seed, true, true, true, "seed-a");
    let key_b = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-b",
    );
    let addr_a = insert_address(&repo, account_id, key_a, "addr-a");
    let addr_b = insert_address(&repo, account_id, key_b, "addr-b");

    assert_eq!(
        resolve_spend_key_id(&repo, account_id, Some(&[key_a]), None).unwrap(),
        Some(key_a)
    );
    assert_eq!(
        resolve_spend_key_id(&repo, account_id, Some(&[key_a, key_b]), None).unwrap(),
        None
    );
    assert_eq!(
        resolve_spend_key_id(&repo, account_id, None, Some(&[addr_a])).unwrap(),
        Some(key_a)
    );
    assert_eq!(
        resolve_spend_key_id(&repo, account_id, None, Some(&[addr_a, addr_b])).unwrap(),
        None
    );

    let err = resolve_spend_key_id(&repo, account_id, Some(&[key_a]), Some(&[addr_b])).unwrap_err();
    assert!(err.to_string().contains("does not match"));
}

#[test]
fn test_auto_select_key_group_sapling_orchard_mixed_matrix() {
    let (db, account_id) = setup_repo();
    let repo = Repository::new(&db);

    let _key_empty = insert_account_key(
        &repo,
        account_id,
        KeyType::Seed,
        true,
        true,
        true,
        "seed-empty",
    );
    let key_twenty = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-20",
    );
    let key_fifty = insert_account_key(
        &repo,
        account_id,
        KeyType::Seed,
        true,
        true,
        true,
        "seed-50",
    );

    let cmu_twenty = insert_sapling_note(&repo, account_id, key_twenty, 20, 0x20, 0);
    let cmu_fifty = insert_sapling_note(&repo, account_id, key_fifty, 50, 0x50, 1);
    // Note insertion order is the commitment tree order for this test harness.
    seed_sapling_shardtree_checkpoint(&db, 1_000, &[cmu_twenty, cmu_fifty]);

    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 10, test_selection_anchors(1_000))
            .unwrap(),
        Some(key_twenty)
    );
    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 30, test_selection_anchors(1_000))
            .unwrap(),
        Some(key_fifty)
    );
    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 60, test_selection_anchors(1_000))
            .unwrap(),
        None
    );
}

#[test]
fn test_auto_select_ignores_unspendable_keys() {
    let (db, account_id) = setup_repo();
    let repo = Repository::new(&db);

    let key_locked = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        false,
        true,
        true,
        "locked",
    );
    let key_spendable = insert_account_key(
        &repo,
        account_id,
        KeyType::Seed,
        true,
        true,
        true,
        "spendable",
    );

    let cmu_locked = insert_sapling_note(&repo, account_id, key_locked, 1_000, 0xA0, 0);
    let cmu_spendable = insert_sapling_note(&repo, account_id, key_spendable, 40, 0xB0, 1);
    seed_sapling_shardtree_checkpoint(&db, 1_000, &[cmu_locked, cmu_spendable]);

    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 30, test_selection_anchors(1_000))
            .unwrap(),
        Some(key_spendable)
    );
}

#[test]
fn test_note_balances_by_key_id_aggregates_values() {
    let notes = vec![
        SelectableNote::new(20, vec![1], 10, vec![1], 0).with_key_id(Some(11)),
        SelectableNote::new(30, vec![2], 10, vec![2], 1).with_key_id(Some(11)),
        SelectableNote::new(50, vec![3], 10, vec![3], 2).with_key_id(Some(12)),
        SelectableNote::new(99, vec![4], 10, vec![4], 3),
    ];

    let balances = note_balances_by_key_id(&notes);
    assert_eq!(balances.get(&11).copied(), Some(50));
    assert_eq!(balances.get(&12).copied(), Some(50));
    assert!(!balances.contains_key(&13));
}

#[test]
fn test_infer_contributing_key_ids_for_amount_smallest_first() {
    let notes = vec![
        SelectableNote::new(20, vec![1], 10, vec![1], 0).with_key_id(Some(2)),
        SelectableNote::new(50, vec![2], 10, vec![2], 1).with_key_id(Some(3)),
        SelectableNote::new(90, vec![3], 10, vec![3], 2).with_key_id(Some(4)),
    ];

    let contributing = infer_contributing_key_ids_for_amount(&notes, 60);
    assert_eq!(contributing.len(), 2);
    assert!(contributing.contains(&2));
    assert!(contributing.contains(&3));
    assert!(!contributing.contains(&4));
}

#[test]
fn test_choose_multi_key_change_sink_prefers_seed() {
    let (db, account_id) = setup_repo();
    let repo = Repository::new(&db);

    let seed_key = insert_account_key(&repo, account_id, KeyType::Seed, true, true, true, "seed");
    let import_low = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-low",
    );
    let import_high = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-high",
    );

    let account_keys_by_id = repo
        .get_account_keys(account_id)
        .unwrap()
        .into_iter()
        .filter_map(|key| key.id.map(|id| (id, key)))
        .collect::<HashMap<_, _>>();
    let contributing = vec![import_low, import_high]
        .into_iter()
        .collect::<HashSet<_>>();
    let balances = HashMap::from([(import_low, 20u64), (import_high, 50u64), (seed_key, 0u64)]);

    assert_eq!(
        choose_multi_key_change_sink_key_id(&account_keys_by_id, &contributing, &balances),
        Some(seed_key)
    );
}

#[test]
fn test_choose_multi_key_change_sink_uses_largest_contributor_without_seed() {
    let (db, account_id) = setup_repo();
    let repo = Repository::new(&db);

    let import_low = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-low",
    );
    let import_high = insert_account_key(
        &repo,
        account_id,
        KeyType::ImportSpend,
        true,
        true,
        true,
        "import-high",
    );

    let account_keys_by_id = repo
        .get_account_keys(account_id)
        .unwrap()
        .into_iter()
        .filter_map(|key| key.id.map(|id| (id, key)))
        .collect::<HashMap<_, _>>();
    let contributing = vec![import_low, import_high]
        .into_iter()
        .collect::<HashSet<_>>();
    let balances = HashMap::from([(import_low, 20u64), (import_high, 50u64)]);

    assert_eq!(
        choose_multi_key_change_sink_key_id(&account_keys_by_id, &contributing, &balances),
        Some(import_high)
    );
}
