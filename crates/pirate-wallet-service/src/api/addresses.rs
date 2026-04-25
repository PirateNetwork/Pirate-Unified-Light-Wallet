use super::{
    address_book_color_to_ffi, address_matches_expected_network_prefix,
    address_prefix_network_type, ensure_primary_account_key, is_decoy_mode_active,
    open_wallet_db_for, should_generate_orchard,
};
use crate::models::{AddressBalanceInfo, AddressInfo, WalletId};
use anyhow::{anyhow, Result};
use orchard::Address as OrchardAddress;
use pirate_core::keys::{
    ExtendedFullViewingKey, ExtendedSpendingKey, OrchardExtendedFullViewingKey,
    OrchardExtendedSpendingKey, OrchardPaymentAddress, PaymentAddress,
};
use pirate_params::NetworkType;
use pirate_storage_sqlite::{AddressType, Repository};
use std::collections::{HashMap, HashSet};
use zcash_primitives::sapling::PaymentAddress as SaplingPaymentAddress;

pub(super) fn current_receive_address(wallet_id: WalletId) -> Result<String> {
    if is_decoy_mode_active() {
        return Ok(String::new());
    }
    tracing::info!("Getting current receive address for wallet {}", wallet_id);

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let current_index = repo.get_current_diversifier_index(secret.account_id, key_id)?;

    if let Some(addr_record) = repo.get_address_by_index_for_scope(
        secret.account_id,
        key_id,
        current_index,
        pirate_storage_sqlite::AddressScope::External,
    )? {
        tracing::debug!(
            "Found existing address at index {}: {}",
            current_index,
            addr_record.address
        );
        return Ok(addr_record.address);
    }

    let use_orchard = should_generate_orchard(&wallet_id)?;
    let (addr_string, address_type) = derive_receive_address(
        &wallet_id,
        &secret,
        Some(&extsk),
        current_index,
        use_orchard,
    )?;

    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id: secret.account_id,
        diversifier_index: current_index,
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
        if use_orchard { "Orchard" } else { "Sapling" },
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

    let use_orchard = should_generate_orchard(&wallet_id)?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let next_index = repo.get_next_diversifier_index(secret.account_id, key_id)?;
    let extsk = if use_orchard {
        None
    } else {
        Some(
            ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?,
        )
    };
    let (addr_string, address_type) =
        derive_receive_address(&wallet_id, &secret, extsk.as_ref(), next_index, use_orchard)?;

    let address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(key_id),
        account_id: secret.account_id,
        diversifier_index: next_index,
        address: addr_string.clone(),
        address_type,
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::External,
    };
    repo.upsert_address(&address)?;

    tracing::info!(
        "Generated and stored next {} address at index {}: {}",
        if use_orchard { "Orchard" } else { "Sapling" },
        next_index,
        addr_string
    );
    Ok(addr_string)
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
    let primary_key_id = ensure_primary_account_key(&repo, &wallet_id, &secret)?;
    let network_type = address_prefix_network_type(&wallet_id)?;
    let orchard_active = should_generate_orchard(&wallet_id)?;
    let selected_key_id = key_id;
    let scan_key_id = selected_key_id.unwrap_or(primary_key_id);
    let key_material = if let Some(id) = key_id {
        let key = repo
            .get_account_key_by_id(id)?
            .ok_or_else(|| anyhow!("Key group not found for {}", id))?;
        if key.account_id != secret.account_id {
            return Err(anyhow!(
                "Key group {} does not belong to wallet {}",
                id,
                wallet_id
            ));
        }
        Some(key)
    } else {
        None
    };

    let mut notes = repo.get_unspent_notes(secret.account_id)?;
    let has_orchard_note_bytes = notes.iter().any(|note| {
        note.note_type == pirate_storage_sqlite::models::NoteType::Orchard
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

    let mut balances_by_address: HashMap<String, (u64, u64, u64)> = HashMap::new();

    for note in notes.iter() {
        if note.value <= 0 {
            continue;
        }
        if let Some(selected) = selected_key_id {
            let note_key_id = note.key_id.unwrap_or(primary_key_id);
            if note_key_id != selected {
                continue;
            }
        }
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
        let value = match u64::try_from(note.value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let entry = balances_by_address
            .entry(address_string)
            .or_insert((0, 0, 0));
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

    let (sapling_fvk, orchard_fvk) = if let Some(key) = key_material.as_ref() {
        let sapling_fvk = if let Some(bytes) = key.sapling_extsk.as_ref() {
            Some(ExtendedSpendingKey::from_bytes(bytes)?.to_extended_fvk())
        } else {
            key.sapling_dfvk
                .as_ref()
                .and_then(|bytes| ExtendedFullViewingKey::from_bytes(bytes))
        };
        let orchard_fvk = if let Some(bytes) = key.orchard_extsk.as_ref() {
            Some(OrchardExtendedSpendingKey::from_bytes(bytes)?.to_extended_fvk())
        } else {
            key.orchard_fvk
                .as_ref()
                .and_then(|bytes| OrchardExtendedFullViewingKey::from_bytes(bytes).ok())
        };
        (sapling_fvk, orchard_fvk)
    } else {
        let sapling_fvk = if !secret.extsk.is_empty() {
            Some(ExtendedSpendingKey::from_bytes(&secret.extsk)?.to_extended_fvk())
        } else {
            secret
                .dfvk
                .as_ref()
                .and_then(|bytes| ExtendedFullViewingKey::from_bytes(bytes))
        };
        let orchard_fvk = if let Some(bytes) = secret.orchard_extsk.as_ref() {
            Some(OrchardExtendedSpendingKey::from_bytes(bytes)?.to_extended_fvk())
        } else {
            secret
                .orchard_ivk
                .as_ref()
                .and_then(|bytes| OrchardExtendedFullViewingKey::from_bytes(bytes).ok())
        };
        (sapling_fvk, orchard_fvk)
    };
    let created_at = chrono::Utc::now().timestamp();
    let mut scanned_addresses: HashMap<String, u32> = HashMap::new();
    if let Some(fvk) = sapling_fvk.as_ref() {
        let mut scan_context = SequentialAddressScan {
            repo: &repo,
            account_id: secret.account_id,
            key_id: scan_key_id,
            address_type: AddressType::Sapling,
            balances_by_address: &balances_by_address,
            scanned: &mut scanned_addresses,
            created_at,
        };
        scan_sequential_addresses(&mut scan_context, |index| {
            Some(fvk.derive_address(index).encode_for_network(network_type))
        })?;
    }
    if orchard_enabled_for_balance {
        if let Some(fvk) = orchard_fvk.as_ref() {
            let mut scan_context = SequentialAddressScan {
                repo: &repo,
                account_id: secret.account_id,
                key_id: scan_key_id,
                address_type: AddressType::Orchard,
                balances_by_address: &balances_by_address,
                scanned: &mut scanned_addresses,
                created_at,
            };
            scan_sequential_addresses(&mut scan_context, |index| {
                fvk.address_at(index).encode_for_network(network_type).ok()
            })?;
        }
    }
    for note in notes.iter_mut() {
        if let Some(selected) = selected_key_id {
            let note_key_id = note.key_id.unwrap_or(primary_key_id);
            if note_key_id != selected {
                continue;
            }
        }
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
        if let Some(diversifier_index) = scanned_addresses.get(&address_string).copied() {
            let address_type = match note.note_type {
                pirate_storage_sqlite::models::NoteType::Sapling => AddressType::Sapling,
                pirate_storage_sqlite::models::NoteType::Orchard => AddressType::Orchard,
            };
            let address_record = pirate_storage_sqlite::Address {
                id: None,
                key_id: note.key_id.or(Some(scan_key_id)),
                account_id: secret.account_id,
                diversifier_index,
                address: address_string.clone(),
                address_type,
                label: None,
                created_at,
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: pirate_storage_sqlite::AddressScope::External,
            };
            let _ = repo.upsert_address(&address_record);
        }
        if let Some(addr) = repo
            .get_address_by_string(secret.account_id, &address_string)?
            .and_then(|addr| addr.id)
        {
            if note.address_id != Some(addr) {
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
        addresses.retain(|addr| addr.address_type != AddressType::Orchard);
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
    diversifier_index: u32,
    use_orchard: bool,
) -> Result<(String, AddressType)> {
    if use_orchard {
        let orchard_extsk_bytes = secret.orchard_extsk.clone().ok_or_else(|| {
            anyhow!("Orchard key not found - wallet needs to be recreated with Orchard support")
        })?;
        let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(&orchard_extsk_bytes)
            .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let orchard_addr = orchard_fvk.address_at(diversifier_index);
        let network_type = address_prefix_network_type(wallet_id)?;
        let addr_string = orchard_addr.encode_for_network(network_type)?;
        Ok((addr_string, AddressType::Orchard))
    } else {
        let extsk = extsk.ok_or_else(|| anyhow!("Invalid spending key bytes"))?;
        let fvk = extsk.to_extended_fvk();
        let payment_addr = fvk.derive_address(diversifier_index);
        let addr_string = payment_addr.encode();
        Ok((addr_string, AddressType::Sapling))
    }
}

fn note_address_string(
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
        pirate_storage_sqlite::models::NoteType::Orchard => {
            if !orchard_enabled_for_balance {
                None
            } else {
                decode_orchard_address_bytes_from_note_bytes(note_bytes)
                    .and_then(|bytes| Option::from(OrchardAddress::from_raw_address_bytes(&bytes)))
                    .and_then(|addr| {
                        OrchardPaymentAddress { inner: addr }
                            .encode_for_network(network_type)
                            .ok()
                    })
            }
        }
    }
}

struct SequentialAddressScan<'a> {
    repo: &'a Repository<'a>,
    account_id: i64,
    key_id: i64,
    address_type: AddressType,
    balances_by_address: &'a HashMap<String, (u64, u64, u64)>,
    scanned: &'a mut HashMap<String, u32>,
    created_at: i64,
}

const ADDRESS_SCAN_MAX_GAP_AFTER_MATCH: u32 = 32;
const ADDRESS_SCAN_HARD_LIMIT: u32 = 4096;
const SAPLING_NOTE_BYTES_VERSION: u8 = 1;
const ORCHARD_NOTE_BYTES_VERSION: u8 = 1;

fn address_matches_type(address: &str, address_type: AddressType) -> bool {
    match address_type {
        AddressType::Sapling => {
            address.starts_with("zs1")
                || address.starts_with("ztestsapling1")
                || address.starts_with("zregtestsapling1")
        }
        AddressType::Orchard => {
            address.starts_with("pirate1")
                || address.starts_with("pirate-test1")
                || address.starts_with("pirate-regtest1")
        }
    }
}

fn scan_sequential_addresses<F>(
    context: &mut SequentialAddressScan<'_>,
    mut derive_address: F,
) -> Result<()>
where
    F: FnMut(u32) -> Option<String>,
{
    let expected_matches = context
        .balances_by_address
        .keys()
        .filter(|address| address_matches_type(address, context.address_type))
        .count();
    let mut matched_addresses = HashSet::new();
    let mut gap_after_match = 0u32;
    let mut index = 0u32;
    loop {
        if index > ADDRESS_SCAN_HARD_LIMIT {
            break;
        }

        let Some(address) = derive_address(index) else {
            break;
        };
        let has_balance = context
            .balances_by_address
            .get(&address)
            .map(|(total, _, _)| *total > 0)
            .unwrap_or(false);

        if index == 0 || has_balance {
            let address_record = pirate_storage_sqlite::Address {
                id: None,
                key_id: Some(context.key_id),
                account_id: context.account_id,
                diversifier_index: index,
                address: address.clone(),
                address_type: context.address_type,
                label: None,
                created_at: context.created_at,
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: pirate_storage_sqlite::AddressScope::External,
            };
            let _ = context.repo.upsert_address(&address_record);
        }

        if has_balance {
            context.scanned.entry(address.clone()).or_insert(index);
            matched_addresses.insert(address);
            gap_after_match = 0;
        } else if !matched_addresses.is_empty() {
            gap_after_match = gap_after_match.saturating_add(1);
            if gap_after_match >= ADDRESS_SCAN_MAX_GAP_AFTER_MATCH {
                break;
            }
        }

        if expected_matches == 0 && index == 0 {
            break;
        }
        if expected_matches > 0 && matched_addresses.len() >= expected_matches {
            break;
        }

        index = index
            .checked_add(1)
            .ok_or_else(|| anyhow!("Diversifier index overflow"))?;
    }

    Ok(())
}

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
