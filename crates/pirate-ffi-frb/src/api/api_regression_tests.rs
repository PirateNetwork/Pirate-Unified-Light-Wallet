use super::*;
use pirate_core::selection::{NoteType as SelectableNoteType, SelectableNote};
use pirate_storage_sqlite::{
    Account, AccountKey, Address, AddressScope, AddressType, ColorTag, Database,
    EncryptionAlgorithm, EncryptionKey, KeyScope, KeyType, MasterKey, NoteRecord,
    NoteType as DbNoteType, Repository,
};
use pirate_sync_lightd::SaplingFrontier;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use zcash_primitives::sapling::{value::NoteValue as SaplingNoteValue, Rseed};

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

fn insert_note(
    repo: &Repository,
    account_id: i64,
    key_id: i64,
    address_id: Option<i64>,
    note_type: DbNoteType,
    value: u64,
    tx_tag: u8,
) {
    let note = NoteRecord {
        id: None,
        account_id,
        key_id: Some(key_id),
        note_type,
        value: value as i64,
        nullifier: vec![tx_tag; 32],
        commitment: vec![tx_tag.wrapping_add(1); 32],
        spent: false,
        height: 1_000,
        txid: vec![tx_tag; 32],
        output_index: tx_tag as i64,
        address_id,
        spent_txid: None,
        diversifier: None,
        merkle_path: None,
        note: None,
        anchor: None,
        position: None,
        memo: None,
    };
    repo.insert_note(&note).unwrap();
}

fn valid_orchard_anchor(seed: u8) -> orchard::tree::Anchor {
    for i in 0..=u16::MAX {
        let mut bytes = [0u8; 32];
        for (idx, b) in bytes.iter_mut().enumerate() {
            *b = seed
                .wrapping_mul(37)
                .wrapping_add(idx as u8)
                .wrapping_add((i as u8).wrapping_mul(17));
        }
        bytes[30] = (i >> 8) as u8;
        bytes[31] = i as u8;
        if let Some(anchor) = Option::from(orchard::tree::Anchor::from_bytes(bytes)) {
            return anchor;
        }
    }
    panic!("failed to construct a valid Orchard anchor")
}

fn sapling_selectable_note(value: u64, seed: u8) -> SelectableNote {
    let extsk = zcash_primitives::zip32::ExtendedSpendingKey::master(&[seed; 32]);
    let dfvk = extsk.to_diversifiable_full_viewing_key();
    let (_, address) = dfvk.default_address();
    let note = zcash_primitives::sapling::Note::from_parts(
        address,
        SaplingNoteValue::from_raw(value),
        Rseed::AfterZip212([seed; 32]),
    );

    let mut frontier = SaplingFrontier::new();
    let pos = frontier
        .apply_note_commitment_with_position(note.cmu().to_bytes())
        .unwrap();
    frontier.mark_position().unwrap();
    let path = frontier.witness(pos).unwrap().unwrap();

    SelectableNote::new(value, vec![seed; 32], 10, vec![seed; 32], seed as u32).with_witness(
        path,
        *address.diversifier(),
        note,
    )
}

fn orchard_selectable_note(value: u64, seed: u8, anchor: orchard::tree::Anchor) -> SelectableNote {
    let mut note =
        SelectableNote::new_orchard(value, vec![seed; 32], 10, vec![seed; 32], seed as u32);
    note.orchard_anchor = Some(anchor);
    note
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

    // Intentionally omit serialized note/witness fields so the selector uses
    // the fallback unspent-note totals path; this keeps the test deterministic.
    insert_note(
        &repo,
        account_id,
        key_twenty,
        None,
        DbNoteType::Orchard,
        20,
        0x20,
    );
    insert_note(
        &repo,
        account_id,
        key_fifty,
        None,
        DbNoteType::Sapling,
        50,
        0x50,
    );

    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 10, None).unwrap(),
        Some(key_twenty)
    );
    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 30, None).unwrap(),
        Some(key_fifty)
    );
    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 60, None).unwrap(),
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

    insert_note(
        &repo,
        account_id,
        key_locked,
        None,
        DbNoteType::Orchard,
        1_000,
        0xA0,
    );
    insert_note(
        &repo,
        account_id,
        key_spendable,
        None,
        DbNoteType::Sapling,
        40,
        0xB0,
    );

    assert_eq!(
        auto_select_spend_key_id_for_amount(&repo, account_id, 30, None).unwrap(),
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

#[test]
fn test_align_sapling_anchor_group_prefers_smallest_sufficient_group() {
    let sapling_small = sapling_selectable_note(40, 0x11);
    let sapling_large = sapling_selectable_note(70, 0x22);
    let expected_anchor = hex::encode(sapling_anchor_for_selectable_note(&sapling_large).unwrap());
    let orchard_anchor = valid_orchard_anchor(0x33);
    let orchard_passthrough = orchard_selectable_note(5, 0x33, orchard_anchor);

    let (aligned, groups, filtered, chosen_anchor_hex) =
        align_sapling_anchor_group(vec![sapling_small, sapling_large, orchard_passthrough], 50);

    assert_eq!(groups, 2);
    assert_eq!(filtered, 1);
    assert_eq!(chosen_anchor_hex, Some(expected_anchor));
    assert_eq!(
        aligned
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Sapling)
            .count(),
        1
    );
    assert_eq!(
        aligned
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Orchard)
            .count(),
        1
    );
}

#[test]
fn test_align_orchard_anchor_group_prefers_richest_when_none_sufficient() {
    let anchor_a = valid_orchard_anchor(0x44);
    let anchor_b = valid_orchard_anchor(0x45);
    assert_ne!(anchor_a, anchor_b);

    let orchard_low = orchard_selectable_note(20, 0x44, anchor_a);
    let orchard_high = orchard_selectable_note(35, 0x55, anchor_b);
    let sapling_passthrough = sapling_selectable_note(10, 0x66);

    let (aligned, groups, filtered, chosen_anchor_hex) =
        align_orchard_anchor_group(vec![orchard_low, orchard_high, sapling_passthrough], 100);

    assert_eq!(groups, 2);
    assert_eq!(filtered, 1);
    assert_eq!(chosen_anchor_hex, Some(hex::encode(anchor_b.to_bytes())));
    assert_eq!(
        aligned
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Orchard)
            .count(),
        1
    );
    assert_eq!(
        aligned
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Sapling)
            .count(),
        1
    );
}

#[test]
fn test_mixed_pool_anchor_alignment_sequence() {
    let sapling_low = sapling_selectable_note(30, 0x71);
    let sapling_high = sapling_selectable_note(70, 0x72);
    let orchard_anchor_low = valid_orchard_anchor(0x73);
    let orchard_anchor_high = valid_orchard_anchor(0x74);
    let orchard_low = orchard_selectable_note(20, 0x73, orchard_anchor_low);
    let orchard_high = orchard_selectable_note(90, 0x74, orchard_anchor_high);

    let (after_sapling, sap_groups, sap_filtered, _) = align_sapling_anchor_group(
        vec![sapling_low, sapling_high, orchard_low, orchard_high],
        80,
    );
    assert_eq!(sap_groups, 2);
    assert_eq!(sap_filtered, 1);
    assert_eq!(after_sapling.len(), 3);

    let (after_orchard, orch_groups, orch_filtered, chosen) =
        align_orchard_anchor_group(after_sapling, 80);
    assert_eq!(orch_groups, 2);
    assert_eq!(orch_filtered, 1);
    assert_eq!(chosen, Some(hex::encode(orchard_anchor_high.to_bytes())));
    assert_eq!(after_orchard.len(), 2);
    assert_eq!(
        after_orchard
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Sapling)
            .count(),
        1
    );
    assert_eq!(
        after_orchard
            .iter()
            .filter(|n| n.note_type == SelectableNoteType::Orchard)
            .count(),
        1
    );
}
