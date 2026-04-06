use super::tunnel::tunnel_transport_config;
use super::*;

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
    secret: WalletSecret,
) -> Result<bool> {
    if let Ok((_db, repo)) = open_wallet_db_for(wallet_id) {
        let account = Account {
            id: None,
            name: account_name,
            created_at: chrono::Utc::now().timestamp(),
        };
        let account_id = repo.insert_account(&account)?;

        let mut secret = secret;
        secret.account_id = account_id;

        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
        repo.upsert_wallet_secret(&encrypted_secret)?;
        let _ = ensure_primary_account_key(&repo, wallet_id, &secret)?;
        return Ok(true);
    }

    Ok(false)
}

fn register_wallet(meta: &WalletMeta) -> Result<()> {
    WALLETS.write().push(meta.clone());
    *ACTIVE_WALLET.write() = Some(meta.id.clone());

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, meta)?;
    set_active_wallet_registry(&registry_db, Some(&meta.id))?;
    touch_wallet_last_used(&registry_db, &meta.id)?;
    Ok(())
}

pub(super) fn create_wallet(
    name: String,
    _entropy_len: Option<u32>,
    birthday_opt: Option<u32>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    let mnemonic = ExtendedSpendingKey::generate_mnemonic(Some(24));
    let network = pirate_params::Network::mainnet();
    let extsk =
        ExtendedSpendingKey::from_mnemonic_with_account(&mnemonic, network.network_type, 0)?;
    let _wallet = Wallet::from_mnemonic(&mnemonic)?;

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(&mnemonic)?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;

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

    register_wallet(&meta)?;

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
        created_at: chrono::Utc::now().timestamp(),
    };
    if persist_wallet_account_secret(&wallet_id, name_for_account, secret)? {
        tracing::info!(
            "Persisted wallet secret (Sapling + Orchard) for wallet {}",
            wallet_id
        );
    }

    Ok(wallet_id)
}

pub(super) fn restore_wallet(
    name: String,
    mnemonic: String,
    birthday_opt: Option<u32>,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;

    let network = pirate_params::Network::mainnet();
    let extsk =
        ExtendedSpendingKey::from_mnemonic_with_account(&mnemonic, network.network_type, 0)?;
    let _wallet = Wallet::from_mnemonic(&mnemonic)?;

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic(&mnemonic)?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;

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

    register_wallet(&meta)?;

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
        created_at: chrono::Utc::now().timestamp(),
    };
    if persist_wallet_account_secret(&wallet_id, name_for_account, secret)? {
        tracing::info!("Persisted encrypted wallet secret for wallet {}", wallet_id);
    }

    Ok(wallet_id)
}

pub(super) fn import_viewing_wallet(
    name: String,
    sapling_viewing_key: Option<String>,
    orchard_viewing_key: Option<String>,
    birthday: u32,
) -> Result<WalletId> {
    ensure_wallet_registry_loaded()?;
    let _wallet = Wallet::from_viewing_keys(
        sapling_viewing_key.as_deref(),
        orchard_viewing_key.as_deref(),
    )?;

    let wallet_id = uuid::Uuid::new_v4().to_string();
    let meta = WalletMeta {
        id: wallet_id.clone(),
        name,
        created_at: chrono::Utc::now().timestamp(),
        watch_only: true,
        birthday_height: birthday,
        network_type: Some("mainnet".to_string()),
    };

    let account_name = meta.name.clone();
    let account_created_at = meta.created_at;

    register_wallet(&meta)?;

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    let account = Account {
        id: None,
        name: account_name,
        created_at: account_created_at,
    };
    let account_id = repo.insert_account(&account)?;

    let mut dfvk_bytes: Option<Vec<u8>> = None;
    if let Some(ref value) = sapling_viewing_key {
        let dfvk = ExtendedFullViewingKey::from_xfvk_bech32_any(value)
            .map_err(|_| anyhow!("Invalid Sapling viewing key (xFVK)"))?;
        dfvk_bytes = Some(dfvk.to_bytes());
    }

    let mut orchard_fvk_bytes: Option<Vec<u8>> = None;
    if let Some(ref value) = orchard_viewing_key {
        let fvk = OrchardExtendedFullViewingKey::from_bech32_any(value)
            .map_err(|_| anyhow!("Invalid Orchard viewing key"))?;
        orchard_fvk_bytes = Some(fvk.to_bytes());
    }

    let dfvk_bytes_for_key = dfvk_bytes.clone();
    let orchard_fvk_bytes_for_key = orchard_fvk_bytes.clone();

    let secret = WalletSecret {
        wallet_id: wallet_id.clone(),
        account_id,
        extsk: Vec::new(),
        dfvk: dfvk_bytes,
        orchard_extsk: None,
        sapling_ivk: None,
        orchard_ivk: orchard_fvk_bytes,
        encrypted_mnemonic: None,
        created_at: account_created_at,
    };

    let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
    repo.upsert_wallet_secret(&encrypted_secret)?;

    let account_key = AccountKey {
        id: None,
        account_id,
        key_type: KeyType::ImportView,
        key_scope: KeyScope::Account,
        label: None,
        birthday_height: birthday as i64,
        created_at: account_created_at,
        spendable: false,
        sapling_extsk: None,
        sapling_dfvk: dfvk_bytes_for_key,
        orchard_extsk: None,
        orchard_fvk: orchard_fvk_bytes_for_key,
        encrypted_mnemonic: None,
    };
    let encrypted_key = repo.encrypt_account_key_fields(&account_key)?;
    let _ = repo.upsert_account_key(&encrypted_key)?;

    Ok(wallet_id)
}
