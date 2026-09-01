use super::*;
use bech32::{Bech32, Hrp};
use sapling::zip32::ExtendedFullViewingKey as SaplingExtendedFullViewingKey;
use zcash_client_backend::encoding::{
    encode_extended_full_viewing_key, encode_extended_spending_key,
};

const KEY_IMPORT_LOG_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AccountKeyInventory {
    account_key_count: usize,
    imported_spending_count: usize,
    sapling_imported_spending_count: usize,
    ironwood_imported_spending_count: usize,
}

impl AccountKeyInventory {
    fn from_account_keys(keys: &[AccountKey], seed_derived_key_ids: &HashSet<i64>) -> Self {
        let mut inventory = Self {
            account_key_count: keys.len(),
            ..Self::default()
        };

        for key in keys {
            if key.key_type != KeyType::ImportSpend
                || key
                    .id
                    .is_some_and(|key_id| seed_derived_key_ids.contains(&key_id))
            {
                continue;
            }

            inventory.imported_spending_count += 1;
            if key.sapling_extsk.is_some() || key.sapling_dfvk.is_some() {
                inventory.sapling_imported_spending_count += 1;
            }
            if key.orchard_extsk.is_some() || key.orchard_fvk.is_some() {
                inventory.ironwood_imported_spending_count += 1;
            }
        }

        inventory
    }
}

fn append_key_import_requested(
    wallet_id: &WalletId,
    birthday_height: u32,
    sapling_requested: bool,
    ironwood_requested: bool,
) {
    let timestamp = unix_timestamp_millis();
    let event = serde_json::json!({
        "id": "log_key_import_requested",
        "timestamp": timestamp,
        "location": "api::key_management::import_spending_key",
        "message": "spending key import requested",
        "data": {
            "schema_version": KEY_IMPORT_LOG_SCHEMA_VERSION,
            "wallet_id": wallet_id,
            "birthday_height": birthday_height,
            "sapling_requested": sapling_requested,
            "ironwood_requested": ironwood_requested,
        },
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "K",
    });
    pirate_core::debug_log::append_line(&event.to_string());
}

fn append_key_import_persisted(
    wallet_id: &WalletId,
    birthday_height: u32,
    sapling_stored: bool,
    ironwood_stored: bool,
    inventory: Option<AccountKeyInventory>,
) {
    let timestamp = unix_timestamp_millis();
    let event = serde_json::json!({
        "id": "log_key_import_persisted",
        "timestamp": timestamp,
        "location": "api::key_management::import_spending_key",
        "message": "spending key import persisted",
        "data": {
            "schema_version": KEY_IMPORT_LOG_SCHEMA_VERSION,
            "wallet_id": wallet_id,
            "birthday_height": birthday_height,
            "sapling_stored": sapling_stored,
            "ironwood_stored": ironwood_stored,
            "account_key_count": inventory.map(|value| value.account_key_count),
            "imported_spending_count": inventory.map(|value| value.imported_spending_count),
            "sapling_imported_spending_count": inventory
                .map(|value| value.sapling_imported_spending_count),
            "ironwood_imported_spending_count": inventory
                .map(|value| value.ironwood_imported_spending_count),
        },
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "K",
    });
    pirate_core::debug_log::append_line(&event.to_string());
}

pub(super) fn export_sapling_viewing_key(wallet_id: WalletId) -> Result<String> {
    require_wallet_signing_session(&wallet_id)?;
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export viewing key from watch-only wallet"));
    }

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    Ok(extsk.to_xfvk_bech32_for_network(network_type))
}

pub(super) fn export_ironwood_viewing_key(wallet_id: WalletId) -> Result<String> {
    require_wallet_signing_session(&wallet_id)?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    if let Some(ironwood_extsk_bytes) = secret.orchard_extsk.as_ref() {
        let ironwood_extsk = IronwoodExtendedSpendingKey::from_bytes(ironwood_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Ironwood spending key bytes: {}", e))?;
        let ironwood_fvk = ironwood_extsk.to_extended_fvk();
        ironwood_fvk
            .to_bech32_for_network(network_type)
            .map_err(|e| anyhow!("Failed to encode Ironwood viewing key: {}", e))
    } else {
        Err(anyhow!("Ironwood keys not available for this wallet"))
    }
}

pub(super) fn list_key_groups(wallet_id: WalletId) -> Result<Vec<KeyGroupInfo>> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    if repo.get_signing_protection(&wallet_id)?.is_none()
        || spending_protection::is_signing_session_unlocked(&wallet_id)
    {
        ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    }
    let keys = repo.get_account_keys(secret.account_id)?;
    let seed_derived = repo
        .get_seed_derived_account_keys(secret.account_id)?
        .into_iter()
        .map(|metadata| (metadata.key_id, metadata))
        .collect::<HashMap<_, _>>();

    let mut items: Vec<KeyGroupInfo> = keys
        .into_iter()
        .filter_map(|key| {
            let id = key.id?;
            let seed_derivation = seed_derived.get(&id);
            if seed_derivation.is_some_and(|metadata| metadata.is_discovery_candidate) {
                return None;
            }
            let has_sapling = key.sapling_extsk.is_some() || key.sapling_dfvk.is_some();
            let has_ironwood = key.orchard_extsk.is_some() || key.orchard_fvk.is_some();
            Some(KeyGroupInfo {
                id,
                label: key.label,
                key_type: if seed_derivation.is_some() {
                    KeyTypeInfo::Seed
                } else {
                    key_type_to_info(key.key_type)
                },
                seed_account_index: seed_derivation
                    .map(|metadata| metadata.derivation_index)
                    .or((key.key_type == KeyType::Seed).then_some(0)),
                spendable: key.spendable,
                has_sapling,
                has_ironwood,
                birthday_height: key.birthday_height,
                created_at: key.created_at,
            })
        })
        .collect();

    items.sort_by(|a, b| match (a.seed_account_index, b.seed_account_index) {
        (Some(a_index), Some(b_index)) => a_index.cmp(&b_index),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.created_at.cmp(&b.created_at),
    });
    Ok(items)
}

pub(super) fn export_key_group_keys(wallet_id: WalletId, key_id: i64) -> Result<KeyExportInfo> {
    require_wallet_signing_session(&wallet_id)?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;
    if key.account_id != secret.account_id {
        return Err(anyhow!("Key group does not belong to this wallet"));
    }

    let network_type = address_prefix_network_type(&wallet_id)?;

    let sapling_viewing_key = if let Some(ref bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
        Some(extsk.to_xfvk_bech32_for_network(network_type))
    } else if let Some(ref bytes) = key.sapling_dfvk {
        encode_sapling_xfvk_from_bytes(bytes, network_type)
    } else {
        None
    };

    let sapling_spending_key = if let Some(ref bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
        Some(encode_extended_spending_key(
            sapling_extsk_hrp_for_network(network_type),
            extsk.inner(),
        ))
    } else {
        None
    };

    let ironwood_viewing_key = if let Some(ref bytes) = key.orchard_extsk {
        let extsk = IronwoodExtendedSpendingKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid Ironwood spending key bytes: {}", e))?;
        Some(
            extsk
                .to_extended_fvk()
                .to_bech32_for_network(network_type)
                .map_err(|e| anyhow!("Failed to encode Ironwood viewing key: {}", e))?,
        )
    } else if let Some(ref bytes) = key.orchard_fvk {
        let fvk = IronwoodExtendedFullViewingKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid Ironwood viewing key bytes: {}", e))?;
        Some(
            fvk.to_bech32_for_network(network_type)
                .map_err(|e| anyhow!("Failed to encode Ironwood viewing key: {}", e))?,
        )
    } else {
        None
    };

    let ironwood_spending_key = if let Some(ref bytes) = key.orchard_extsk {
        let extsk = IronwoodExtendedSpendingKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid Ironwood spending key bytes: {}", e))?;
        Some(encode_ironwood_extsk(&extsk, network_type)?)
    } else {
        None
    };

    Ok(KeyExportInfo {
        key_id,
        sapling_viewing_key,
        ironwood_viewing_key,
        sapling_spending_key,
        ironwood_spending_key,
    })
}

pub(super) fn list_addresses_for_key(
    wallet_id: WalletId,
    key_id: i64,
) -> Result<Vec<KeyAddressInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    let mut addresses = repo.get_addresses_by_key(secret.account_id, key_id)?;
    addresses.retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);
    addresses.retain(|addr| {
        address_matches_expected_network_prefix(&addr.address, addr.address_type, network_type)
    });

    Ok(addresses
        .into_iter()
        .map(|addr| KeyAddressInfo {
            key_id,
            address: addr.address,
            diversifier_index: addr.diversifier_index,
            label: addr.label,
            created_at: addr.created_at,
            color_tag: address_book_color_to_ffi(addr.color_tag),
        })
        .collect())
}

pub(super) fn generate_address_for_key(
    wallet_id: WalletId,
    key_id: i64,
    use_ironwood: bool,
) -> Result<String> {
    if use_ironwood && !should_generate_ironwood(&wallet_id)? {
        return Err(anyhow!("Ironwood is not active for this wallet"));
    }
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;

    let account_id = key.account_id;
    let network_type = address_prefix_network_type(&wallet_id)?;
    let address_type = if use_ironwood {
        AddressType::Ironwood
    } else {
        AddressType::Sapling
    };
    let viewing_keys = super::addresses::viewing_keys_for_account_key(&key)?;
    super::addresses::backfill_full_diversifier_indices(
        &repo,
        account_id,
        network_type,
        &viewing_keys,
    )?;

    // Allocating the index and deriving+storing the address for it must be
    // one atomic operation - see allocate_next_diversified_address's doc
    // comment for the concurrent-caller collision this closes.
    let address = repo.allocate_next_diversified_address(
        account_id,
        key_id,
        pirate_storage_sqlite::AddressScope::External,
        address_type,
        move |next_index, next_index_88| {
            // The closure runs inside allocate_next_diversified_address's
            // transaction and is bounded by pirate-storage-sqlite's own
            // Result/Error type, not anyhow - map failures into its generic
            // `Storage` variant; the `?` on the outer call converts back to
            // anyhow::Error for this function's own Result.
            use pirate_storage_sqlite::Error as StorageError;
            let (addr_string, address_type, actual_index_88) = if use_ironwood {
                let fvk = if let Some(extsk_bytes) = key.orchard_extsk.as_deref() {
                    IronwoodExtendedSpendingKey::from_bytes(extsk_bytes)
                        .map_err(|e| {
                            StorageError::Storage(format!(
                                "Invalid Ironwood spending key bytes: {e}"
                            ))
                        })?
                        .to_extended_fvk()
                } else {
                    let fvk_bytes = key.orchard_fvk.as_ref().ok_or_else(|| {
                        StorageError::Storage("Ironwood viewing key not available".to_string())
                    })?;
                    IronwoodExtendedFullViewingKey::from_bytes(fvk_bytes).map_err(|e| {
                        StorageError::Storage(format!("Invalid Ironwood viewing key bytes: {e}"))
                    })?
                };
                let addr = fvk
                    .address_at_index(next_index_88)
                    .encode_for_network(network_type)
                    .map_err(|e| {
                        StorageError::Storage(format!("Ironwood address encoding failed: {e}"))
                    })?;
                (addr, AddressType::Ironwood, next_index_88)
            } else {
                let dfvk = if let Some(extsk_bytes) = key.sapling_extsk.as_deref() {
                    ExtendedSpendingKey::from_bytes(extsk_bytes)
                        .map_err(|e| {
                            StorageError::Storage(format!(
                                "Invalid Sapling spending key bytes: {e}"
                            ))
                        })?
                        .to_extended_fvk()
                } else {
                    let dfvk_bytes = key.sapling_dfvk.as_ref().ok_or_else(|| {
                        StorageError::Storage("Sapling viewing key not available".to_string())
                    })?;
                    ExtendedFullViewingKey::from_bytes(dfvk_bytes).ok_or_else(|| {
                        StorageError::Storage("Invalid Sapling viewing key bytes".to_string())
                    })?
                };
                let (actual_index, address) =
                    dfvk.find_address_from_index(next_index_88).ok_or_else(|| {
                        StorageError::Storage(
                            "Sapling diversifier index space is exhausted".to_string(),
                        )
                    })?;
                let addr = address.encode_for_network(network_type);
                (addr, AddressType::Sapling, actual_index)
            };

            Ok(pirate_storage_sqlite::Address {
                id: None,
                key_id: Some(key_id),
                account_id,
                diversifier_index: next_index,
                diversifier_index_88: Some(actual_index_88),
                address: addr_string,
                address_type,
                label: None,
                created_at: chrono::Utc::now().timestamp(),
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: pirate_storage_sqlite::AddressScope::External,
            })
        },
    )?;

    Ok(address.address)
}

pub(super) fn import_spending_key(
    wallet_id: WalletId,
    sapling_key: Option<String>,
    ironwood_key: Option<String>,
    label: Option<String>,
    birthday_height: u32,
) -> Result<i64> {
    let sapling_requested = sapling_key.is_some();
    let ironwood_requested = ironwood_key.is_some();
    append_key_import_requested(
        &wallet_id,
        birthday_height,
        sapling_requested,
        ironwood_requested,
    );

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    if sapling_key.is_none() && ironwood_key.is_none() {
        return Err(anyhow!("Provide a Sapling or Ironwood spending key"));
    }

    let wallet_network = wallet_network_type(&wallet_id)?;
    let mut sapling_extsk = None;
    let mut sapling_dfvk = None;
    let mut orchard_extsk = None;
    let mut orchard_fvk = None;
    let mut network_from_key: Option<NetworkType> = None;

    if let Some(value) = sapling_key.as_ref() {
        let (extsk, network) = ExtendedSpendingKey::from_bech32_any(value)
            .map_err(|e| anyhow!("Invalid Sapling spending key: {}", e))?;
        if network != wallet_network {
            return Err(anyhow!(
                "Sapling spending key network ({}) does not match wallet network ({})",
                network_type_name(network),
                network_type_name(wallet_network)
            ));
        }
        network_from_key = Some(network);
        sapling_dfvk = Some(extsk.to_extended_fvk().to_bytes());
        sapling_extsk = Some(extsk.to_bytes());
    }

    if let Some(value) = ironwood_key.as_ref() {
        let (extsk, network) = IronwoodExtendedSpendingKey::from_bech32_any(value)
            .map_err(|e| anyhow!("Invalid Ironwood spending key: {}", e))?;
        if network != wallet_network {
            return Err(anyhow!(
                "Ironwood spending key network ({}) does not match wallet network ({})",
                network_type_name(network),
                network_type_name(wallet_network)
            ));
        }
        if let Some(existing) = network_from_key {
            if existing != network {
                return Err(anyhow!(
                    "Sapling and Ironwood keys are for different networks"
                ));
            }
        }
        orchard_fvk = Some(extsk.to_extended_fvk().to_bytes());
        orchard_extsk = Some(extsk.to_bytes());
    }

    let key = AccountKey {
        id: None,
        account_id: secret.account_id,
        key_type: KeyType::ImportSpend,
        key_scope: KeyScope::Account,
        label,
        birthday_height: birthday_height as i64,
        created_at: chrono::Utc::now().timestamp(),
        spendable: true,
        sapling_extsk,
        sapling_dfvk,
        orchard_extsk,
        orchard_fvk,
        encrypted_mnemonic: None,
    };

    let encrypted = repo.encrypt_account_key_fields(&key)?;
    let key_id = repo
        .upsert_account_key(&encrypted)
        .map_err(|e| anyhow!(e.to_string()))?;
    let inventory = repo.get_account_keys(secret.account_id).ok().map(|keys| {
        let seed_derived_key_ids = repo
            .get_seed_derived_account_keys(secret.account_id)
            .unwrap_or_default()
            .into_iter()
            .map(|metadata| metadata.key_id)
            .collect::<HashSet<_>>();
        AccountKeyInventory::from_account_keys(&keys, &seed_derived_key_ids)
    });
    append_key_import_persisted(
        &wallet_id,
        birthday_height,
        sapling_requested,
        ironwood_requested,
        inventory,
    );
    sync_control::clear_wallet_data_caches(&wallet_id);
    Ok(key_id)
}

struct VerifiedSpendingKeyMaterial {
    canonical_address: String,
    diversifier_index_88: [u8; 11],
    sapling_extsk: Option<Vec<u8>>,
    sapling_dfvk: Option<Vec<u8>>,
    orchard_extsk: Option<Vec<u8>>,
    orchard_fvk: Option<Vec<u8>>,
}

fn normalized_bech32_input(value: &str) -> String {
    let has_lowercase = value.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_uppercase = value.bytes().any(|byte| byte.is_ascii_uppercase());
    if has_uppercase && !has_lowercase {
        value.to_ascii_lowercase()
    } else {
        value.to_string()
    }
}

fn verify_spending_key_address(
    pool: VerifiedSpendingKeyPool,
    spending_key: &str,
    expected_address: &str,
    _address_index: u32,
    wallet_network: NetworkType,
) -> Result<VerifiedSpendingKeyMaterial> {
    // `address_index` is retained as legacy display metadata by the caller.
    // Ownership is proven directly by reversing the encoded address with the
    // full viewing key; scanning an ordinal range would be both unnecessary
    // and an attacker-controlled CPU cost.
    let key_for_decode = normalized_bech32_input(spending_key);
    let address_for_decode = normalized_bech32_input(expected_address);
    match pool {
        VerifiedSpendingKeyPool::Sapling => {
            let (extsk, network) = ExtendedSpendingKey::from_bech32_any(&key_for_decode)
                .map_err(|_| anyhow!("Invalid Sapling spending key"))?;
            if network != wallet_network {
                return Err(anyhow!(
                    "Spending key network does not match wallet network"
                ));
            }
            let decoded = PaymentAddress::decode_for_network(wallet_network, &address_for_decode)
                .map_err(|_| anyhow!("Invalid Sapling address for wallet network"))?;
            let canonical = decoded.encode_for_network(wallet_network);
            let dfvk = extsk.to_extended_fvk();
            let diversifier_index_88 = dfvk
                .diversifier_index(&decoded)
                .filter(|(_, scope)| *scope == pirate_core::keys::DiversifierScope::External)
                .map(|(index, _)| index)
                .ok_or_else(|| anyhow!("Expected address is not controlled by the spending key"))?;
            if !canonical.eq_ignore_ascii_case(expected_address) {
                return Err(anyhow!(
                    "Expected address is not controlled by the spending key"
                ));
            }
            Ok(VerifiedSpendingKeyMaterial {
                canonical_address: canonical,
                diversifier_index_88,
                sapling_extsk: Some(extsk.to_bytes()),
                sapling_dfvk: Some(dfvk.to_bytes()),
                orchard_extsk: None,
                orchard_fvk: None,
            })
        }
        VerifiedSpendingKeyPool::Ironwood => {
            let (extsk, network) = IronwoodExtendedSpendingKey::from_bech32_any(&key_for_decode)
                .map_err(|_| anyhow!("Invalid Ironwood spending key"))?;
            if network != wallet_network {
                return Err(anyhow!(
                    "Spending key network does not match wallet network"
                ));
            }
            let decoded = IronwoodPaymentAddress::decode_any_network(&address_for_decode)
                .map_err(|_| anyhow!("Invalid Ironwood address for wallet network"))?;
            let canonical = decoded
                .encode_for_network(wallet_network)
                .map_err(|_| anyhow!("Invalid Ironwood address for wallet network"))?;
            let fvk = extsk.to_extended_fvk();
            let diversifier_index_88 = fvk
                .diversifier_index(&decoded, pirate_core::keys::DiversifierScope::External)
                .ok_or_else(|| anyhow!("Expected address is not controlled by the spending key"))?;
            if !canonical.eq_ignore_ascii_case(expected_address) {
                return Err(anyhow!(
                    "Expected address is not controlled by the spending key"
                ));
            }
            Ok(VerifiedSpendingKeyMaterial {
                canonical_address: canonical,
                diversifier_index_88,
                sapling_extsk: None,
                sapling_dfvk: None,
                orchard_extsk: Some(extsk.to_bytes()),
                orchard_fvk: Some(fvk.to_bytes()),
            })
        }
    }
}

fn validate_import_birthday(birthday_height: u32, known_tip: u64) -> Result<()> {
    if birthday_height == 0 {
        return Err(anyhow!("Birthday height must be greater than zero"));
    }
    if known_tip == 0 {
        return Err(anyhow!(
            "Wallet chain tip is unknown; synchronize before importing a key"
        ));
    }
    if u64::from(birthday_height) > known_tip {
        return Err(anyhow!(
            "Birthday height exceeds the wallet's known chain tip"
        ));
    }
    Ok(())
}

pub(super) async fn import_spending_key_verified(
    wallet_id: WalletId,
    pool: VerifiedSpendingKeyPool,
    spending_key: String,
    expected_address: String,
    address_index: u32,
    label: Option<String>,
    birthday_height: u32,
) -> Result<VerifiedSpendingKeyImport> {
    let label = label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if label
        .as_ref()
        .is_some_and(|value| value.len() > pirate_storage_sqlite::MAX_LABEL_LENGTH)
    {
        return Err(anyhow!("Import label is too long"));
    }

    // SyncEngine snapshots account keys when it is constructed. Stop and join
    // an existing engine before changing the inventory, and keep startup/rescan
    // serialized until the atomic storage transaction commits.
    let _sync_operation_guard = sync_control::acquire_exclusive_key_import(&wallet_id).await?;
    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found"))?;
    let wallet_network = wallet_network_type(&wallet_id)?;
    let known_tip = pirate_storage_sqlite::SpendabilityStateStorage::new(&db)
        .load_state()?
        .target_height;
    validate_import_birthday(birthday_height, known_tip)?;

    let material = verify_spending_key_address(
        pool,
        &spending_key,
        &expected_address,
        address_index,
        wallet_network,
    )?;
    let key = AccountKey {
        id: None,
        account_id: secret.account_id,
        key_type: KeyType::ImportSpend,
        key_scope: KeyScope::Account,
        label,
        birthday_height: i64::from(birthday_height),
        created_at: chrono::Utc::now().timestamp(),
        spendable: true,
        sapling_extsk: material.sapling_extsk,
        sapling_dfvk: material.sapling_dfvk,
        orchard_extsk: material.orchard_extsk,
        orchard_fvk: material.orchard_fvk,
        encrypted_mnemonic: None,
    };

    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: None,
        account_id: secret.account_id,
        diversifier_index: address_index,
        diversifier_index_88: Some(material.diversifier_index_88),
        address: material.canonical_address.clone(),
        address_type: match pool {
            VerifiedSpendingKeyPool::Sapling => AddressType::Sapling,
            VerifiedSpendingKeyPool::Ironwood => AddressType::Ironwood,
        },
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };

    let (key_id, already_imported, effective_birthday, rescan_required) = repo
        .import_verified_spending_key(&key, &address, SPENDABILITY_REASON_ERR_RESCAN_REQUIRED)
        .map_err(|error| anyhow!(error.to_string()))?;
    let spendability_state = pirate_storage_sqlite::SpendabilityStateStorage::new(&db)
        .load_state()
        .map_err(|error| anyhow!(error.to_string()))?;
    sync_control::clear_wallet_data_caches(&wallet_id);

    Ok(VerifiedSpendingKeyImport {
        key_id,
        pool,
        address: material.canonical_address,
        address_index,
        birthday_height: u32::try_from(effective_birthday)
            .map_err(|_| anyhow!("Stored birthday height is invalid"))?,
        already_imported,
        rescan_required,
        required_rescan_from_height: (spendability_state.required_rescan_from_height > 0)
            .then(|| {
                u32::try_from(spendability_state.required_rescan_from_height)
                    .map_err(|_| anyhow!("Required rescan height is invalid"))
            })
            .transpose()?,
    })
}

fn key_type_to_info(key_type: KeyType) -> KeyTypeInfo {
    match key_type {
        KeyType::Seed => KeyTypeInfo::Seed,
        KeyType::ImportSpend => KeyTypeInfo::ImportedSpending,
        KeyType::ImportView => KeyTypeInfo::ImportedViewing,
    }
}

fn sapling_extfvk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "zxviews",
        NetworkType::Testnet => "zxviewtestsapling",
        NetworkType::Regtest => "zxviewregtestsapling",
    }
}

fn sapling_extsk_hrp_for_network(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "secret-extended-key-main",
        NetworkType::Testnet => "secret-extended-key-test",
        NetworkType::Regtest => "secret-extended-key-regtest",
    }
}

fn encode_sapling_xfvk_from_bytes(bytes: &[u8], network: NetworkType) -> Option<String> {
    if bytes.len() != 169 {
        return None;
    }
    let extfvk = SaplingExtendedFullViewingKey::read(&mut &bytes[..]).ok()?;
    Some(encode_extended_full_viewing_key(
        sapling_extfvk_hrp_for_network(network),
        &extfvk,
    ))
}

fn encode_ironwood_extsk(
    extsk: &IronwoodExtendedSpendingKey,
    network: NetworkType,
) -> Result<String> {
    let hrp = Hrp::parse(ironwood_extsk_hrp_for_network(network))
        .map_err(|e| anyhow!("Invalid Ironwood HRP: {}", e))?;
    bech32::encode::<Bech32>(hrp, &extsk.to_bytes())
        .map_err(|e| anyhow!("Bech32 encoding failed: {}", e))
}

fn network_type_name(network: NetworkType) -> &'static str {
    match network {
        NetworkType::Mainnet => "mainnet",
        NetworkType::Testnet => "testnet",
        NetworkType::Regtest => "regtest",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_key(key_type: KeyType, sapling: bool, ironwood: bool) -> AccountKey {
        AccountKey {
            id: None,
            account_id: 1,
            key_type,
            key_scope: KeyScope::Account,
            label: None,
            birthday_height: 1,
            created_at: 1,
            spendable: key_type != KeyType::ImportView,
            sapling_extsk: sapling.then(|| vec![0x11]),
            sapling_dfvk: None,
            orchard_extsk: ironwood.then(|| vec![0x22]),
            orchard_fvk: None,
            encrypted_mnemonic: None,
        }
    }

    #[test]
    fn account_key_inventory_counts_imported_pools_from_metadata() {
        let mut seed_derived = account_key(KeyType::ImportSpend, true, false);
        seed_derived.id = Some(42);
        let keys = vec![
            account_key(KeyType::Seed, true, true),
            account_key(KeyType::ImportSpend, true, false),
            account_key(KeyType::ImportSpend, false, true),
            account_key(KeyType::ImportSpend, true, true),
            account_key(KeyType::ImportView, true, false),
            seed_derived,
        ];

        let inventory = AccountKeyInventory::from_account_keys(&keys, &HashSet::from([42]));

        assert_eq!(inventory.account_key_count, 6);
        assert_eq!(inventory.imported_spending_count, 3);
        assert_eq!(inventory.sapling_imported_spending_count, 2);
        assert_eq!(inventory.ironwood_imported_spending_count, 2);
    }
}

#[cfg(test)]
mod verified_import_tests {
    use super::*;

    const MNEMONIC: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const OTHER_MNEMONIC: &str =
        "legal winner thank year wave sausage worth useful legal winner thank yellow";

    fn sapling_key_and_address(network: NetworkType, index: u32) -> (String, String) {
        let extsk = ExtendedSpendingKey::from_mnemonic_with_account(MNEMONIC, network, 0).unwrap();
        let key =
            encode_extended_spending_key(sapling_extsk_hrp_for_network(network), extsk.inner());
        let address = extsk
            .to_extended_fvk()
            .derive_address(index)
            .encode_for_network(network);
        (key, address)
    }

    fn ironwood_key_and_address(network: NetworkType, index: u32) -> (String, String) {
        let seed = ExtendedSpendingKey::seed_bytes_from_mnemonic(MNEMONIC).unwrap();
        let extsk = IronwoodExtendedSpendingKey::master(&seed)
            .unwrap()
            .derive_account(133, 0)
            .unwrap();
        let key = encode_ironwood_extsk(&extsk, network).unwrap();
        let address = extsk
            .to_extended_fvk()
            .address_at(index)
            .encode_for_network(network)
            .unwrap();
        (key, address)
    }

    #[test]
    fn verifies_sapling_and_ironwood_address_ownership() {
        let (sapling_key, sapling_address) = sapling_key_and_address(NetworkType::Mainnet, 7);
        let sapling = verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &sapling_key,
            &sapling_address,
            7,
            NetworkType::Mainnet,
        )
        .unwrap();
        assert_eq!(sapling.canonical_address, sapling_address);
        assert!(sapling.sapling_extsk.is_some());
        assert!(sapling.orchard_extsk.is_none());

        let (ironwood_key, ironwood_address) = ironwood_key_and_address(NetworkType::Mainnet, 3);
        let ironwood = verify_spending_key_address(
            VerifiedSpendingKeyPool::Ironwood,
            &ironwood_key,
            &ironwood_address,
            3,
            NetworkType::Mainnet,
        )
        .unwrap();
        assert_eq!(ironwood.canonical_address, ironwood_address);
        assert!(ironwood.orchard_extsk.is_some());
        assert!(ironwood.sapling_extsk.is_none());
    }

    #[test]
    fn accepts_uppercase_keys_and_addresses_and_returns_canonical_address() {
        let (sapling_key, sapling_address) = sapling_key_and_address(NetworkType::Mainnet, 4);
        let sapling = verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &sapling_key.to_ascii_uppercase(),
            &sapling_address.to_ascii_uppercase(),
            4,
            NetworkType::Mainnet,
        )
        .unwrap();
        assert_eq!(sapling.canonical_address, sapling_address);

        let (ironwood_key, ironwood_address) = ironwood_key_and_address(NetworkType::Mainnet, 4);
        let ironwood = verify_spending_key_address(
            VerifiedSpendingKeyPool::Ironwood,
            &ironwood_key.to_ascii_uppercase(),
            &ironwood_address.to_ascii_uppercase(),
            4,
            NetworkType::Mainnet,
        )
        .unwrap();
        assert_eq!(ironwood.canonical_address, ironwood_address);

        let seed = ExtendedSpendingKey::seed_bytes_from_mnemonic(MNEMONIC).unwrap();
        let wrong_network_address = IronwoodExtendedSpendingKey::master(&seed)
            .unwrap()
            .derive_account(133, 0)
            .unwrap()
            .to_extended_fvk()
            .address_at(4)
            .encode_for_network(NetworkType::Testnet)
            .unwrap()
            .to_ascii_uppercase();
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Ironwood,
            &ironwood_key,
            &wrong_network_address,
            4,
            NetworkType::Mainnet,
        )
        .is_err());

        let mixed_case_address = format!("Z{}", &sapling_address[1..]);
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &sapling_key,
            &mixed_case_address,
            4,
            NetworkType::Mainnet,
        )
        .is_err());

        let mixed_case_key = format!("S{}", &sapling_key[1..]);
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &mixed_case_key,
            &sapling_address,
            4,
            NetworkType::Mainnet,
        )
        .is_err());
    }

    #[test]
    fn verifies_address_ownership_independently_of_legacy_sequence_metadata() {
        let (key, address) = sapling_key_and_address(NetworkType::Mainnet, 2);
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &key,
            &address,
            3,
            NetworkType::Mainnet,
        )
        .is_ok());
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &key,
            &address,
            u32::MAX,
            NetworkType::Mainnet,
        )
        .is_ok());

        let other = ExtendedSpendingKey::from_mnemonic_with_account(
            OTHER_MNEMONIC,
            NetworkType::Mainnet,
            0,
        )
        .unwrap();
        let other_key = encode_extended_spending_key(
            sapling_extsk_hrp_for_network(NetworkType::Mainnet),
            other.inner(),
        );
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &other_key,
            &address,
            2,
            NetworkType::Mainnet,
        )
        .is_err());

        let (testnet_key, testnet_address) = sapling_key_and_address(NetworkType::Testnet, 2);
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &testnet_key.to_ascii_uppercase(),
            &testnet_address,
            2,
            NetworkType::Mainnet,
        )
        .is_err());
    }

    #[test]
    fn accepts_the_full_legacy_sequence_range_without_scanning_it() {
        let (key, address) = sapling_key_and_address(NetworkType::Mainnet, 2);
        assert!(verify_spending_key_address(
            VerifiedSpendingKeyPool::Sapling,
            &key,
            &address,
            u32::MAX,
            NetworkType::Mainnet,
        )
        .is_ok());
    }

    #[test]
    fn birthday_requires_a_known_tip_and_must_not_exceed_it() {
        assert!(validate_import_birthday(1, 0).is_err());
        assert!(validate_import_birthday(0, 100).is_err());
        assert!(validate_import_birthday(100, 100).is_ok());
        assert!(validate_import_birthday(101, 100).is_err());
    }
}
