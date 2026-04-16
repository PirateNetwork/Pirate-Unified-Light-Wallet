# CLI

The repo-owned CLI lives in:

- `crates/pirate-cli-core`
- `crates/piratewallet-cli`

Build:

```bash
bash scripts/build-cli.sh
```

Direct build:

```bash
cd crates
cargo build --release -p piratewallet-cli
```

Binary path:

```bash
./crates/target/release/piratewallet-cli
```

## Global flags

- `--format pretty`
- `--format json`

`pretty` changes formatting only. It does not change the result data.

## Common output shapes

These type names describe the portable wallet-facing fields that outside
integrators should rely on. The CLI may include extra implementation-specific
fields in some responses, but those are intentionally not treated as part of
the builder-facing reference here.

- `BuildInfo`
  - `version`
  - `git_commit`
  - `build_date`
  - `rust_version`
  - `target_triple`
- `NetworkInfo`
  - `name`
  - `coin_type`
  - `rpc_port`
  - `default_birthday`
- `WalletMeta`
  - `id`
  - `name`
  - `created_at`
  - `watch_only`
  - `birthday_height`
  - `network_type`
- `AddressInfo`
  - `address`
  - `diversifier_index`
  - `created_at`
- `AddressBalanceInfo`
  - `address`
  - `balance`
  - `spendable`
  - `pending`
  - `key_id`
  - `address_id`
  - `created_at`
  - `diversifier_index`
- `Balance`
  - `total`
  - `spendable`
  - `pending`
- `SyncStatus`
  - `local_height`
  - `target_height`
  - `percent`
  - `eta`
  - `stage`
  - `last_checkpoint`
  - `blocks_per_second`
  - `notes_decrypted`
  - `last_batch_ms`
- `SpendabilityStatus`
  - `spendable`
  - `rescan_required`
  - `target_height`
  - `anchor_height`
  - `validated_anchor_height`
  - `repair_queued`
  - `reason_code`
- `FeeInfo`
  - `default_fee`
  - `min_fee`
  - `max_fee`
  - `fee_per_output`
  - `memo_fee_multiplier`
- `TxInfo`
  - `txid`
  - `height`
  - `timestamp`
  - `amount`
  - `fee`
  - `memo`
  - `confirmed`
- `NoteInfo`
  - `id`
  - `note_type`
  - `value`
  - `spent`
  - `height`
  - `txid`
  - `output_index`
  - `key_id`
  - `address_id`
  - `memo`
- `PendingTx`
  - `id`
  - `outputs`
  - `total_amount`
  - `fee`
  - `change`
  - `input_total`
  - `num_inputs`
  - `expiry_height`
  - `created_at`
- `SignedTx`
  - `txid`
  - `raw`
  - `size`
- `KeyGroupInfo`
  - `id`
  - `key_type`
  - `spendable`
  - `has_sapling`
  - `has_orchard`
  - `birthday_height`
  - `created_at`
- `KeyExportInfo`
  - `key_id`
  - `sapling_viewing_key`
  - `orchard_viewing_key`
  - `sapling_spending_key`
  - `orchard_spending_key`
- `CheckpointInfo`
  - `height`
  - `timestamp`
- `SyncLogEntryFfi`
  - `timestamp`
  - `level`
  - `module`
  - `message`
- acknowledgement result for mutating service calls:
  - `{"acknowledged": true}`

One legacy command uses custom output:

- `send <legacy-json>`
  - `{"txid":"<transaction-id>"}`

## Top-level commands

- `build-info`
  - Flags: none
  - Output: `BuildInfo`
- `network-info`
  - Flags: none
  - Output: `NetworkInfo`
- `exec-json <REQUEST_JSON>`
  - Flags: none
  - Output: JSON envelope
    - `ok`
    - `result`
    - `error`

## `wallet` command group

- `wallet registry-exists`
  - Output: `bool`
- `wallet list`
  - Output: `WalletMeta[]`
- `wallet active`
  - Output: active wallet id string or `null`
- `wallet create <NAME> [--birthday <HEIGHT>] [--mnemonic-language <LANG>]`
  - Output: wallet id string
- `wallet restore <NAME> <MNEMONIC> [--birthday <HEIGHT>] [--mnemonic-language <LANG>]`
  - Output: wallet id string
- `wallet import-viewing <NAME> [--sapling-viewing-key <KEY>] [--orchard-viewing-key <KEY>] --birthday <HEIGHT>`
  - Output: wallet id string
- `wallet switch <WALLET_ID>`
  - Output: `{"acknowledged": true}`
- `wallet rename <WALLET_ID> <NEW_NAME>`
  - Output: `{"acknowledged": true}`
- `wallet set-birthday <WALLET_ID> <HEIGHT>`
  - Output: `{"acknowledged": true}`
- `wallet delete <WALLET_ID>`
  - Output: `{"acknowledged": true}`

## Mnemonic helpers

- `generate-mnemonic [--word-count <COUNT>] [--mnemonic-language <LANG>]`
  - Output: mnemonic string
- `validate-mnemonic <MNEMONIC> [--mnemonic-language <LANG>]`
  - Output: `bool`

Supported `--mnemonic-language` values:

- `english`
- `chinese-simplified`
- `chinese-traditional`
- `french`
- `italian`
- `japanese`
- `korean`
- `spanish`

Behavior:

- `generate-mnemonic` uses the requested word list when the flag is present
- `validate-mnemonic` attempts autodetection when the flag is omitted
- `wallet restore` attempts autodetection when the flag is omitted

## `address` command group

These commands accept `--wallet-id <WALLET_ID>`. If omitted, the active wallet is used.

- `address current [--wallet-id <WALLET_ID>]`
  - Output: address string
- `address next [--wallet-id <WALLET_ID>]`
  - Output: address string
- `address list [--wallet-id <WALLET_ID>]`
  - Output: `AddressInfo[]`
- `address balances [--wallet-id <WALLET_ID>] [--key-id <KEY_ID>]`
  - Output: `AddressBalanceInfo[]`

## Legacy-compatible top-level commands

These commands preserve older CLI names. Wallet-scoped commands accept `--wallet-id <WALLET_ID>`. If it is omitted, the active wallet is used.

- `addresses [--wallet-id <WALLET_ID>]`
  - Output:
    - `z_addresses`
    - `t_addresses`
- `balance [--wallet-id <WALLET_ID>]`
  - Output:
    - `zbalance`
    - `verified_zbalance`
    - `spendable_zbalance`
    - `unverified_zbalance`
    - `tbalance`
    - `z_addresses`
    - `t_addresses`
- `transactions [--wallet-id <WALLET_ID>] [--limit <COUNT>]`
  - Output: `TxInfo[]`
- `list [--wallet-id <WALLET_ID>] [--limit <COUNT>]`
  - Output: Qortal-compatible transaction array
    - `txid`
    - `block_height`
    - `datetime`
    - `amount`
    - `fee`
    - `memo`
    - `incoming_metadata`
    - `outgoing_metadata`
    - `incoming_metadata_change`
    - `outgoing_metadata_change`
    - optional `unconfirmed`
- `lasttxid [--wallet-id <WALLET_ID>]`
  - Output:
    - `last_txid`
- `height [--wallet-id <WALLET_ID>]`
  - Output:
    - `height`
- `notes [--wallet-id <WALLET_ID>] [--all]`
  - Output: `NoteInfo[]`
- `info [--wallet-id <WALLET_ID>]`
  - Output:
    - `build`: `BuildInfo`
    - `network`: `NetworkInfo`
- `defaultfee`
  - Output:
    - `defaultfee`
- `new [--wallet-id <WALLET_ID>] [--key-id <KEY_ID>] [sapling|orchard|z]`
  - Output:
    - `pool`
    - `address`
- `seed [--wallet-id <WALLET_ID>] [--mnemonic-language <LANG>]`
  - Legacy raw advanced seed export
  - Intended for operator and integration use where local authorization UX is handled by the caller
  - Output:
    - `seed`
    - `birthday`
  - Behavior:
    - without `--mnemonic-language`, export uses the wallet's original stored mnemonic language
    - with `--mnemonic-language`, export re-renders the same seed entropy in the requested language
- `import <KEY> <BIRTHDAY> [--wallet-id <WALLET_ID>] [--name <NAME>] [--no-rescan]`
  - Accepted inputs:
    - Sapling spending key
    - Orchard spending key
    - Sapling viewing key
    - Orchard viewing key
  - Output when importing a spending key:
    - `key_id`
  - Output when importing a viewing wallet:
    - `wallet_id`
- `export [--wallet-id <WALLET_ID>] [TARGET]`
  - `TARGET` may be a key id or an address
  - Output with `TARGET`:
    - one `KeyExportInfo`
  - Output without `TARGET`:
    - `KeyExportInfo[]`
- `clear [--wallet-id <WALLET_ID>]`
  - Output: `{"acknowledged": true}`
- `syncstatus [--wallet-id <WALLET_ID>]`
  - Output: Qortal-compatible sync object
  - While syncing:
    - `sync_id`
    - `in_progress`
    - `last_error`
    - `start_block`
    - `end_block`
    - `synced_blocks`
    - `trial_decryptions_blocks`
    - `txn_scan_blocks`
    - `total_blocks`
    - `batch_num`
    - `batch_total`
  - When not syncing:
    - `sync_id`
    - `in_progress`
    - `last_error`
    - `scanned_height`
- `stop [--wallet-id <WALLET_ID>]`
  - Output: `{"acknowledged": true}`

## `sync` command group

If you run `sync` without a subcommand, it behaves like `sync start`.

- `sync [--wallet-id <WALLET_ID>] [compact|deep]`
  - Output: `{"acknowledged": true}`
- `sync start [--wallet-id <WALLET_ID>] [compact|deep]`
  - Output: `{"acknowledged": true}`
- `sync status [--wallet-id <WALLET_ID>]`
  - Output: `SyncStatus`
- `sync cancel [--wallet-id <WALLET_ID>]`
  - Output: `{"acknowledged": true}`
- `sync rescan [--wallet-id <WALLET_ID>] <FROM_HEIGHT>`
  - Output: `{"acknowledged": true}`

## `send` command group

The CLI supports both staged send and legacy one-shot send.
Change-address selection is automatic. Before Orchard activation, Sapling-only
change returns to the legacy first selected Sapling spend address; after Orchard
activation, Sapling-only change uses the wallet's internal Sapling change
address. Orchard spends or outputs continue to use Orchard internal change.

- `send build <WALLET_ID> <OUTPUTS_JSON> [--fee <ARRRTOSHIS>]`
  - `OUTPUTS_JSON` is a JSON array of:
    - `addr`
    - `amount`
    - optional `memo`
  - Output: `PendingTx`
- `send sign <WALLET_ID> <PENDING_JSON>`
  - `PENDING_JSON` must be a serialized `PendingTx`
  - Output: `SignedTx`
- `send broadcast <SIGNED_JSON>`
  - `SIGNED_JSON` must be a serialized `SignedTx`
  - Output: transaction id string
- `send '<LEGACY_REQUEST_JSON>'`
  - Request shape:
    - optional `input`
    - `output`: array of
      - `address`
      - `amount`
      - optional `memo`
    - optional `fee`
  - The legacy `input` field is accepted for compatibility and ignored.
  - Output:
    - `txid`

## `diag` command group

- `diag logs <WALLET_ID> [--limit <COUNT>]`
  - Output: `SyncLogEntryFfi[]`
- `diag checkpoint <WALLET_ID> <HEIGHT>`
  - Output: `CheckpointInfo | null`

## Direct service access

Use `exec-json` when you need a method that does not have a dedicated command yet.

Example:

```bash
./crates/target/release/piratewallet-cli exec-json '{"method":"get_fee_info"}'
```

## Commands intentionally left out

These older light-cli commands are not part of this CLI:

- `encryptmessage`
- `decryptmessage`
- `sendprogress`
- `arrrprice`

## Release handling

CLI publication is controlled by `release-artifacts.toml`.

On a release tag, CLI assets are only published when the CLI version changed from the previous tag.
