use super::Repository;
use crate::address_book::ColorTag;
use crate::models::{Address, AddressScope, AddressType};
use crate::Result;
use rusqlite::{params, OptionalExtension, Row};

const ADDRESS_SELECT_COLUMNS: &str =
    "id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope";

pub(super) fn get_current_diversifier_index_for_scope(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    scope: AddressScope,
) -> Result<u32> {
    let max_index: Option<i64> = repo.db.conn().query_row(
        "SELECT MAX(diversifier_index) FROM addresses WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3",
        params![account_id, key_id, address_scope_str(scope)],
        |row| row.get(0),
    )?;

    Ok(max_index.map_or(0, |value| value as u32))
}

pub(super) fn backfill_address_key_id(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
) -> Result<usize> {
    let rows = repo.db.conn().execute(
        "UPDATE addresses SET key_id = ?1 WHERE account_id = ?2 AND key_id IS NULL",
        params![key_id, account_id],
    )?;
    Ok(rows)
}

pub(super) fn upsert_address(repo: &Repository<'_>, address: &Address) -> Result<()> {
    repo.db.conn().execute(
        "INSERT INTO addresses (account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(address) DO UPDATE SET
             account_id = excluded.account_id,
             key_id = COALESCE(excluded.key_id, addresses.key_id),
             diversifier_index = CASE
                 WHEN excluded.diversifier_index = 0 THEN addresses.diversifier_index
                 ELSE excluded.diversifier_index
             END,
             address_type = excluded.address_type,
             label = COALESCE(excluded.label, addresses.label),
             created_at = addresses.created_at,
             color_tag = addresses.color_tag,
             address_scope = CASE
                 WHEN addresses.address_scope = 'internal' THEN addresses.address_scope
                 ELSE excluded.address_scope
             END",
        params![
            address.account_id,
            address.key_id,
            address.diversifier_index as i64,
            address.address,
            address_type_str(address.address_type),
            address.label,
            address.created_at,
            address.color_tag.as_u8() as i64,
            address_scope_str(address.address_scope),
        ],
    )?;
    Ok(())
}

pub(super) fn get_address_by_string(
    repo: &Repository<'_>,
    account_id: i64,
    address: &str,
) -> Result<Option<Address>> {
    let sql = format!(
        "SELECT {ADDRESS_SELECT_COLUMNS} FROM addresses WHERE account_id = ?1 AND address = ?2"
    );
    let mut stmt = repo.db.conn().prepare(&sql)?;
    let result = stmt
        .query_row(params![account_id, address], decode_address_row)
        .optional()?;
    Ok(result)
}

pub(super) fn get_address_by_index_for_scope(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    diversifier_index: u32,
    scope: AddressScope,
) -> Result<Option<Address>> {
    let sql = format!(
        "SELECT {ADDRESS_SELECT_COLUMNS} FROM addresses
         WHERE account_id = ?1 AND key_id = ?2 AND diversifier_index = ?3 AND address_scope = ?4"
    );
    let mut stmt = repo.db.conn().prepare(&sql)?;
    let result = stmt
        .query_row(
            params![
                account_id,
                key_id,
                diversifier_index as i64,
                address_scope_str(scope)
            ],
            decode_address_row,
        )
        .optional()?;
    Ok(result)
}

pub(super) fn get_all_addresses(repo: &Repository<'_>, account_id: i64) -> Result<Vec<Address>> {
    let sql = format!(
        "SELECT {ADDRESS_SELECT_COLUMNS} FROM addresses
         WHERE account_id = ?1
         ORDER BY diversifier_index ASC"
    );
    let mut stmt = repo.db.conn().prepare(&sql)?;
    let addresses = stmt
        .query_map([account_id], decode_address_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(addresses)
}

pub(super) fn get_addresses_by_key(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
) -> Result<Vec<Address>> {
    let sql = format!(
        "SELECT {ADDRESS_SELECT_COLUMNS} FROM addresses
         WHERE account_id = ?1 AND key_id = ?2
         ORDER BY diversifier_index ASC"
    );
    let mut stmt = repo.db.conn().prepare(&sql)?;
    let addresses = stmt
        .query_map([account_id, key_id], decode_address_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(addresses)
}

pub(super) fn update_address_label(
    repo: &Repository<'_>,
    account_id: i64,
    address: &str,
    label: Option<String>,
) -> Result<()> {
    repo.db.conn().execute(
        "UPDATE addresses SET label = ?1 WHERE account_id = ?2 AND address = ?3",
        params![label, account_id, address],
    )?;
    Ok(())
}

pub(super) fn update_address_color_tag(
    repo: &Repository<'_>,
    account_id: i64,
    address: &str,
    color_tag: ColorTag,
) -> Result<()> {
    repo.db.conn().execute(
        "UPDATE addresses SET color_tag = ?1 WHERE account_id = ?2 AND address = ?3",
        params![color_tag.as_u8() as i64, account_id, address],
    )?;
    Ok(())
}

fn decode_address_row(row: &Row<'_>) -> rusqlite::Result<Address> {
    Ok(Address {
        id: Some(row.get(0)?),
        account_id: row.get(1)?,
        key_id: row.get(2)?,
        diversifier_index: row.get::<_, i64>(3)? as u32,
        address: row.get(4)?,
        address_type: decode_address_type(row)?,
        label: row.get(6)?,
        created_at: row.get(7)?,
        color_tag: ColorTag::from_u8(row.get::<_, i64>(8)? as u8),
        address_scope: decode_address_scope(row)?,
    })
}

fn decode_address_type(row: &Row<'_>) -> rusqlite::Result<AddressType> {
    let value: String = row.get(5).unwrap_or_else(|_| "Sapling".to_string());
    Ok(match value.as_str() {
        "Orchard" => AddressType::Orchard,
        _ => AddressType::Sapling,
    })
}

fn decode_address_scope(row: &Row<'_>) -> rusqlite::Result<AddressScope> {
    let value: String = row.get(9).unwrap_or_else(|_| "external".to_string());
    Ok(match value.as_str() {
        "internal" => AddressScope::Internal,
        _ => AddressScope::External,
    })
}

fn address_type_str(address_type: AddressType) -> &'static str {
    match address_type {
        AddressType::Sapling => "Sapling",
        AddressType::Orchard => "Orchard",
    }
}

fn address_scope_str(scope: AddressScope) -> &'static str {
    match scope {
        AddressScope::External => "external",
        AddressScope::Internal => "internal",
    }
}
