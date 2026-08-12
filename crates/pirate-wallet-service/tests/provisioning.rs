use pirate_wallet_service::{
    configure_wallet_storage, export_seed_raw, generate_mnemonic, get_active_wallet, list_wallets,
    restore_wallet, MnemonicLanguage,
};

#[test]
fn restored_wallet_is_visible_with_its_seed_and_active_selection() {
    let storage = tempfile::tempdir().unwrap();
    configure_wallet_storage(
        storage.path().to_string_lossy().into_owned(),
        "test passphrase".to_string(),
    )
    .unwrap();

    let mnemonic = generate_mnemonic(Some(24), Some(MnemonicLanguage::English)).unwrap();
    let wallet_id = restore_wallet(
        "Restored Wallet".to_string(),
        mnemonic.clone(),
        Some(3_500_000),
        Some(MnemonicLanguage::English),
    )
    .unwrap();

    let wallets = list_wallets().unwrap();
    assert_eq!(wallets.len(), 1);
    assert_eq!(wallets[0].id, wallet_id);
    assert_eq!(wallets[0].birthday_height, 3_500_000);
    assert_eq!(
        get_active_wallet().unwrap().as_deref(),
        Some(wallet_id.as_str())
    );
    assert_eq!(
        export_seed_raw(wallet_id, Some(MnemonicLanguage::English)).unwrap(),
        mnemonic
    );
}
