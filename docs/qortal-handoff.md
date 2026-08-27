# Qortal Integration Handoff

## Integration target

Qortal Core loads Pirate Wallet as a desktop JNI library through
`com.rust.litewalletjni.LiteWalletJni`. It does not launch a wallet CLI
process. The handoff artifact is therefore `pirate-qortal-jni`; the
`pirate-qortal-cli` binary remains useful for command-line testing only.

## Native artifacts

`scripts/build-qortal-jni.sh` produces these platform-specific libraries:

| Platform | File |
| --- | --- |
| Linux x86_64 | `librust-linux-x86_64.so` |
| Linux aarch64 | `librust-linux-aarch64.so` |
| Windows x86_64 | `librust-windows-x86_64.dll` |
| macOS x86_64 | `librust-macos-x86_64.dylib` |
| macOS aarch64 | `librust-macos-aarch64.dylib` |

Qortal Core must add the aarch64 filename to its platform selector before
Apple Silicon can load it natively. Qortal's current selector also maps
FreeBSD to the Linux filenames, but Linux GNU shared libraries are not FreeBSD
binaries. FreeBSD needs its own Rust target, build, filename, and tested runner;
it is not part of this artifact set.

The Java declarations to merge into Qortal Core are under
`bindings/qortal-jni/`.

The adapter preserves the legacy utility contracts as well as the command
surface: `initlogging()` returns `OK`, mnemonic generation returns
`seedPhrase`, validation returns `checkSeedPhrase: Ok/Error`, and wallet
initialization includes both `seed` and `birthday`.

## Required Qortal Core changes

### Configure storage before initialization

For each entropy-backed Qortal wallet, select a separate directory and call:

```java
LiteWalletJni.configurestorage(walletDirectory.toString(), encryptionKey);
```

Use a path below Qortal's existing Pirate Chain wallet directory, scoped by the
same entropy hash used for `wallet-<hash>.dat`. The existing
`ARRRWalletEncryption + entropy` key derivation can remain the encryption key.
This keeps different Qortal accounts isolated while allowing the unified core
to switch namespaces safely.

### Migrate the old wallet blob once

The old library serialized its complete wallet into `wallet-<hash>.dat`. The
unified core uses an encrypted SQLite registry and wallet database, so that blob
is not a compatible database format.

Qortal already derives the deterministic mnemonic from the same 32-byte
entropy. Migration is therefore:

1. Call `configurestorage()` for the entropy-specific namespace.
2. Derive the mnemonic with `getseedphrasefromentropyb64()`.
3. Call `initfromseed()` even when the old `.dat` file exists.
4. Start sync and confirm the expected address before allowing spending.
5. Archive or remove the old `.dat` file after the unified wallet has synced.

The JNI tests pin the legacy Sapling derivation path
`m/32'/141'/0'` against a known entropy/address vector. Ironwood account zero is
derived from the same BIP39 seed in addition to that unchanged Sapling account.

Subsequent starts call `configurestorage()` and `initfromseed()` again. The JNI
adapter selects the existing deterministic wallet instead of restoring a
duplicate. `initfromb64()` can select an already migrated unified database, but
it deliberately refuses to treat a legacy blob as SQLite.

The unified database persists every mutation. Remove the hourly `save()` and
load/write cycle from `PirateChainWalletController`; no explicit save is needed.

### Remove obsolete proving-parameter inputs

The JNI signatures retain `params`, `saplingOutputBase64`, and
`saplingSpendBase64` during the transition so the Java declaration remains
easy to merge. The unified core does not read them. Qortal can remove
`coinparams.json`, `saplingoutput_base64`, and `saplingspend_base64` from the
published library bundle after its Java integration stops checking for them.

### Update sync-status parsing

Use `in_progress`, not the older `syncing` field. While syncing, the object
contains:

- `sync_id` as a numeric, monotonically increasing session id
- `start_block`, `end_block`, `synced_blocks`, and `total_blocks`
- `trial_decryptions_blocks` and `txn_scan_blocks`
- `batch_num` and `batch_total`

When idle it contains `scanned_height`. The unified scanner processes block
download, trial decryption, and transaction recording as one pipeline, so the
two legacy scan counters report the same completed block range. It is exposed
as one logical batch (`batch_num: 0`, `batch_total: 1`).

## Command compatibility

`LiteWalletJni.execute(command, args)` accepts the commands used by Qortal Core:

| Command | Unified implementation |
| --- | --- |
| `sync` | Starts compact sync on the persistent service runtime |
| `syncstatus` / `syncStatus` | Returns the Qortal progress schema |
| `height` | Returns the local scanned height |
| `info` | Returns `latest_block_height`, querying the configured server before the first sync |
| `balance` | Returns shielded wallet totals and external receive-address balances |
| `list` | Returns incoming, outgoing, and change metadata |
| `export` | Returns the active-pool address and matching keys from its spendable key group |
| `send` | Selects the key group identified by the supplied wallet-owned input address |
| `sendp2sh` | Funds the supplied P2SH script from that key group's Sapling or Ironwood notes |
| `redeemp2sh` | Redeems or refunds funding output zero |
| `encryptionstatus` | Always reports encrypted storage |
| `encrypt`, `decrypt`, `unlock` | Transition-compatible success responses; storage is unlocked by `configurestorage()` |

Qortal request objects may use the legacy output field `address`; the unified
service also accepts its native field name `addr`.

The top-level `balance` totals include internal change. Its `z_addresses` array
contains external receive-address rows only, matching Qortal's address-picker
contract, and must not be summed to reconstruct the wallet total. For sends, the
supplied external address identifies its owning key group; note selection also
includes that group's internal change so post-Ironwood funds remain spendable.
The `export` command follows activation as well: it returns Sapling key material
before Ironwood and the matching Ironwood address and keys afterward.

Before the first sync, `height` reports the restore birthday rather than zero.
This preserves Qortal's initialization check without claiming the wallet is
current: `info` obtains the real chain tip, so Qortal's synchronization gate
still sees the wallet as behind. The JNI adapter uses direct transport to match
the legacy embedded wallet's network behavior.

`list` constructs incoming metadata from the encrypted note database and
recovers outgoing Sapling and Ironwood recipients from the raw transaction. If a
historical raw transaction is temporarily unavailable, the response emits one
`[UNKNOWN]` recipient with the correct external value so Qortal does not turn an
outgoing transaction into a zero-value transaction.

P2SH redemption verifies that the input is P2SH, the redeem script hashes to
that address, and funding output zero pays the same address. It rejects a
request when outputs plus the declared fee do not consume the exact funding
value, preventing an accidental remainder from becoming miner fee. Ironwood
redemption outputs obtain their anchor from lightwalletd, so Qortal's temporary
null-seed wallet does not need a separate sync first.

## Build and verify

From the repository root:

```bash
bash scripts/build-qortal-jni.sh
cd crates
cargo test -p pirate-qortal-jni --locked
cargo test -p pirate-wallet-service qortal --locked
cargo test -p pirate-cli-core --lib qortal_ --locked
cargo test -p pirate-core qortal_p2sh --locked -- --nocapture
```

The JNI library also exports `invokeJson(requestJson, pretty)`, which exposes
the typed `WalletServiceRequest` contract directly for future Qortal code that
no longer needs command-string compatibility.

## Verified spending-key recovery

Qortal recovery code should use `import_spending_key_verified`, not the older
`import_spending_key` request. The verified request imports exactly one pool at
a time and requires the caller to provide the receive address and its
sequential address index:

```json
{
  "method": "import_spending_key_verified",
  "wallet_id": "<wallet UUID>",
  "pool": "sapling",
  "spending_key": "<encoded spending key>",
  "expected_address": "<wallet receive address>",
  "address_index": 0,
  "label": "Recovered wallet",
  "birthday_height": 123456
}
```

Before modifying SQLite, the wallet service decodes the key and address for the
active wallet network and derives the address at `address_index`. The index is
bounded to 4096 so untrusted input cannot force an unbounded Sapling diversifier
search. The wallet must already have a nonzero known chain tip, and the birthday
must not exceed that tip. The known tip is persisted by a completed
synchronization and survives sync cancellation, including the internal
cancellation that ends a completed one-shot sync, so callers can synchronize,
cancel cleanly, and then import. Only a failed synchronization resets the
persisted heights. A mismatch is rejected without writing the key. A
successful request atomically stores the encrypted key, verified address, and
durable rescan-required state. Repeating the same request returns the existing
key group instead of inserting a duplicate, and an earlier repeated birthday
lowers the retained scan start.

The response contains only `key_id`, `pool`, `address`, `address_index`,
`birthday_height`, `already_imported`, `rescan_required`, and
`required_rescan_from_height`; it never returns the spending key. When
`required_rescan_from_height` is non-null, the caller must invoke `rescan` from
that height and keep sending disabled until spendability reports that the
rescan has completed. This field is the durable minimum across all pending
verified-key imports, so callers must not substitute the most recent key's
`birthday_height`. The native rescan path also clamps later caller requests to
this floor after a restart. A null value means no verified-key replay is
pending; `rescan_required` can still be true for a different wallet-wide
reason. An exact delayed retry is a true no-op: it preserves a completed rescan
and returns the wallet's current `rescan_required` state instead of disabling
spending again.

Valid all-uppercase Bech32 spending keys and addresses are accepted. Address
storage and responses use canonical lowercase; mixed-case and wrong-network
encodings are rejected. Callers do not need to normalize Bech32 casing before
invoking this request.

Starting the full birthday rescan deliberately clears any narrower queued
witness-repair range because the historical replay supersedes it. The storage
operation normally owns its immediate transaction. If a future native caller
invokes it inside an existing transaction, that outer caller owns rollback on
error. Before the transaction starts, the service serializes the import with
sync lifecycle operations and stops any active engine so the next engine loads
the updated account-key inventory.

This operation is the native prerequisite for importing external Pirate wallet
exports into Qortal's encrypted SQLite wallet. File parsing and user-facing
format selection remain Qortal-side follow-up work. Viewing-key recovery is not
covered by this request.
