use pirate_storage_sqlite::{
    generate_salt, spending_protection, Account, AccountKey, AppPassphrase, Database,
    EncryptionAlgorithm, EncryptionKey, KeyScope, KeyType, MasterKey, Repository, WalletSecret,
};
use tempfile::NamedTempFile;

fn test_db() -> Database {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_path_buf();
    let _ = file.into_temp_path();
    let salt = generate_salt();
    let key = EncryptionKey::from_passphrase("signing-protection-test", &salt).unwrap();
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    Database::open(path, &key, master_key).unwrap()
}

#[test]
fn signing_lock_hides_spending_material_but_keeps_viewing_material() {
    let db = test_db();
    let repo = Repository::new(&db);
    let wallet_id = "protected-wallet";
    let account_id = repo
        .insert_account(&Account {
            id: None,
            name: "Protected".to_string(),
            created_at: 1,
        })
        .unwrap();
    let secret = WalletSecret {
        wallet_id: wallet_id.to_string(),
        account_id,
        extsk: b"primary-sapling-spend".to_vec(),
        dfvk: Some(b"primary-sapling-view".to_vec()),
        orchard_extsk: Some(b"primary-ironwood-spend".to_vec()),
        sapling_ivk: None,
        orchard_ivk: None,
        encrypted_mnemonic: Some(b"seed words".to_vec()),
        mnemonic_language: Some("english".to_string()),
        created_at: 1,
    };
    repo.upsert_wallet_secret(&repo.encrypt_wallet_secret_fields(&secret).unwrap())
        .unwrap();
    let account_key = AccountKey {
        id: None,
        account_id,
        key_type: KeyType::Seed,
        key_scope: KeyScope::Account,
        label: None,
        birthday_height: 1,
        created_at: 1,
        spendable: true,
        sapling_extsk: Some(b"group-sapling-spend".to_vec()),
        sapling_dfvk: Some(b"group-sapling-view".to_vec()),
        orchard_extsk: Some(b"group-ironwood-spend".to_vec()),
        orchard_fvk: Some(b"group-ironwood-view".to_vec()),
        encrypted_mnemonic: None,
    };
    repo.upsert_account_key(&repo.encrypt_account_key_fields(&account_key).unwrap())
        .unwrap();

    let credential = "edge-account-session-secret";
    let kdf_salt = generate_salt();
    let signing_key = AppPassphrase::derive_key(credential, &kdf_salt).unwrap();
    let marker = signing_key.encrypt(b"credential-check").unwrap();
    repo.enable_signing_protection(wallet_id, account_id, &kdf_salt, &marker, &signing_key)
        .unwrap();

    spending_protection::lock_signing_session(wallet_id);
    let locked_secret = repo.get_wallet_secret(wallet_id).unwrap().unwrap();
    assert!(locked_secret.extsk.is_empty());
    assert!(locked_secret.orchard_extsk.is_none());
    assert!(locked_secret.encrypted_mnemonic.is_none());
    assert_eq!(locked_secret.dfvk, Some(b"primary-sapling-view".to_vec()));
    let locked_key = repo.get_account_keys(account_id).unwrap().remove(0);
    assert!(locked_key.sapling_extsk.is_none());
    assert!(locked_key.orchard_extsk.is_none());
    assert_eq!(
        locked_key.sapling_dfvk,
        Some(b"group-sapling-view".to_vec())
    );
    assert_eq!(
        locked_key.orchard_fvk,
        Some(b"group-ironwood-view".to_vec())
    );

    spending_protection::unlock_signing_session(wallet_id.to_string(), signing_key);
    let unlocked_secret = repo.get_wallet_secret(wallet_id).unwrap().unwrap();
    assert_eq!(unlocked_secret.extsk, b"primary-sapling-spend".to_vec());
    assert_eq!(
        unlocked_secret.orchard_extsk,
        Some(b"primary-ironwood-spend".to_vec())
    );
    assert_eq!(
        unlocked_secret.encrypted_mnemonic,
        Some(b"seed words".to_vec())
    );
    let unlocked_key = repo.get_account_keys(account_id).unwrap().remove(0);
    assert_eq!(
        unlocked_key.sapling_extsk,
        Some(b"group-sapling-spend".to_vec())
    );
    assert_eq!(
        unlocked_key.orchard_extsk,
        Some(b"group-ironwood-spend".to_vec())
    );
    spending_protection::lock_signing_session(wallet_id);
}
