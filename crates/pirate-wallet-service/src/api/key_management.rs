use super::*;
use bech32::{Bech32, Hrp};
use sapling::zip32::ExtendedFullViewingKey as SaplingExtendedFullViewingKey;
use zcash_client_backend::encoding::{
    encode_extended_full_viewing_key, encode_extended_spending_key,
};

pub(super) fn export_sapling_viewing_key(wallet_id: WalletId) -> Result<String> {
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

    ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let keys = repo.get_account_keys(secret.account_id)?;

    let mut items: Vec<KeyGroupInfo> = keys
        .into_iter()
        .filter_map(|key| {
            let id = key.id?;
            let has_sapling = key.sapling_extsk.is_some() || key.sapling_dfvk.is_some();
            let has_ironwood = key.orchard_extsk.is_some() || key.orchard_fvk.is_some();
            Some(KeyGroupInfo {
                id,
                label: key.label,
                key_type: key_type_to_info(key.key_type),
                spendable: key.spendable,
                has_sapling,
                has_ironwood,
                birthday_height: key.birthday_height,
                created_at: key.created_at,
            })
        })
        .collect();

    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(items)
}

pub(super) fn export_key_group_keys(wallet_id: WalletId, key_id: i64) -> Result<KeyExportInfo> {
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

    // Allocating the index and deriving+storing the address for it must be
    // one atomic operation - see allocate_next_diversified_address's doc
    // comment for the concurrent-caller collision this closes.
    let address = repo.allocate_next_diversified_address(
        account_id,
        key_id,
        pirate_storage_sqlite::AddressScope::External,
        address_type,
        move |next_index| {
            // The closure runs inside allocate_next_diversified_address's
            // transaction and is bounded by pirate-storage-sqlite's own
            // Result/Error type, not anyhow - map failures into its generic
            // `Storage` variant; the `?` on the outer call converts back to
            // anyhow::Error for this function's own Result.
            use pirate_storage_sqlite::Error as StorageError;
            let (addr_string, address_type) = if use_ironwood {
                let fvk_bytes = key.orchard_fvk.as_ref().ok_or_else(|| {
                    StorageError::Storage("Ironwood viewing key not available".to_string())
                })?;
                let fvk = IronwoodExtendedFullViewingKey::from_bytes(fvk_bytes).map_err(|e| {
                    StorageError::Storage(format!("Invalid Ironwood viewing key bytes: {e}"))
                })?;
                let addr = fvk
                    .address_at(next_index)
                    .encode_for_network(network_type)
                    .map_err(|e| {
                        StorageError::Storage(format!("Ironwood address encoding failed: {e}"))
                    })?;
                (addr, AddressType::Ironwood)
            } else {
                let dfvk_bytes = key.sapling_dfvk.as_ref().ok_or_else(|| {
                    StorageError::Storage("Sapling viewing key not available".to_string())
                })?;
                let dfvk = ExtendedFullViewingKey::from_bytes(dfvk_bytes).ok_or_else(|| {
                    StorageError::Storage("Invalid Sapling viewing key bytes".to_string())
                })?;
                let addr = dfvk
                    .derive_address(next_index)
                    .encode_for_network(network_type);
                (addr, AddressType::Sapling)
            };

            Ok(pirate_storage_sqlite::Address {
                id: None,
                key_id: Some(key_id),
                account_id,
                diversifier_index: next_index,
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
    sync_control::clear_wallet_data_caches(&wallet_id);
    Ok(key_id)
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
