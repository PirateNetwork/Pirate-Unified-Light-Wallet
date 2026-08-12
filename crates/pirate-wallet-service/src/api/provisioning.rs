use super::tunnel::tunnel_transport_config;
use super::*;
use pirate_core::mnemonic::{canonicalize_mnemonic, generate_mnemonic, MnemonicLanguage};

pub(super) fn resolve_wallet_birthday_height(birthday_opt: Option<u32>) -> u32 {
    if let Some(birthday) = birthday_opt {
        return birthday;
    }

    let endpoint = LightdEndpoint::default();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let client_config = endpoint::build_light_client_config(
        &endpoint,
        transport,
        socks5_url,
        allow_direct_fallback,
        RetryConfig::default(),
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(10),
    );
    let client = LightClient::with_config(client_config);
    let fetch_latest = || async {
        if client.connect().await.is_err() {
            return None;
        }
        client.get_latest_block().await.ok().map(|h| h as u32)
    };
    let latest_height = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(fetch_latest()),
        Err(_) => {
            let runtime = tokio::runtime::Runtime::new().ok();
            runtime.as_ref().and_then(|rt| rt.block_on(fetch_latest()))
        }
    };

    latest_height.unwrap_or_else(|| Network::mainnet().default_birthday_height)
}

fn persist_wallet_account_secret(
    wallet_id: &str,
    account_name: String,
    mut secret: WalletSecret,
    birthday_height: u32,
) -> Result<()> {
    let passphrase = app_passphrase()?;
    let (db, _key, _master_key) = open_wallet_db_with_passphrase(wallet_id, &passphrase)?;
    let repo = Repository::new(&db);
    let transaction = db.conn().unchecked_transaction()?;

    let result = (|| -> Result<()> {
        let account = Account {
            id: None,
            name: account_name,
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account)?;

        secret.account_id = account_id;

        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
        repo.upsert_wallet_secret(&encrypted_secret)?;
        let _ = ensure_primary_account_key_at_birthday(&repo, &secret, birthday_height)?;
        Ok(())
    })();

    result?;
    transaction.commit()?;
    Ok(())
}

fn register_wallet(meta: &WalletMeta) -> Result<()> {
    let registry_db = open_wallet_registry()?;
    let transaction = registry_db.conn().unchecked_transaction()?;
    persist_wallet_meta(&registry_db, meta)?;
    set_active_wallet_registry(&registry_db, Some(&meta.id))?;
    touch_wallet_last_used(&registry_db, &meta.id)?;
    transaction.commit()?;

    WALLETS.write().push(meta.clone());
    *ACTIVE_WALLET.write() = Some(meta.id.clone());
    Ok(())
}

fn run_provisioning_steps<P, R, C>(persist: P, register: R, cleanup: C) -> Result<()>
where
    P: FnOnce() -> Result<()>,
    R: FnOnce() -> Result<()>,
    C: FnOnce(),
{
    match persist().and_then(|_| register()) {
        Ok(()) => Ok(()),
        Err(error) => {
            cleanup();
            Err(error)
        }
    }
}

fn provision_seed_wallet(
    meta: &WalletMeta,
    account_name: String,
    secret: WalletSecret,
) -> Result<()> {
    run_provisioning_steps(
        || persist_wallet_account_secret(&meta.id, account_name, secret, meta.birthday_height),
        || register_wallet(meta),
        || encrypted_db::remove_wallet_storage_artifacts(&meta.id),
    )
}

pub(super) fn create_wallet(
    name: String,
    _entropy_len: Option<u32>,
    birthday_opt: Option<u32>,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    let mnemonic_language = mnemonic_language.unwrap_or_default();
    let mnemonic = generate_mnemonic(Some(24), Some(mnemonic_language));
    let network = pirate_params::Network::mainnet();
    let extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
        &mnemonic,
        network.network_type,
        0,
        Some(mnemonic_language),
    )?;
    let _wallet = Wallet::from_mnemonic_in_language(&mnemonic, Some(mnemonic_language))?;

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
        &mnemonic,
        Some(mnemonic_language),
    )?;
    let orchard_master = IronwoodExtendedSpendingKey::master(&seed_bytes)?;

    let coin_type = network.coin_type;
    let account = 0;
    let orchard_extsk = orchard_master.derive_account(coin_type, account)?;

    let birthday_height = resolve_wallet_birthday_height(birthday_opt);

    let name_for_account = name.clone();
    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height,
        network_type: Some("mainnet".to_string()),
    };

    let dfvk_bytes = extsk.to_extended_fvk().to_bytes();
    let secret = WalletSecret {
        wallet_id: wallet_id.clone(),
        account_id: 0,
        extsk: extsk.to_bytes(),
        dfvk: Some(dfvk_bytes),
        orchard_extsk: Some(orchard_extsk.to_bytes()),
        sapling_ivk: None,
        orchard_ivk: None,
        encrypted_mnemonic: Some(mnemonic.as_bytes().to_vec()),
        mnemonic_language: Some(mnemonic_language.as_key().to_string()),
        created_at: chrono::Utc::now().timestamp(),
    };
    provision_seed_wallet(&meta, name_for_account, secret)?;
    tracing::info!(
        "Persisted wallet secret (Sapling + Ironwood) for wallet {}",
        wallet_id
    );

    Ok(wallet_id)
}

pub(super) fn restore_wallet(
    name: String,
    mnemonic: String,
    birthday_opt: Option<u32>,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    let (mnemonic, mnemonic_language) = canonicalize_mnemonic(&mnemonic, mnemonic_language)?;
    let network = pirate_params::Network::mainnet();
    let extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
        &mnemonic,
        network.network_type,
        0,
        Some(mnemonic_language),
    )?;
    let _wallet = Wallet::from_mnemonic_in_language(&mnemonic, Some(mnemonic_language))?;

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
        &mnemonic,
        Some(mnemonic_language),
    )?;
    let orchard_master = IronwoodExtendedSpendingKey::master(&seed_bytes)?;

    let coin_type = network.coin_type;
    let account = 0;
    let orchard_extsk = orchard_master.derive_account(coin_type, account)?;

    let birthday_height =
        birthday_opt.unwrap_or_else(|| pirate_params::Network::mainnet().default_birthday_height);

    let name_for_account = name.clone();
    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height,
        network_type: Some("mainnet".to_string()),
    };

    let dfvk_bytes = extsk.to_extended_fvk().to_bytes();
    let secret = WalletSecret {
        wallet_id: wallet_id.clone(),
        account_id: 0,
        extsk: extsk.to_bytes(),
        dfvk: Some(dfvk_bytes),
        orchard_extsk: Some(orchard_extsk.to_bytes()),
        sapling_ivk: None,
        orchard_ivk: None,
        encrypted_mnemonic: Some(mnemonic.as_bytes().to_vec()),
        mnemonic_language: Some(mnemonic_language.as_key().to_string()),
        created_at: chrono::Utc::now().timestamp(),
    };
    provision_seed_wallet(&meta, name_for_account, secret)?;
    tracing::info!("Persisted encrypted wallet secret for wallet {}", wallet_id);

    Ok(wallet_id)
}

fn persist_viewing_wallet_account(
    meta: &WalletMeta,
    sapling_dfvk: Option<Vec<u8>>,
    ironwood_fvk: Option<Vec<u8>>,
) -> Result<()> {
    let passphrase = app_passphrase()?;
    let (db, _key, _master_key) = open_wallet_db_with_passphrase(&meta.id, &passphrase)?;
    let repo = Repository::new(&db);
    let transaction = db.conn().unchecked_transaction()?;

    let result = (|| -> Result<()> {
        let account_id = repo.insert_account(&Account {
            id: None,
            name: meta.name.clone(),
            created_at: meta.created_at,
        })?;

        let secret = WalletSecret {
            wallet_id: meta.id.clone(),
            account_id,
            extsk: Vec::new(),
            dfvk: sapling_dfvk.clone(),
            orchard_extsk: None,
            sapling_ivk: None,
            orchard_ivk: ironwood_fvk.clone(),
            encrypted_mnemonic: None,
            mnemonic_language: None,
            created_at: meta.created_at,
        };
        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
        repo.upsert_wallet_secret(&encrypted_secret)?;

        let account_key = AccountKey {
            id: None,
            account_id,
            key_type: KeyType::ImportView,
            key_scope: KeyScope::Account,
            label: None,
            birthday_height: meta.birthday_height as i64,
            created_at: meta.created_at,
            spendable: false,
            sapling_extsk: None,
            sapling_dfvk,
            orchard_extsk: None,
            orchard_fvk: ironwood_fvk,
            encrypted_mnemonic: None,
        };
        let encrypted_key = repo.encrypt_account_key_fields(&account_key)?;
        let _ = repo.upsert_account_key(&encrypted_key)?;
        Ok(())
    })();

    result?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn import_viewing_wallet(
    name: String,
    sapling_viewing_key: Option<String>,
    ironwood_viewing_key: Option<String>,
    birthday: u32,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;
    let _wallet = Wallet::from_viewing_keys(
        sapling_viewing_key.as_deref(),
        ironwood_viewing_key.as_deref(),
    )?;

    let sapling_dfvk = sapling_viewing_key
        .as_deref()
        .map(|value| {
            ExtendedFullViewingKey::from_xfvk_bech32_any(value)
                .map(|key| key.to_bytes())
                .map_err(|_| anyhow!("Invalid Sapling viewing key (xFVK)"))
        })
        .transpose()?;
    let ironwood_fvk = ironwood_viewing_key
        .as_deref()
        .map(|value| {
            IronwoodExtendedFullViewingKey::from_bech32_any(value)
                .map(|key| key.to_bytes())
                .map_err(|_| anyhow!("Invalid Ironwood viewing key"))
        })
        .transpose()?;

    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: true,
        birthday_height: birthday,
        network_type: Some("mainnet".to_string()),
    };

    run_provisioning_steps(
        || persist_viewing_wallet_account(&meta, sapling_dfvk, ironwood_fvk),
        || register_wallet(&meta),
        || encrypted_db::remove_wallet_storage_artifacts(&meta.id),
    )?;

    Ok(wallet_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn persistence_failure_never_attempts_registry_publication() {
        let registered = Cell::new(false);
        let cleaned = Cell::new(false);

        let error = run_provisioning_steps(
            || Err(anyhow!("secret persistence failed")),
            || {
                registered.set(true);
                Ok(())
            },
            || cleaned.set(true),
        )
        .expect_err("persistence failure must abort provisioning");

        assert_eq!(error.to_string(), "secret persistence failed");
        assert!(!registered.get());
        assert!(cleaned.get());
    }

    #[test]
    fn registry_failure_cleans_durable_wallet_artifacts() {
        let steps = RefCell::new(Vec::new());

        let error = run_provisioning_steps(
            || {
                steps.borrow_mut().push("persist");
                Ok(())
            },
            || {
                steps.borrow_mut().push("register");
                Err(anyhow!("registry commit failed"))
            },
            || steps.borrow_mut().push("cleanup"),
        )
        .expect_err("registry failure must abort provisioning");

        assert_eq!(error.to_string(), "registry commit failed");
        assert_eq!(*steps.borrow(), ["persist", "register", "cleanup"]);
    }

    #[test]
    fn successful_provisioning_keeps_wallet_artifacts() {
        let steps = RefCell::new(Vec::new());

        run_provisioning_steps(
            || {
                steps.borrow_mut().push("persist");
                Ok(())
            },
            || {
                steps.borrow_mut().push("register");
                Ok(())
            },
            || steps.borrow_mut().push("cleanup"),
        )
        .unwrap();

        assert_eq!(*steps.borrow(), ["persist", "register"]);
    }
}
