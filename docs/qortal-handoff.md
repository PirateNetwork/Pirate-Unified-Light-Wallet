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
`m/32'/141'/0'` against a known entropy/address vector. Orchard account zero is
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
| `balance` | Returns shielded totals and per-address balances |
| `list` | Returns incoming, outgoing, and change metadata |
| `export` | Returns the primary spendable key group |
| `send` | Restricts note selection to the supplied wallet-owned input address |
| `sendp2sh` | Funds the supplied P2SH script from Sapling or Orchard notes |
| `redeemp2sh` | Redeems or refunds funding output zero |
| `encryptionstatus` | Always reports encrypted storage |
| `encrypt`, `decrypt`, `unlock` | Transition-compatible success responses; storage is unlocked by `configurestorage()` |

Qortal request objects may use the legacy output field `address`; the unified
service also accepts its native field name `addr`.

Before the first sync, `height` reports the restore birthday rather than zero.
This preserves Qortal's initialization check without claiming the wallet is
current: `info` obtains the real chain tip, so Qortal's synchronization gate
still sees the wallet as behind. The JNI adapter uses direct transport to match
the legacy embedded wallet's network behavior.

`list` constructs incoming metadata from the encrypted note database and
recovers outgoing Sapling and Orchard recipients from the raw transaction. If a
historical raw transaction is temporarily unavailable, the response emits one
`[UNKNOWN]` recipient with the correct external value so Qortal does not turn an
outgoing transaction into a zero-value transaction.

P2SH redemption verifies that the input is P2SH, the redeem script hashes to
that address, and funding output zero pays the same address. It rejects a
request when outputs plus the declared fee do not consume the exact funding
value, preventing an accidental remainder from becoming miner fee. Orchard
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
