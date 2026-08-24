# Key import and sync telemetry

The wallet emits privacy-safe, structured `debug.log` events for diagnosing key imports and shielded-note discovery. These events answer two different questions:

1. Was an imported spending key successfully stored?
2. Did the sync scanner actually prepare trial-decryption keys for it?

The events never contain seed phrases, spending or viewing key bytes, addresses, labels, key fingerprints, account IDs, or key IDs. The wallet ID passes through the central debug-log redactor. Aggregate key counts and birthday heights are intentionally retained because they are required to diagnose incomplete wallet recovery.

## Event schema

All events below use `data.schema_version = 1`.

### `spending key import requested`

Emitted before import validation begins. Its fields are:

- `birthday_height`
- `sapling_requested`
- `ironwood_requested`

This event proves only that the application attempted an import. It does not prove that the supplied key was valid or persisted.

### `spending key import persisted`

Emitted only after the encrypted account-key record is successfully written. Its fields are:

- `birthday_height`
- `sapling_stored`
- `ironwood_stored`
- `account_key_count`
- `imported_spending_count`
- `sapling_imported_spending_count`
- `ironwood_imported_spending_count`

The aggregate counts are a best-effort post-write inventory. They are `null` if that diagnostic read fails; this does not turn an otherwise successful import into an application error. A request event without a corresponding persisted event means the import was rejected, failed, or was interrupted.

### `sync key inventory`

Emitted when a wallet is attached to the sync engine, after stored account keys have been converted into usable key groups and external/internal incoming viewing keys (IVKs). Its fields are grouped by diagnostic layer:

- Stored metadata: `account_key_count`, `seed_count`, `imported_spending_count`, `imported_viewing_count`, and `spendable_count`.
- Pool metadata: `sapling_imported_spending_count`, `ironwood_imported_spending_count`, `sapling_imported_viewing_count`, `ironwood_imported_viewing_count`, `sapling_min_birthday_height`, and `ironwood_min_birthday_height`.
- Usable scanner groups: `key_group_count`, `sapling_key_group_count`, `ironwood_key_group_count`, `sapling_imported_spending_key_group_count`, and `ironwood_imported_spending_key_group_count`.
- Prepared scanner keys: `sapling_ivk_count`, `ironwood_ivk_count`, `sapling_imported_spending_ivk_count`, `ironwood_imported_spending_ivk_count`, and the external/internal IVK counts for each pool.

The layers are deliberately separate. For example, a nonzero `sapling_imported_spending_count` proves that an imported Sapling spending-key record was loaded from storage. Nonzero `sapling_imported_spending_key_group_count` and `sapling_imported_spending_ivk_count` then prove that the imported record itself reached trial decryption. A difference between stored-record and usable-group counts points to incomplete or unsupported key data rather than a display-only balance problem.

Each full-viewing-key group normally produces one external and one internal IVK for its pool. Support tooling should compare counts rather than assume that relationship forever, because future key scopes may intentionally prepare a different set.

## Interpreting an empty wallet

If a completed rescan shows the expected imported-key record, usable pool group, and prepared IVKs but finds no notes or transactions, the wallet did scan with that key. That result strongly points to the wrong key, the wrong chain/network, or funds that were never controlled by that key; it is not evidence of a UI-only balance omission.

An empty result does not prove that a seed or key was never used anywhere. It proves only that the selected network and scanned height range contain no history decryptable by the keys loaded for that run.
