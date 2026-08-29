use super::Repository;
use crate::address_book::ColorTag;
use crate::models::{Address, AddressDisplayPreference, AddressScope, AddressType};
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension, Row};

const ADDRESS_SELECT_COLUMNS: &str =
    "id, account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope, diversifier_index_be";

fn encode_diversifier_index_be(index: [u8; 11]) -> [u8; 11] {
    let mut encoded = index;
    encoded.reverse();
    encoded
}

fn decode_diversifier_index_be(mut encoded: [u8; 11]) -> [u8; 11] {
    encoded.reverse();
    encoded
}

fn increment_diversifier_index(index: &mut [u8; 11]) -> Result<()> {
    for byte in index {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            return Ok(());
        }
    }
    Err(Error::Validation(
        "ZIP-32 diversifier index space is exhausted".to_string(),
    ))
}

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

pub(super) fn get_current_diversifier_index_for_scope_and_type(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    scope: AddressScope,
    address_type: AddressType,
) -> Result<u32> {
    let max_index: Option<i64> = repo.db.conn().query_row(
        "SELECT MAX(diversifier_index) FROM addresses
         WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3 AND address_type = ?4",
        params![
            account_id,
            key_id,
            address_scope_str(scope),
            address_type_str(address_type)
        ],
        |row| row.get(0),
    )?;

    Ok(max_index.map_or(0, |value| value as u32))
}

pub(super) fn get_next_diversifier_index_for_scope_and_type(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    scope: AddressScope,
    address_type: AddressType,
) -> Result<u32> {
    let max_index: Option<i64> = repo.db.conn().query_row(
        "SELECT MAX(diversifier_index) FROM addresses
         WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3 AND address_type = ?4",
        params![
            account_id,
            key_id,
            address_scope_str(scope),
            address_type_str(address_type)
        ],
        |row| row.get(0),
    )?;

    let Some(max_index) = max_index else {
        return Ok(0);
    };
    let max_index = u32::try_from(max_index)
        .map_err(|_| Error::Validation("Stored address sequence is invalid".to_string()))?;
    if max_index < u32::MAX {
        return Ok(max_index + 1);
    }

    // A legacy caller may have supplied u32::MAX as display metadata. Keep
    // ordinary allocation on the constant-time MAX + 1 path and search for a
    // free display sequence only for this exceptional poisoned-maximum case.
    let mut statement = repo.db.conn().prepare(
        "SELECT DISTINCT diversifier_index FROM addresses
         WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3 AND address_type = ?4
         ORDER BY diversifier_index",
    )?;
    let rows = statement.query_map(
        params![
            account_id,
            key_id,
            address_scope_str(scope),
            address_type_str(address_type)
        ],
        |row| row.get::<_, i64>(0),
    )?;

    let mut lowest_free = 0_u32;
    for row in rows {
        let stored = u32::try_from(row?)
            .map_err(|_| Error::Validation("Stored address sequence is invalid".to_string()))?;
        if stored < lowest_free {
            continue;
        }
        if stored > lowest_free {
            return Ok(lowest_free);
        }
        lowest_free = lowest_free
            .checked_add(1)
            .ok_or_else(|| Error::Validation("Address sequence is exhausted".to_string()))?;
    }
    Ok(lowest_free)
}

fn get_next_diversifier_index_88_for_scope_and_type(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    scope: AddressScope,
    address_type: AddressType,
) -> Result<[u8; 11]> {
    let encoded: Option<Vec<u8>> = repo
        .db
        .conn()
        .query_row(
            "SELECT diversifier_index_be FROM addresses
             WHERE account_id = ?1 AND key_id = ?2 AND address_scope = ?3
               AND address_type = ?4 AND diversifier_index_be IS NOT NULL
             ORDER BY diversifier_index_be DESC LIMIT 1",
            params![
                account_id,
                key_id,
                address_scope_str(scope),
                address_type_str(address_type)
            ],
            |row| row.get(0),
        )
        .optional()?;

    let Some(encoded) = encoded else {
        return Ok([0; 11]);
    };
    let encoded: [u8; 11] = encoded.try_into().map_err(|_| {
        Error::Validation("Stored ZIP-32 diversifier index has an invalid length".to_string())
    })?;
    let mut index = decode_diversifier_index_be(encoded);
    increment_diversifier_index(&mut index)?;
    Ok(index)
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

/// Atomically allocates the next diversifier index for an account, key, scope,
/// and shielded pool, then persists whatever `build` derives for that index.
///
/// `get_next_diversifier_index` + a separate `upsert_address` call is a
/// read-then-write with no lock held across the two: two threads calling it
/// concurrently (e.g. two HTTP requests generating an address at the same
/// moment) can both read the same current max index before either writes,
/// derive the *same* next address, and hand it out to two different
/// callers. `BEGIN IMMEDIATE` takes SQLite's write lock before the read, so
/// a second, concurrent caller - even on its own connection, since
/// `pirate-wallet-service` caches one connection per thread - blocks until
/// the first commits and then correctly observes its newly-inserted row.
///
/// `build` must be a pure function of the sequence number and the first raw
/// 88-bit index it may use (it runs with the write
/// lock held, so it must not perform its own database I/O).
pub(super) fn allocate_next_diversified_address<F>(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    scope: AddressScope,
    address_type: AddressType,
    build: F,
) -> Result<Address>
where
    F: FnOnce(u32, [u8; 11]) -> Result<Address>,
{
    let conn = repo.db.conn();
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = (|| {
        let next_index = get_next_diversifier_index_for_scope_and_type(
            repo,
            account_id,
            key_id,
            scope,
            address_type,
        )?;
        let next_index_88 = get_next_diversifier_index_88_for_scope_and_type(
            repo,
            account_id,
            key_id,
            scope,
            address_type,
        )?;
        let address = build(next_index, next_index_88)?;
        let actual_index = address.diversifier_index_88.ok_or_else(|| {
            Error::Validation("Address derivation did not return a ZIP-32 index".to_string())
        })?;
        if encode_diversifier_index_be(actual_index) < encode_diversifier_index_be(next_index_88) {
            return Err(Error::Validation(
                "Address derivation moved the ZIP-32 index backwards".to_string(),
            ));
        }
        upsert_address(repo, &address)?;
        Ok(address)
    })();

    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    } else {
        conn.execute_batch("COMMIT")?;
    }

    result
}

pub(super) fn upsert_address(repo: &Repository<'_>, address: &Address) -> Result<()> {
    repo.db.conn().execute(
        "INSERT INTO addresses (account_id, key_id, diversifier_index, address, address_type, label, created_at, color_tag, address_scope, diversifier_index_be)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
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
             END,
             diversifier_index_be = COALESCE(
                 excluded.diversifier_index_be,
                 addresses.diversifier_index_be
             )",
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
            address
                .diversifier_index_88
                .map(encode_diversifier_index_be)
                .map(|value| value.to_vec()),
        ],
    )?;
    Ok(())
}

pub(super) fn repair_address_ownership(repo: &Repository<'_>, address: &Address) -> Result<()> {
    let key_id = address.key_id.ok_or_else(|| {
        Error::Validation("Address ownership repair requires a key group".to_string())
    })?;
    let index = address.diversifier_index_88.ok_or_else(|| {
        Error::Validation("Address ownership repair requires a ZIP-32 index".to_string())
    })?;
    let changed = repo.db.conn().execute(
        "UPDATE addresses SET key_id = ?1, address_type = ?2, address_scope = ?3, diversifier_index_be = ?4
         WHERE account_id = ?5 AND address = ?6",
        params![
            key_id,
            address_type_str(address.address_type),
            address_scope_str(address.address_scope),
            encode_diversifier_index_be(index).to_vec(),
            address.account_id,
            &address.address,
        ],
    )?;
    if changed != 1 {
        return Err(Error::Storage(
            "Address ownership repair target is unavailable".to_string(),
        ));
    }
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

pub(super) fn get_address_by_index_for_scope_and_type(
    repo: &Repository<'_>,
    account_id: i64,
    key_id: i64,
    diversifier_index: u32,
    scope: AddressScope,
    address_type: AddressType,
) -> Result<Option<Address>> {
    let sql = format!(
        "SELECT {ADDRESS_SELECT_COLUMNS} FROM addresses
         WHERE account_id = ?1 AND key_id = ?2 AND diversifier_index = ?3
           AND address_scope = ?4 AND address_type = ?5"
    );
    let mut stmt = repo.db.conn().prepare(&sql)?;
    let result = stmt
        .query_row(
            params![
                account_id,
                key_id,
                diversifier_index as i64,
                address_scope_str(scope),
                address_type_str(address_type)
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

pub(super) fn get_address_display_preferences(
    repo: &Repository<'_>,
    account_id: i64,
) -> Result<Vec<AddressDisplayPreference>> {
    let mut stmt = repo.db.conn().prepare(
        "SELECT preferences.address_id, preferences.is_pinned, preferences.is_archived
         FROM address_display_preferences preferences
         INNER JOIN addresses ON addresses.id = preferences.address_id
         WHERE addresses.account_id = ?1",
    )?;
    let preferences = stmt
        .query_map([account_id], |row| {
            Ok(AddressDisplayPreference {
                address_id: row.get(0)?,
                is_pinned: row.get::<_, i64>(1)? != 0,
                is_archived: row.get::<_, i64>(2)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(preferences)
}

pub(super) fn set_address_pinned(
    repo: &Repository<'_>,
    account_id: i64,
    address_id: i64,
    is_pinned: bool,
) -> Result<()> {
    let rows = repo.db.conn().execute(
        "INSERT INTO address_display_preferences (
             address_id, is_pinned, is_archived, updated_at
         )
         SELECT id, ?3, 0, datetime('now')
         FROM addresses
         WHERE account_id = ?1 AND id = ?2
         ON CONFLICT(address_id) DO UPDATE SET
             is_pinned = excluded.is_pinned,
             is_archived = CASE
                 WHEN excluded.is_pinned = 1 THEN 0
                 ELSE address_display_preferences.is_archived
             END,
             updated_at = excluded.updated_at",
        params![account_id, address_id, is_pinned as i64],
    )?;
    if rows == 0 {
        return Err(Error::NotFound(format!(
            "address row {address_id} for account {account_id}"
        )));
    }
    Ok(())
}

pub(super) fn set_address_archived(
    repo: &Repository<'_>,
    account_id: i64,
    address_id: i64,
    is_archived: bool,
) -> Result<()> {
    let rows = repo.db.conn().execute(
        "INSERT INTO address_display_preferences (
             address_id, is_pinned, is_archived, updated_at
         )
         SELECT id, 0, ?3, datetime('now')
         FROM addresses
         WHERE account_id = ?1 AND id = ?2
         ON CONFLICT(address_id) DO UPDATE SET
             is_archived = excluded.is_archived,
             is_pinned = CASE
                 WHEN excluded.is_archived = 1 THEN 0
                 ELSE address_display_preferences.is_pinned
             END,
             updated_at = excluded.updated_at",
        params![account_id, address_id, is_archived as i64],
    )?;
    if rows == 0 {
        return Err(Error::NotFound(format!(
            "address row {address_id} for account {account_id}"
        )));
    }
    Ok(())
}

fn decode_address_row(row: &Row<'_>) -> rusqlite::Result<Address> {
    let encoded_index: Option<Vec<u8>> = row.get(10)?;
    let diversifier_index_88 = encoded_index
        .map(|encoded| {
            let encoded: [u8; 11] = encoded.try_into().map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Blob,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "ZIP-32 diversifier index must contain 11 bytes",
                    )
                    .into(),
                )
            })?;
            Ok::<[u8; 11], rusqlite::Error>(decode_diversifier_index_be(encoded))
        })
        .transpose()?;
    Ok(Address {
        id: Some(row.get(0)?),
        account_id: row.get(1)?,
        key_id: row.get(2)?,
        diversifier_index: row.get::<_, i64>(3)? as u32,
        diversifier_index_88,
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
        "Orchard" => AddressType::Ironwood,
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
        AddressType::Ironwood => "Orchard",
    }
}

fn address_scope_str(scope: AddressScope) -> &'static str {
    match scope {
        AddressScope::External => "external",
        AddressScope::Internal => "internal",
    }
}
