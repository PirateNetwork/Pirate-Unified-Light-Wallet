# Qortal CLI Adapter

The Qortal-facing CLI lives in:

- `crates/pirate-qortal-cli`

The adapter code lives in:

- `crates/pirate-cli-core`

The P2SH backend lives in:

- `crates/pirate-wallet-service/src/api/qortal_p2sh.rs`
- `crates/pirate-core/src/qortal_p2sh.rs`

This is a separate adapter. The normal CLI stays generic.

## Build

```bash
cd crates
cargo build --release -p pirate-qortal-cli
```

Binary path:

```bash
./crates/target/release/pirate-qortal-cli
```

## Global flags

- `--format pretty`
- `--format json`

Every supported Qortal command accepts:

- `--wallet-id <WALLET_ID>`

If `--wallet-id` is omitted, the adapter uses the active wallet.

## Supported commands

- `syncstatus`
- `balance`
- `list`
- `sendp2sh`
- `redeemp2sh`

The adapter binary can also fall back to the normal CLI parser when invoked with non-Qortal command names. The supported Qortal contract is the command list above.

## Shared wallet-command path

`pirate-qortal-cli` is only the binary entrypoint. The actual command
implementation lives in:

- `crates/pirate-cli-core`

That means this binary can do two different things:

- run the dedicated Qortal adapter commands above
- fall back to the general wallet CLI for non-Qortal command names

Mnemonic-language support applies only to that shared wallet-command path, not
to the Qortal-specific adapter commands.

Examples:

```bash
./crates/target/release/pirate-qortal-cli generate-mnemonic --word-count 24 --mnemonic-language spanish
./crates/target/release/pirate-qortal-cli wallet restore "Recovered Wallet" "..." --mnemonic-language japanese
./crates/target/release/pirate-qortal-cli seed --wallet-id <wallet-id> --mnemonic-language italian
```

## `syncstatus`

Usage:

```bash
./crates/target/release/pirate-qortal-cli syncstatus --wallet-id <wallet-id>
```

Output while syncing:

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

Output when not syncing:

- `sync_id`
- `in_progress`
- `last_error`
- `scanned_height`

Notes:

- `sync_id` is the wallet id used for the request
- `last_error` is currently `null`
- `batch_num` and `batch_total` are currently `0`

## `balance`

Usage:

```bash
./crates/target/release/pirate-qortal-cli balance --wallet-id <wallet-id>
```

Output:

- `zbalance`
- `verified_zbalance`
- `spendable_zbalance`
- `unverified_zbalance`
- `tbalance`
- `z_addresses`
- `t_addresses`

Each `z_addresses` entry contains:

- `address`
- `zbalance`
- `verified_zbalance`
- `spendable_zbalance`
- `unverified_zbalance`

Notes:

- `tbalance` is always `0`
- `t_addresses` is always `[]`
- values come from the unified wallet backend and are shielded-only

## `list`

Usage:

```bash
./crates/target/release/pirate-qortal-cli list --wallet-id <wallet-id> --limit 20
```

Flags:

- `--wallet-id <WALLET_ID>`
- `--limit <COUNT>`

Output:

Array of transaction entries with:

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

Optional field:

- `unconfirmed`

Notes:

- `unconfirmed` is only present when the transaction is not confirmed
- the metadata arrays are currently empty arrays

## `sendp2sh`

Usage:

```bash
./crates/target/release/pirate-qortal-cli sendp2sh \
  '{"input":"zs1...","output":[{"addr":"zs1...","amount":200000,"memo":"optional"}],"script":"BASE58_ENCODED_SCRIPT_OUTPUT","fee":10000}' \
  --wallet-id <wallet-id>
```

Flags:

- `--wallet-id <WALLET_ID>`

Argument:

- `<REQUEST_JSON>`

Request JSON fields:

- `input`
  - wallet-owned source address
- `output`
  - array of
    - `addr`
    - `amount`
    - optional `memo`
- `script`
  - Base58-encoded output script bytes
- `fee`
  - fee in arrrtoshis

Validation rules:

- `input` must not be empty
- `output` must not be empty
- each output `amount` must be non-zero
- `script` must decode from Base58 and must not be empty
- the source address must belong to the wallet
- the source key group must be spendable

Behavior:

- the command spends wallet-owned shielded notes selected from the provided source address
- one P2SH script output is added per recipient
- shielded change follows the wallet's fixed internal change-address policy for that key group

Output:

- `txid`

## `redeemp2sh`

Usage:

```bash
./crates/target/release/pirate-qortal-cli redeemp2sh \
  '{"input":"t3...","output":[{"addr":"zs1...","amount":200000,"memo":"optional"}],"fee":10000,"script":"BASE58_ENCODED_REDEEM_SCRIPT","txid":"BASE58_ENCODED_32_BYTE_TXID","locktime":0,"secret":"BASE58_SECRET_OR_EMPTY_FOR_REFUND","privkey":"BASE58_ENCODED_32_BYTE_PRIVATE_KEY"}' \
  --wallet-id <wallet-id>
```

Flags:

- `--wallet-id <WALLET_ID>`

Argument:

- `<REQUEST_JSON>`

Request JSON fields:

- `input`
  - transparent P2SH address
- `output`
  - array of
    - `addr`
    - `amount`
    - optional `memo`
- `fee`
  - fee in arrrtoshis
- `script`
  - Base58-encoded redeem script
- `txid`
  - Base58-encoded 32-byte funding transaction id
- `locktime`
  - `0` for redeem
  - greater than `0` for refund
- `secret`
  - required for redeem
  - must be empty for refund
- `privkey`
  - Base58-encoded 32-byte private key

Validation rules:

- `input` must not be empty
- `output` must not be empty
- each output `amount` must be non-zero
- `script` must decode from Base58 and must not be empty
- `txid` must decode to 32 bytes
- `privkey` must decode to 32 bytes
- `locktime == 0` requires a non-empty `secret`
- `locktime > 0` requires an empty `secret`

Behavior:

- the adapter spends output index `0` of the funding transaction
- it signs the transparent P2SH spend and creates the requested outputs

Output:

- `txid`

## Output contract summary

The adapter matches the Qortal contract for:

- `syncstatus`
- `balance`
- `list`
- `sendp2sh`
- `redeemp2sh`

Output summary:

- `syncstatus`
  - sync progress object
- `balance`
  - shielded balance object with empty transparent fields
- `list`
  - transaction array
- `sendp2sh`
  - `{"txid":"<transaction-id>"}`
- `redeemp2sh`
  - `{"txid":"<transaction-id>"}`

The underlying Qortal-specific implementation lives in:

- `crates/pirate-cli-core`
- `crates/pirate-wallet-service/src/api/qortal_p2sh.rs`
- `crates/pirate-core/src/qortal_p2sh.rs`

## Checks

Useful commands:

```bash
cd crates
cargo test -p pirate-cli-core --lib qortal_
cargo check -p pirate-wallet-service
cargo test -p pirate-core qortal_p2sh -- --nocapture
cargo build --release -p pirate-qortal-cli
```
