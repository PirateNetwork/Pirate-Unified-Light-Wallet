//! Regression coverage for a bug where wallets created via
//! `import_viewing_wallet` (watch-only: no spending key, ever) could not
//! actually list balances or generate receive addresses.
//!
//! `ensure_primary_account_key` only recognized `KeyType::Seed` keys, so it
//! never found the `KeyType::ImportView` key `import_viewing_wallet` itself
//! creates, and fell through to a fallback that unconditionally tries to
//! decode `secret.extsk` as a Sapling spending key - always empty, and
//! therefore always an error, for a watch-only wallet. That broke
//! `list_address_balances` (and anything else that calls
//! `ensure_primary_account_key`). Separately, `next_receive_address`
//! requires a real spending key by design and can never work for a
//! watch-only wallet at all; `generate_address_for_key` is the
//! watch-only-safe alternative, deriving straight from the stored viewing
//! key.

use super::*;
use std::sync::Mutex;
use tempfile::tempdir;

// configure_wallet_storage mutates process-wide statics (WALLETS,
// ACTIVE_WALLET, the registry cache, ...), same as the panic_duress tests
// elsewhere in this crate. Serialize this module's tests against each other
// so they don't stomp on one another's wallet storage context.
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Best-effort reset of the same process-wide statics `configure_wallet_storage`
/// mutates, so this test doesn't leave the app "unlocked" for whichever test
/// in this binary happens to run next (mirrors the `panic_duress` tests'
/// `reset_test_state`).
fn reset_global_wallet_state() {
    passphrase_store::clear_passphrase();
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    *WALLETS.write() = Vec::new();
    *ACTIVE_WALLET.write() = None;
    encrypted_db::invalidate_all_wallet_db_caches();
}

#[test]
fn watch_only_wallet_supports_balance_listing_and_address_generation() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_global_wallet_state();
    let temp_dir = tempdir().unwrap();

    configure_wallet_storage(
        temp_dir.path().to_string_lossy().to_string(),
        "test-passphrase-123".to_string(),
    )
    .unwrap();

    // A real seed-based wallet to export viewing keys from, exactly like an
    // operator would before handing them to a watch-only walletd instance.
    let source_wallet = create_wallet("source".to_string(), None, Some(1_000_000), None).unwrap();
    let sapling_vk = export_sapling_viewing_key(source_wallet.clone()).unwrap();
    let ironwood_vk = export_ironwood_viewing_key(source_wallet).unwrap();

    let watch_wallet = import_viewing_wallet(
        "btcpayserver".to_string(),
        Some(sapling_vk),
        Some(ironwood_vk),
        1_000_000,
    )
    .unwrap();

    let groups = list_key_groups(watch_wallet.clone()).unwrap();
    assert_eq!(groups.len(), 1, "expected exactly one key group for a freshly imported watch-only wallet");
    assert!(!groups[0].spendable, "an imported-viewing key group must never be marked spendable");
    let key_id = groups[0].id;

    // Regression: get_accounts (list_address_balances) used to fail here.
    // A fresh wallet seeds its index-0 addresses with zero balances rather
    // than returning an empty list, so check amounts, not list emptiness.
    let balances = list_address_balances(watch_wallet.clone(), None).unwrap();
    assert!(
        balances.iter().all(|b| b.balance == 0),
        "a fresh wallet with no synced notes should report zero balances, got {balances:?}"
    );

    // Regression: create_account/create_address used to fail here.
    let address = generate_address_for_key(watch_wallet.clone(), key_id, false).unwrap();
    assert!(
        address.starts_with("zs1"),
        "expected a mainnet Sapling address, got {address}"
    );

    let address2 = generate_address_for_key(watch_wallet, key_id, false).unwrap();
    assert_ne!(
        address, address2,
        "consecutive calls must mint distinct diversified addresses, like separate BTCPay invoices need"
    );

    reset_global_wallet_state();
}
