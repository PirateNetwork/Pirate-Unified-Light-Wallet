use super::{address_book_color_from_ffi, address_book_color_to_ffi};
use crate::models::{AddressBookColorTag, AddressBookEntryFfi, WalletId};
use anyhow::{anyhow, Result};
use pirate_storage_sqlite::address_book::{
    AddressBookEntry as DbAddressBookEntry, AddressBookStorage,
};

fn parse_rfc3339_timestamp(value: &str) -> Result<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|e| anyhow!("Invalid timestamp: {}", e))?;
    Ok(parsed.timestamp())
}

fn address_book_entry_to_ffi(entry: DbAddressBookEntry) -> Result<AddressBookEntryFfi> {
    Ok(AddressBookEntryFfi {
        id: entry.id,
        wallet_id: entry.wallet_id,
        address: entry.address,
        label: entry.label,
        notes: entry.notes,
        color_tag: address_book_color_to_ffi(entry.color_tag),
        is_favorite: entry.is_favorite,
        created_at: parse_rfc3339_timestamp(&entry.created_at)?,
        updated_at: parse_rfc3339_timestamp(&entry.updated_at)?,
        last_used_at: match entry.last_used_at {
            Some(value) => Some(parse_rfc3339_timestamp(&value)?),
            None => None,
        },
        use_count: entry.use_count,
    })
}

pub(super) fn list_address_book(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;

    let mut entries = AddressBookStorage::list(db.conn(), &wallet_id)?;
    if wallet_id != "legacy" {
        if let Ok(mut legacy) = AddressBookStorage::list(db.conn(), "legacy") {
            entries.append(&mut legacy);
        }
    }

    entries.sort_by(|a, b| {
        if a.is_favorite != b.is_favorite {
            return if a.is_favorite {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.label.cmp(&b.label)
    });

    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

pub(super) fn add_address_book_entry(
    wallet_id: WalletId,
    address: String,
    label: String,
    notes: Option<String>,
    color_tag: AddressBookColorTag,
) -> Result<AddressBookEntryFfi> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;

    let mut entry = DbAddressBookEntry::new(wallet_id.clone(), address, label);
    if let Some(notes_value) = notes {
        if !notes_value.is_empty() {
            entry = entry.with_notes(notes_value);
        }
    }
    entry = entry.with_color_tag(address_book_color_from_ffi(color_tag));

    let id = AddressBookStorage::insert(db.conn(), &entry)?;
    let stored = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found after insert"))?;
    address_book_entry_to_ffi(stored)
}

pub(super) fn update_address_book_entry(
    wallet_id: WalletId,
    id: i64,
    label: Option<String>,
    notes: Option<String>,
    color_tag: Option<AddressBookColorTag>,
    is_favorite: Option<bool>,
) -> Result<AddressBookEntryFfi> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let mut entry = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found"))?;

    if let Some(label_value) = label {
        entry.label = label_value;
    }
    if let Some(notes_value) = notes {
        entry.notes = if notes_value.is_empty() {
            None
        } else {
            Some(notes_value)
        };
    }
    if let Some(tag) = color_tag {
        entry.color_tag = address_book_color_from_ffi(tag);
    }
    if let Some(favorite) = is_favorite {
        entry.is_favorite = favorite;
    }

    AddressBookStorage::update(db.conn(), &entry)?;
    let updated = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?
        .ok_or_else(|| anyhow!("Address book entry not found after update"))?;
    address_book_entry_to_ffi(updated)
}

pub(super) fn delete_address_book_entry(wallet_id: WalletId, id: i64) -> Result<()> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::delete(db.conn(), &wallet_id, id)?;
    Ok(())
}

pub(super) fn toggle_address_book_favorite(wallet_id: WalletId, id: i64) -> Result<bool> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::toggle_favorite(db.conn(), &wallet_id, id)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

pub(super) fn mark_address_used(wallet_id: WalletId, address: String) -> Result<()> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::mark_used(db.conn(), &wallet_id, &address)?;
    Ok(())
}

pub(super) fn get_label_for_address(
    wallet_id: WalletId,
    address: String,
) -> Result<Option<String>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::get_label_for_address(db.conn(), &wallet_id, &address)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

pub(super) fn address_exists_in_book(wallet_id: WalletId, address: String) -> Result<bool> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::exists(db.conn(), &wallet_id, &address)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

pub(super) fn get_address_book_count(wallet_id: WalletId) -> Result<u32> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    AddressBookStorage::count(db.conn(), &wallet_id)
        .map_err(|e| anyhow!("Address book error: {}", e))
}

pub(super) fn get_address_book_entry(
    wallet_id: WalletId,
    id: i64,
) -> Result<Option<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let entry = AddressBookStorage::get_by_id(db.conn(), &wallet_id, id)?;
    match entry {
        Some(value) => Ok(Some(address_book_entry_to_ffi(value)?)),
        None => Ok(None),
    }
}

pub(super) fn get_address_book_entry_by_address(
    wallet_id: WalletId,
    address: String,
) -> Result<Option<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let entry = AddressBookStorage::get_by_address(db.conn(), &wallet_id, &address)?;
    match entry {
        Some(value) => Ok(Some(address_book_entry_to_ffi(value)?)),
        None => Ok(None),
    }
}

pub(super) fn search_address_book(
    wallet_id: WalletId,
    query: String,
) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::search(db.conn(), &wallet_id, &query)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

pub(super) fn get_address_book_favorites(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::list_favorites(db.conn(), &wallet_id)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}

pub(super) fn get_recently_used_addresses(
    wallet_id: WalletId,
    limit: u32,
) -> Result<Vec<AddressBookEntryFfi>> {
    let (db, _repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let entries = AddressBookStorage::recently_used(db.conn(), &wallet_id, limit)?;
    entries
        .into_iter()
        .map(address_book_entry_to_ffi)
        .collect::<Result<Vec<_>>>()
}
