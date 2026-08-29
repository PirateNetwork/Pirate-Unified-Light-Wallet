use super::{
    address_book_color_to_ffi, address_matches_expected_network_prefix,
    address_prefix_network_type, ensure_primary_account_key, is_decoy_mode_active,
    open_wallet_db_for, should_generate_ironwood,
};
use crate::models::{AddressBalanceInfo, AddressInfo, WalletId};
use anyhow::{anyhow, Result};
use orchard::Address as IronwoodAddress;
use pirate_core::keys::{
    DiversifierScope, ExtendedFullViewingKey, ExtendedSpendingKey, IronwoodExtendedFullViewingKey,
    IronwoodExtendedSpendingKey, IronwoodPaymentAddress, PaymentAddress,
};
use pirate_params::NetworkType;
use pirate_storage_sqlite::{AddressType, Repository};
use sapling::PaymentAddress as SaplingPaymentAddress;
use std::collections::HashMap;

pub(super) struct AddressViewingKeys {
    key_id: i64,
    sapling: Option<ExtendedFullViewingKey>,
    ironwood: Option<IronwoodExtendedFullViewingKey>,
}

pub(super) fn viewing_keys_for_account_key(
    key: &pirate_storage_sqlite::AccountKey,
) -> Result<AddressViewingKeys> {
    let key_id = key
        .id
        .ok_or_else(|| anyhow!("Key group is missing its id"))?;
    let sapling = if let Some(bytes) = key.sapling_extsk.as_deref() {
        Some(ExtendedSpendingKey::from_bytes(bytes)?.to_extended_fvk())
    } else {
        key.sapling_dfvk
            .as_deref()
            .and_then(ExtendedFullViewingKey::from_bytes)
    };
    let ironwood = if let Some(bytes) = key.orchard_extsk.as_deref() {
        Some(IronwoodExtendedSpendingKey::from_bytes(bytes)?.to_extended_fvk())
    } else {
        key.orchard_fvk
            .as_deref()
            .map(IronwoodExtendedFullViewingKey::from_bytes)
            .transpose()?
    };
    Ok(AddressViewingKeys {
        key_id,
        sapling,
        ironwood,
    })
}

fn storage_scope(scope: DiversifierScope) -> pirate_storage_sqlite::AddressScope {
    match scope {
        DiversifierScope::External => pirate_storage_sqlite::AddressScope::External,
        DiversifierScope::Internal => pirate_storage_sqlite::AddressScope::Internal,
    }
}

fn recover_address_index(
    address: &str,
    address_type: AddressType,
    network: NetworkType,
    keys: &AddressViewingKeys,
) -> Option<([u8; 11], pirate_storage_sqlite::AddressScope)> {
    match address_type {
        AddressType::Sapling => {
            let address = PaymentAddress::decode_for_network(network, address).ok()?;
            let (index, scope) = keys.sapling.as_ref()?.diversifier_index(&address)?;
            Some((index, storage_scope(scope)))
        }
        AddressType::Ironwood => {
            let address = IronwoodPaymentAddress::decode_for_network(network, address).ok()?;
            let fvk = keys.ironwood.as_ref()?;
            for scope in [DiversifierScope::External, DiversifierScope::Internal] {
                if let Some(index) = fvk.diversifier_index(&address, scope) {
                    return Some((index, storage_scope(scope)));
                }
            }
            None
        }
    }
}

pub(super) fn backfill_full_diversifier_indices(
    repo: &Repository<'_>,
    account_id: i64,
    network: NetworkType,
    keys: &AddressViewingKeys,
) -> Result<()> {
    for mut address in repo.get_addresses_by_key(account_id, keys.key_id)? {
        let Some((index, scope)) =
            recover_address_index(&address.address, address.address_type, network, keys)
        else {
            continue;
        };
        if address.key_id == Some(keys.key_id)
            && address.diversifier_index_88 == Some(index)
            && address.address_scope == scope
        {
            continue;
        }
        address.key_id = Some(keys.key_id);
        address.diversifier_index_88 = Some(index);
        address.address_scope = scope;
        repo.repair_address_ownership(&address)?;
    }
    Ok(())
}

pub(super) fn current_receive_address(wallet_id: WalletId) -> Result<String> {
    if is_decoy_mode_active() {
        return Ok(String::new());
    }
    tracing::info!("Getting current receive address for wallet {}", wallet_id);

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let use_ironwood = should_generate_ironwood(&wallet_id)?;
    let address_type = if use_ironwood {
        AddressType::Ironwood
    } else {
        AddressType::Sapling
    };
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Primary key group not found"))?;
    let viewing_keys = viewing_keys_for_account_key(&key)?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    backfill_full_diversifier_indices(&repo, secret.account_id, network_type, &viewing_keys)?;
    let current_index = repo.get_current_diversifier_index_for_scope_and_type(
        secret.account_id,
        key_id,
        pirate_storage_sqlite::AddressScope::External,
        address_type,
    )?;

    if let Some(addr_record) = repo.get_address_by_index_for_scope_and_type(
        secret.account_id,
        key_id,
        current_index,
        pirate_storage_sqlite::AddressScope::External,
        address_type,
    )? {
        tracing::debug!(
            "Found existing address at index {}: {}",
            current_index,
            addr_record.address
        );
        return Ok(addr_record.address);
    }

    let extsk = if use_ironwood {
        None
    } else {
        Some(
            ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?,
        )
    };
    let (addr_string, address_type, diversifier_index_88) =
        derive_receive_address(&wallet_id, &secret, extsk.as_ref(), [0; 11], use_ironwood)?;

    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id: secret.account_id,
        diversifier_index: current_index,
        diversifier_index_88: Some(diversifier_index_88),
        address: addr_string.clone(),
        address_type,
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };
    repo.upsert_address(&address)?;

    tracing::debug!(
        "Generated and stored {} address at index {}: {}",
        if use_ironwood { "Ironwood" } else { "Sapling" },
        current_index,
        addr_string
    );
    Ok(addr_string)
}

pub(super) fn next_receive_address(wallet_id: WalletId) -> Result<String> {
    if is_decoy_mode_active() {
        return Ok(String::new());
    }
    tracing::info!("Generating next receive address for wallet {}", wallet_id);

    let use_ironwood = should_generate_ironwood(&wallet_id)?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let address_type = if use_ironwood {
        AddressType::Ironwood
    } else {
        AddressType::Sapling
    };
    let extsk = if use_ironwood {
        None
    } else {
        Some(
            ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?,
        )
    };
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Primary key group not found"))?;
    let viewing_keys = viewing_keys_for_account_key(&key)?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    backfill_full_diversifier_indices(&repo, secret.account_id, network_type, &viewing_keys)?;
    let account_id = secret.account_id;
    let wallet_id_for_derivation = wallet_id.clone();
    let address = repo.allocate_next_diversified_address(
        account_id,
        key_id,
        pirate_storage_sqlite::AddressScope::External,
        address_type,
        move |next_index, next_index_88| {
            let (addr_string, address_type, actual_index_88) = derive_receive_address(
                &wallet_id_for_derivation,
                &secret,
                extsk.as_ref(),
                next_index_88,
                use_ironwood,
            )
            .map_err(|e| pirate_storage_sqlite::Error::Storage(e.to_string()))?;

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

    tracing::info!(
        "Generated and stored next {} address at index {}: {}",
        if use_ironwood { "Ironwood" } else { "Sapling" },
        address.diversifier_index,
        address.address
    );
    Ok(address.address)
}

pub(super) fn list_addresses(wallet_id: WalletId) -> Result<Vec<AddressInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    let mut addresses = repo.get_all_addresses(secret.account_id)?;
    addresses.retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);
    addresses.retain(|addr| {
        address_matches_expected_network_prefix(&addr.address, addr.address_type, network_type)
    });

    Ok(addresses
        .into_iter()
        .map(|addr| AddressInfo {
            address: addr.address,
            diversifier_index: addr.diversifier_index,
            label: addr.label,
            created_at: addr.created_at,
            color_tag: address_book_color_to_ffi(addr.color_tag),
        })
        .collect())
}

/// Lists external receive-address balances by default. A key-group query also
/// exposes that group's internal change-address rows for account inspection.
pub(super) fn list_address_balances(
    wallet_id: WalletId,
    key_id: Option<i64>,
) -> Result<Vec<AddressBalanceInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    let orchard_active = should_generate_ironwood(&wallet_id)?;
    let selected_key_id = key_id;
    let account_keys = repo.get_account_keys(secret.account_id)?;
    if let Some(id) = selected_key_id {
        if !account_keys.iter().any(|key| key.id == Some(id)) {
            return Err(anyhow!(
                "Key group {} does not belong to wallet {}",
                id,
                wallet_id
            ));
        }
    }
    let viewing_keys = account_keys
        .iter()
        .filter(|key| selected_key_id.is_none_or(|selected| key.id == Some(selected)))
        .map(viewing_keys_for_account_key)
        .collect::<Result<Vec<_>>>()?;
    for keys in &viewing_keys {
        backfill_full_diversifier_indices(&repo, secret.account_id, network_type, keys)?;
    }

    let mut notes = repo.get_unspent_notes(secret.account_id)?;
    let has_orchard_note_bytes = notes.iter().any(|note| {
        note.note_type == pirate_storage_sqlite::models::NoteType::Ironwood
            && note
                .note
                .as_ref()
                .map(|bytes| !bytes.is_empty())
                .unwrap_or(false)
    });
    let orchard_enabled_for_balance = orchard_active || has_orchard_note_bytes;
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height;
    const MIN_DEPTH: u64 = 1;
    let confirmation_threshold = current_height.saturating_sub(MIN_DEPTH.saturating_sub(1));

    let created_at = chrono::Utc::now().timestamp();
    for note in notes.iter_mut() {
        let Some(note_bytes) = note.note.as_deref() else {
            continue;
        };
        let Some(address_string) = note_address_string(
            note.note_type,
            note_bytes,
            network_type,
            orchard_enabled_for_balance,
        ) else {
            continue;
        };
        let address_type = match note.note_type {
            pirate_storage_sqlite::models::NoteType::Sapling => AddressType::Sapling,
            pirate_storage_sqlite::models::NoteType::Ironwood => AddressType::Ironwood,
        };
        let existing = repo.get_address_by_string(secret.account_id, &address_string)?;
        let recovered = viewing_keys.iter().find_map(|keys| {
            recover_address_index(&address_string, address_type, network_type, keys)
                .map(|(index, scope)| (keys.key_id, index, scope))
        });
        let mut note_key_changed = false;
        if let Some((owner_key_id, index, scope)) = recovered {
            let address_record = pirate_storage_sqlite::Address {
                id: existing.as_ref().and_then(|address| address.id),
                key_id: Some(owner_key_id),
                account_id: secret.account_id,
                diversifier_index: match existing.as_ref() {
                    Some(address) => address.diversifier_index,
                    None => repo.get_next_diversifier_index_for_scope_and_type(
                        secret.account_id,
                        owner_key_id,
                        scope,
                        address_type,
                    )?,
                },
                diversifier_index_88: Some(index),
                address: address_string.clone(),
                address_type,
                label: None,
                created_at,
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: scope,
            };
            let address_changed = existing.as_ref().is_none_or(|address| {
                address.key_id != Some(owner_key_id)
                    || address.address_type != address_type
                    || address.address_scope != scope
                    || address.diversifier_index_88 != Some(index)
            });
            if address_changed {
                if existing.is_some() {
                    repo.repair_address_ownership(&address_record)?;
                } else {
                    repo.upsert_address(&address_record)?;
                }
            }
            note_key_changed = note.key_id != Some(owner_key_id);
            if note_key_changed {
                note.key_id = Some(owner_key_id);
            }
        }
        if let Some(addr) = repo
            .get_address_by_string(secret.account_id, &address_string)?
            .and_then(|addr| addr.id)
        {
            if note.address_id != Some(addr) || note_key_changed {
                note.address_id = Some(addr);
                repo.update_note_by_id(note)?;
            }
        }
    }

    let mut addresses = if let Some(id) = key_id {
        repo.get_addresses_by_key(secret.account_id, id)?
    } else {
        repo.get_all_addresses(secret.account_id)?
    };
    if !orchard_enabled_for_balance {
        addresses.retain(|addr| addr.address_type != AddressType::Ironwood);
    }
    addresses.retain(|addr| {
        address_matches_expected_network_prefix(&addr.address, addr.address_type, network_type)
    });
    if key_id.is_none() {
        addresses
            .retain(|addr| addr.address_scope != pirate_storage_sqlite::AddressScope::Internal);
    }

    let mut balances: HashMap<i64, (u64, u64, u64)> = HashMap::new();

    for note in notes {
        let Some(address_id) = note.address_id else {
            continue;
        };
        if note.value <= 0 {
            continue;
        }
        let value = match u64::try_from(note.value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let entry = balances.entry(address_id).or_insert((0, 0, 0));
        entry.0 = entry
            .0
            .checked_add(value)
            .ok_or_else(|| anyhow!("Balance overflow"))?;

        let note_height = note.height as u64;
        if note_height > 0 && note_height <= confirmation_threshold {
            entry.1 = entry
                .1
                .checked_add(value)
                .ok_or_else(|| anyhow!("Balance overflow"))?;
        } else {
            entry.2 = entry
                .2
                .checked_add(value)
                .ok_or_else(|| anyhow!("Balance overflow"))?;
        }
    }

    Ok(addresses
        .into_iter()
        .filter_map(|addr| {
            let id = addr.id?;
            let (total, spendable, pending) = balances.get(&id).copied().unwrap_or((0, 0, 0));
            Some(AddressBalanceInfo {
                address: addr.address,
                balance: total,
                spendable,
                pending,
                key_id: addr.key_id,
                address_id: id,
                label: addr.label,
                created_at: addr.created_at,
                color_tag: address_book_color_to_ffi(addr.color_tag),
                diversifier_index: addr.diversifier_index,
            })
        })
        .collect())
}

fn derive_receive_address(
    wallet_id: &WalletId,
    secret: &pirate_storage_sqlite::WalletSecret,
    extsk: Option<&ExtendedSpendingKey>,
    diversifier_index: [u8; 11],
    use_ironwood: bool,
) -> Result<(String, AddressType, [u8; 11])> {
    if use_ironwood {
        let orchard_extsk_bytes = secret.orchard_extsk.clone().ok_or_else(|| {
            anyhow!("Ironwood key not found - wallet needs to be recreated with Ironwood support")
        })?;
        let orchard_extsk = IronwoodExtendedSpendingKey::from_bytes(&orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Ironwood spending key bytes: {}", e))?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let orchard_addr = orchard_fvk.address_at_index(diversifier_index);
        let network_type = address_prefix_network_type(wallet_id)?;
        let addr_string = orchard_addr.encode_for_network(network_type)?;
        Ok((addr_string, AddressType::Ironwood, diversifier_index))
    } else {
        let extsk = extsk.ok_or_else(|| anyhow!("Invalid spending key bytes"))?;
        let fvk = extsk.to_extended_fvk();
        let (actual_index, payment_addr) = fvk
            .find_address_from_index(diversifier_index)
            .ok_or_else(|| anyhow!("Sapling diversifier index space is exhausted"))?;
        let addr_string = payment_addr.encode_for_network(address_prefix_network_type(wallet_id)?);
        Ok((addr_string, AddressType::Sapling, actual_index))
    }
}

pub(super) fn note_address_string(
    note_type: pirate_storage_sqlite::models::NoteType,
    note_bytes: &[u8],
    network_type: NetworkType,
    orchard_enabled_for_balance: bool,
) -> Option<String> {
    match note_type {
        pirate_storage_sqlite::models::NoteType::Sapling => {
            decode_sapling_address_bytes_from_note_bytes(note_bytes)
                .and_then(|bytes| SaplingPaymentAddress::from_bytes(&bytes))
                .map(|addr| PaymentAddress { inner: addr }.encode_for_network(network_type))
        }
        pirate_storage_sqlite::models::NoteType::Ironwood => {
            if !orchard_enabled_for_balance {
                None
            } else {
                decode_orchard_address_bytes_from_note_bytes(note_bytes)
                    .and_then(|bytes| Option::from(IronwoodAddress::from_raw_address_bytes(&bytes)))
                    .and_then(|addr| {
                        IronwoodPaymentAddress { inner: addr }
                            .encode_for_network(network_type)
                            .ok()
                    })
            }
        }
    }
}

const SAPLING_NOTE_BYTES_VERSION: u8 = 1;
const ORCHARD_NOTE_BYTES_VERSION: u8 = 1;

fn decode_sapling_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == SAPLING_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
}

fn decode_orchard_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == ORCHARD_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn recovers_sapling_address_ownership_beyond_u32_directly() {
        let fvk = ExtendedSpendingKey::from_mnemonic(MNEMONIC)
            .unwrap()
            .to_extended_fvk();
        let start = [7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        let (actual, address) = fvk.find_address_from_index(start).unwrap();
        let keys = AddressViewingKeys {
            key_id: 7,
            sapling: Some(fvk),
            ironwood: None,
        };

        let recovered = recover_address_index(
            &address.encode_for_network(NetworkType::Mainnet),
            AddressType::Sapling,
            NetworkType::Mainnet,
            &keys,
        );

        assert_eq!(
            recovered,
            Some((actual, pirate_storage_sqlite::AddressScope::External))
        );
    }

    #[test]
    fn recovers_ironwood_address_ownership_across_the_88_bit_space() {
        let fvk = IronwoodExtendedSpendingKey::master(&[11u8; 32])
            .unwrap()
            .to_extended_fvk();
        let index = [9, 8, 7, 6, 5, 4, 3, 2, 1, 1, 1];
        let address = fvk
            .address_at_index(index)
            .encode_for_network(NetworkType::Mainnet)
            .unwrap();
        let keys = AddressViewingKeys {
            key_id: 8,
            sapling: None,
            ironwood: Some(fvk),
        };

        assert_eq!(
            recover_address_index(&address, AddressType::Ironwood, NetworkType::Mainnet, &keys,),
            Some((index, pirate_storage_sqlite::AddressScope::External))
        );
    }
}
