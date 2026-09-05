# react-native-pirate-wallet

`react-native-pirate-wallet` is the React Native wrapper for the unified Pirate wallet backend.

It exposes one JS API over the same native service layer used by the Android SDK and iOS SDK.

The package is meant for React Native wallets such as Edge Wallet.

Repo-level build and integration notes:

- `docs/react-native-plugin.md`

## What it wraps

- Android: JNI bridge over `libpirate_ffi_native.so`
- iOS: Objective-C bridge over `PirateWalletNative.xcframework`
- JS: typed wallet wrapper plus a polling synchronizer

The JS surface mirrors the SDK boundary used by the native Android and iOS SDKs.

## Account-scoped wallet storage

Configure wallet storage before any wallet operation. The recommended mobile
path creates a random registry credential inside iOS Keychain or Android
Keystore and never returns it to JavaScript:

```js
const sdk = createPirateWalletSdk()

await sdk.configureSecureAccountStorage({
  accountId: edgeAccountIdHash
})
```

The native module creates an app-private directory for that account and asks the
Rust service to open or create that account's wallet namespace. The selected
directory contains:

- `wallet_registry.db`
- per-wallet database files
- database salts
- sealed database key files

The protected device credential unlocks the registry and per-wallet databases
in that namespace so viewing data and the compact-block cache can support
concurrent synchronization before an Edge account is unlocked.

By default, Android derives the directory under:

```text
Context.filesDir/pirate_wallet/accounts/<sanitized-account-id>
```

iOS derives the directory under:

```text
Application Support/PirateWallet/accounts/<sanitized-account-id>
```

Integrations that need to provide their own app-private path may pass
`storagePath`:

```js
await sdk.configureSecureAccountStorage({
  accountId: edgeAccountIdHash,
  storagePath: accountPrivatePath
})
```

## Repo layout

- `android/`
- `example/`
- `ios/`
- `scripts/`
- `src/`

Native libraries are distributed in exact-version Android ARM, Android x86_64,
iOS device, iOS simulator arm64, and iOS simulator x86_64 companion packages.
On macOS, CocoaPods combines the two thin simulator archives into the universal
XCFramework slice expected by Xcode.

## Preparing native artifacts in this repo

Before testing or packaging this plugin from the monorepo, stage the native artifacts:

```bash
bash scripts/prepare-react-native-plugin.sh
```

That copies:

- Android JNI libraries into the two Android companion packages
- the iOS device XCFramework slice and two thin simulator archives into the
  three iOS companion packages

There is also a minimal consumer app in:

- `bindings/react-native-pirate-wallet/example/`

That app is used to verify install, autolinking, and a couple of real native calls.

## Public JS surface

Main exports:

- `PirateWalletSdk`
- `PirateWalletSynchronizer`
- `createPirateWalletSdk()`

The synchronizer is implemented in JS and polls the native service through the bridge. It does not depend on native event emitters.

## RPC and API reference

The JS wrapper is a typed layer over the native `invoke(requestJson, pretty)` bridge.

Low-level entry points:

- `sdk.invoke(requestJson, pretty = false)`
  - sends a raw JSON request to the native bridge
  - returns a JSON envelope string
- `sdk.configureSecureAccountStorage({ accountId, storagePath? })`
  - recommended mobile entry point
  - stores a random registry credential in iOS Keychain or Android Keystore
  - the credential never crosses the React Native bridge
  - selects an account-specific registry/database directory
- `sdk.configureAccountStorage({ accountId, passphrase, storagePath? })`
  - advanced compatibility entry point for hosts that already protect the credential
  - RPC: `configure_wallet_storage`
  - selects an account-specific registry/database directory
  - creates the registry with `passphrase` if it does not exist
  - unlocks the existing registry with `passphrase` if it already exists
  - clears loaded registry state, active wallet state, DB caches, and sync caches
    before switching namespaces
- `sdk.buildInfoJson(pretty = false)`
  - raw JSON envelope for `get_build_info`
- `sdk.buildInfo()`
  - RPC: `get_build_info`
  - returns:
    - `version`
    - `gitCommit`
    - `buildDate`
    - `rustVersion`
    - `targetTriple`
- `createPirateWalletSdk()`
  - returns a new `PirateWalletSdk` instance backed by the linked native module

The typed JS methods below unwrap the native JSON envelope and return the `result` value directly.

### Amount wire format

All arrrtoshi amount values on the JSON wire are decimal strings, not JSON
numbers. This includes balances, transaction amounts, fees, pending
transaction totals, payment disclosure amounts, and `parseAmount()` results.
Amount request fields accept decimal strings, safe integer numbers, or
`bigint`; the JS wrapper serializes them as strings before calling native code.

### Wallet lifecycle

- `walletRegistryExists()`
  - RPC: `wallet_registry_exists`
  - returns `boolean`
- `listWallets()`
  - RPC: `list_wallets`
  - returns `WalletMeta[]`
- `getActiveWalletId()`
  - RPC: `get_active_wallet`
  - returns `string | null`
- `getActiveWallet()`
  - helper over `getActiveWalletId()` and `listWallets()`
  - returns `WalletMeta | null`
- `getWallet(walletId)`
  - helper over `listWallets()`
  - returns `WalletMeta | null`
- `createWallet(requestOrName, birthdayHeight?)`
  - RPC: `create_wallet`
  - request fields:
    - `name`
    - optional `birthdayHeight`
    - optional `mnemonicLanguage`
  - returns wallet id string
- `restoreWallet(requestOrName, mnemonic?, birthdayHeight?, mnemonicLanguage?)`
  - RPC: `restore_wallet`
  - request fields:
    - `name`
    - `mnemonic`
    - optional `birthdayHeight`
    - optional `mnemonicLanguage`
  - returns wallet id string
- `importViewingWallet(requestOrName, saplingViewingKey?, ironwoodViewingKey?, birthdayHeight)`
  - RPC: `import_viewing_wallet`
  - request fields:
    - `name`
    - optional `saplingViewingKey`
    - optional `ironwoodViewingKey`
    - `birthdayHeight`
  - returns wallet id string
- `switchWallet(walletId)`
  - RPC: `switch_wallet`
  - returns acknowledgement object
- `renameWallet(walletId, newName)`
  - RPC: `rename_wallet`
  - returns acknowledgement object
- `deleteWallet(walletId)`
  - RPC: `delete_wallet`
  - returns acknowledgement object
- `setWalletBirthdayHeight(walletId, birthdayHeight)`
  - RPC: `set_wallet_birthday_height`
  - returns acknowledgement object
- `getLatestBirthdayHeight(walletId)`
  - helper over `getWallet(walletId)`
  - returns `number | null`

#### Active wallet and wallet IDs

Wallet metadata is stored in the backend registry. The registry also persists
an active wallet ID, which acts as the SDK's current-wallet pointer for flows
that need one. Integrations that already keep their own wallet selection can
call wallet-scoped methods directly with `walletId`.

`switchWallet(walletId)` updates the active-wallet pointer, records last-used
metadata, and stops sync for the previously active wallet. Multi-wallet sync
should be driven by wallet-scoped synchronizers rather than by switching the
active wallet between running wallets.

### Mnemonic, formatting, and network

- `generateMnemonic(wordCount?, mnemonicLanguage?)`
  - RPC: `generate_mnemonic`
  - returns mnemonic string
- `validateMnemonic(mnemonic, mnemonicLanguage?)`
  - RPC: `validate_mnemonic`
  - returns `boolean`
- `inspectMnemonic(mnemonic)`
  - RPC: `inspect_mnemonic`
  - returns:
    - `isValid`
    - `detectedLanguage`
    - `ambiguousLanguages`
    - `wordCount`
- `getNetworkInfo()`
  - RPC: `get_network_info`
  - returns:
    - `name`
    - `coinType`
    - `rpcPort`
    - `defaultBirthday`

### Lightwalletd endpoints and failover pools

Endpoint configuration is wallet-scoped. The recommended integration flow is
to test candidate servers, save one primary plus its alternates, and then start
or restart that wallet's synchronizer:

```js
const primary = 'https://lightd1.pirate.black:443'
const alternates = [
  'https://lightwalletd1.cryptoforge.cc:443',
  'https://pirate.mathnodes.com:443'
]

const tests = await Promise.all(
  [primary, ...alternates].map(url => sdk.testLightdEndpoint({ url }))
)

if (tests.every(result => result.success)) {
  await sdk.setLightdEndpointPool({
    walletId,
    url: primary,
    failoverEndpoints: alternates
  })
}

const saved = await sdk.getLightdEndpointConfig(walletId)
```

- `getLightdEndpoint(walletId)`
  - RPC: `get_lightd_endpoint`
  - returns the effective primary endpoint URL
- `getLightdEndpointConfig(walletId)`
  - RPC: `get_lightd_endpoint_config`
  - returns `LightdEndpointConfig`:
    - `host`
    - `port`
    - `useTls`
    - `tlsPin`
    - `label`
    - `automaticFailover`
    - `failoverEndpoints`
    - `isConfigured`
- `testLightdEndpoint({ url, tlsPin? })`
  - RPC: `test_node`
  - also accepts `testLightdEndpoint(url, tlsPin?)`
  - tests through the currently selected Direct, Tor, SOCKS5, or I2P transport
  - reports success, height, latency, transport, TLS/pin information, server
    version, chain name, and any connection error
- `setLightdEndpoint({ walletId, url, tlsPin? })`
  - RPC: `set_lightd_endpoint`
  - also accepts `setLightdEndpoint(walletId, url, tlsPin?)`
  - saves one primary and clears any previously configured failover pool
- `setLightdEndpointPool({ walletId, url, failoverEndpoints, tlsPin? })`
  - RPC: `set_lightd_endpoint_pool`
  - also accepts
    `setLightdEndpointPool(walletId, url, failoverEndpoints, tlsPin?)`
  - saves the primary and up to 16 explicit alternates
  - an empty `failoverEndpoints` array disables automatic failover

Pool membership is validated by the backend before anything is persisted.
Every member must resolve to the same recognized Pirate network, use the same
clearnet, onion, or I2P route, and use the same HTTP/TLS security mode as the
primary. The primary is removed from the alternate list and duplicate
alternates are collapsed. A pinned primary cannot use automatic failover,
because one server's SPKI pin cannot authenticate unrelated servers; use
`setLightdEndpoint()` when pinning a single server.

Saving either endpoint form cancels an existing sync session for that wallet so
it cannot continue against stale connection state. Restart the synchronizer
after the setter resolves. Pool candidates are still checked for compatible
chain identity and history before failover or historical work is assigned; the
array order is not a request to trust a candidate blindly.

`testLightdEndpoint()` returns a structured failure result for connection-level
failures. Invalid setter input or a rejected pool throws through the normal SDK
promise, so applications should show the error and retain the previous saved
configuration.

- `formatAmount(arrrtoshis)`
  - RPC: `format_amount`
  - returns formatted string
- `parseAmount(arrr)`
  - RPC: `parse_amount`
  - returns integer arrrtoshis as a decimal string

### Validation

- `isValidShieldedAddr(address)`
  - RPC: `is_valid_shielded_address`
  - returns `boolean`
- `validateAddress(address)`
  - RPC: `validate_address`
  - returns:
    - `isValid`
    - `addressType`
    - `reason`
- `validateConsensusBranch(walletId)`
  - RPC: `validate_consensus_branch`
  - returns:
    - `sdkBranchId`
    - `serverBranchId`
    - `isValid`
    - `hasServerBranch`
    - `hasSdkBranch`
    - `isServerNewer`
    - `isSdkNewer`
    - `errorMessage`

  Consensus branch IDs are opaque. Use `isValid` for compatibility; the two
  `*Newer` fields remain for wire compatibility and are always `false`.

### Addresses and balances

Receive-address APIs are shielded and wallet-scoped:

- `getCurrentReceiveAddress(walletId)`
  - helper over `getCurrentAddress(walletId)`
- `getCurrentAddress(walletId)`
  - RPC: `current_receive_address`
  - returns the current external receive address without rotating it
- `getNextReceiveAddress(walletId)`
  - helper over `getNextAddress(walletId)`
- `getNextAddress(walletId)`
  - RPC: `next_receive_address`
  - rotates to and returns the next external receive address
- `listAddresses(walletId)`
  - RPC: `list_addresses`
  - returns generated external receive addresses
- `listAddressBalances(walletId, keyId?)`
  - RPC: `list_address_balances`
  - without `keyId`, returns external receive-address balance entries only
  - with `keyId`, also returns internal change-address entries for that key group

These APIs return shielded receive addresses. Newly generated addresses use
Sapling before Ironwood activation and Ironwood after activation. At activation,
both current- and next-address calls select Ironwood; existing Sapling addresses
remain valid, so `listAddresses(walletId)` can contain both pools over time.

Internal change is always included in `getBalance(walletId)`. Do not sum an
unfiltered `listAddressBalances(walletId)` response to calculate the wallet
total, because its default external-only view intentionally omits internal
address rows.

- `getBalance(walletId)`
  - RPC: `get_balance`
  - returns decimal-string amount fields:
    - `total`
    - `spendable`
    - `pending`
- `getShieldedPoolBalances(walletId)`
  - RPC: `get_shielded_pool_balances`
  - returns:
    - `sapling`
    - `ironwood`
- `getSpendabilityStatus(walletId)`
  - RPC: `get_spendability_status`
  - returns:
    - `spendable`
    - `rescanRequired`
    - `targetHeight`
    - `anchorHeight`
    - `validatedAnchorHeight`
    - `repairQueued`
    - `reasonCode`

`reasonCode` is a closed set:

- `OK`: signing may proceed
- `ERR_SYNC_FINALIZING`: scanning reached the tip but anchor validation is still finishing
- `ERR_WITNESS_REPAIR_QUEUED`: witness repair is queued or actively processing
- `ERR_RESCAN_REQUIRED`: imported key material or local state requires a historical replay

Keep the send action disabled unless `spendable` is `true`. A queued repair
remains visible until witness reconstruction and anchor validation both finish.

- `getLightdEndpointPoolDiagnostics(walletId)`
  - RPC: `get_lightd_endpoint_pool_diagnostics`
  - performs a live readiness and same-chain probe using the wallet's current transport
  - returns the configured primary, selected active endpoint, failover mode, and per-endpoint health, tip, latency, and rejection reason
  - `activeEndpoint` is `null` when no configured candidate passes the complete probe

### Transactions

- `listTransactions(walletId, limit?)`
  - RPC: `list_transactions`
  - returns transaction array
- `fetchTransactionMemo(walletId, txId, outputIndex?)`
  - RPC: `fetch_transaction_memo`
  - returns `string | null`
- `getTransactionDetails(walletId, txId)`
  - RPC: `get_transaction_details`
  - returns transaction detail object or `null`
- `exportPaymentDisclosures(walletId, txId)`
  - RPC: `export_payment_disclosures`
  - returns all recoverable payment disclosures for a sent transaction
- `exportSaplingPaymentDisclosure(walletId, txId, outputIndex)`
  - RPC: `export_sapling_payment_disclosure`
  - returns one Sapling output disclosure string
- `exportIronwoodPaymentDisclosure(walletId, txId, actionIndex)`
  - RPC: `export_ironwood_payment_disclosure`
  - returns one Ironwood action disclosure string
- `verifyPaymentDisclosure(walletId, disclosure)`
  - RPC: `verify_payment_disclosure`
  - decrypts one Sapling or Ironwood disclosure using the wallet's configured lightwalletd endpoint

`PaymentDisclosure` includes `disclosureType`, `txid`, `outputIndex`, `address`,
`amount`, optional `memo`, and the shareable `disclosure` string.
`verifyPaymentDisclosure` returns the same decrypted payment fields plus
`memoHex`.
- `getFeeInfo()`
  - RPC: `get_fee_info`
  - returns decimal-string fee fields plus `memoFeeMultiplier`:
    - `defaultFee`
    - `minFee`
    - `maxFee`
    - `feePerOutput`
    - `memoFeeMultiplier`

### Sync

- `startSync(walletIdOrRequest, mode = 'Compact')`
  - RPC: `start_sync`
  - request fields:
    - `walletId`
    - `mode`
  - returns acknowledgement object
- `getSyncStatus(walletId)`
  - RPC: `sync_status`
  - returns:
    - `localHeight`
    - `targetHeight`
    - `percent`
    - `eta`
    - `stage`
    - `lastCheckpoint`
    - `blocksPerSecond`
    - `notesDecrypted`
    - `lastBatchMs`
- `cancelSync(walletId)`
  - RPC: `cancel_sync`
  - returns acknowledgement object
- `rescan(walletIdOrRequest, fromHeight?)`
  - RPC: `rescan`
  - request fields:
    - `walletId`
    - `fromHeight`
  - returns acknowledgement object

Each wallet has its own sync state. Apps can run more than one wallet sync by
creating synchronizers for different wallet IDs, subject to normal device,
network, and lightwalletd resource limits. Compact block ranges are cached per
endpoint, so later scans for another wallet on the same endpoint can reuse
previously fetched ranges.

### Send flow

- `buildTransaction(walletIdOrRequest, outputs?, fee?)`
  - RPC: `build_tx`
  - request fields:
    - `walletId`
    - `outputs`
    - optional `fee`
  - each output contains:
    - `addr`
    - `amount`
    - optional `memo`
  - returns pending transaction object
- `signTransaction(walletId, pending)`
  - RPC: `sign_tx`
  - returns signed transaction object
- `broadcastTransaction(walletId, signed)`
  - RPC: `broadcast_tx`
  - uses the specified wallet's endpoint pool and repair state
  - returns transaction id string
- `send(walletId, outputsOrOutput, fee?)`
  - helper over `buildTransaction()`, `signTransaction()`, and `broadcastTransaction()`
  - returns transaction id string

`buildTransaction()`, `signTransaction()`, `broadcastTransaction()`, and
`send()` are wallet-scoped. `broadcastTransaction()` requires the wallet ID so
endpoint selection and repair state always belong to the wallet that created
the transaction.

### Wallet signing sessions

Wallet-scoped signing protection is opt-in and additive. Enabling it wraps the
seed and spending keys with a second key derived from the Edge account session
credential. Viewing keys and cached compact blocks remain available while the
wallet is locked.

```js
// Run once after creating or restoring this wallet.
await sdk.enableWalletSigningProtection(walletId, edgeAccountSessionCredential)

// Run after the Edge account is unlocked in a later app session.
await sdk.unlockWalletSigning(walletId, edgeAccountSessionCredential)

// Gate the send action on both status calls.
const signing = await sdk.getWalletSigningStatus(walletId)
const spendability = await sdk.getSpendabilityStatus(walletId)

// Run when the account locks, the app signs out, or protected state is cleared.
await sdk.lockWalletSigning(walletId)
```

- `enableWalletSigningProtection(walletId, sessionCredential)` performs the
  one-time atomic key rewrap and leaves that wallet unlocked for the session
- `unlockWalletSigning(walletId, sessionCredential)` installs only that
  wallet's signing key in memory
- `getWalletSigningStatus(walletId)` returns `protectionEnabled` and `unlocked`
- `lockWalletSigning(walletId)` clears the credential and cached wallet database handles
- `lockAllWalletSigning()` clears all signing sessions and wallet database handles

Once protection is enabled, `signTransaction()` fails with
`ERR_SIGNING_SESSION_LOCKED` until the wallet is unlocked. Do not persist the
session credential in AsyncStorage, Redux persistence, logs, or crash reports.

Change-address selection is automatic. Sapling-only change uses legacy
same-address change before Ironwood activation and Sapling internal change after
activation; Ironwood spends or outputs use Ironwood internal change.

### Viewing keys and watch-only

- `exportSaplingViewingKey(walletId)`
  - RPC: `export_sapling_viewing_key`
  - returns Sapling viewing key string
- `exportIronwoodViewingKey(walletId)`
  - RPC: `export_ironwood_viewing_key`
  - returns Ironwood viewing key string
- `importSaplingViewingKeyAsWatchOnly(requestOrName, saplingViewingKey?, birthdayHeight?)`
  - RPC: `import_sapling_viewing_key_as_watch_only`
  - returns wallet id string
- `getWatchOnlyCapabilities(walletId)`
  - RPC: `get_watch_only_capabilities`
  - returns capability object

### Advanced key management

These methods live under `sdk.advancedKeyManagement`.

- `listKeyGroups(walletId)`
  - RPC: `list_key_groups`
  - returns key group array
- `exportKeyGroupKeys(walletId, keyId)`
  - RPC: `export_key_group_keys`
  - returns:
    - `keyId`
    - `saplingViewingKey`
    - `ironwoodViewingKey`
    - `saplingSpendingKey`
    - `ironwoodSpendingKey`
- `importSpendingKey(requestOrWalletId, birthdayHeight?, saplingSpendingKey?, ironwoodSpendingKey?)`
  - RPC: `import_spending_key`
  - returns key id number
- `exportSeed(walletId, mnemonicLanguage?)`
  - RPC: `export_seed_raw`
  - returns mnemonic string

### Mnemonic language values

Where `mnemonicLanguage` is supported, the accepted values are:

- `english`
- `chinese_simplified`
- `chinese_traditional`
- `french`
- `italian`
- `japanese`
- `korean`
- `spanish`

Behavior:

- if omitted during `restoreWallet()` or `validateMnemonic()`, the backend attempts autodetection
- if omitted during `exportSeed()`, the wallet's original stored mnemonic language is used
- if provided during export, the same seed entropy is re-rendered in the requested language

### Synchronizer

Create a synchronizer with:

- `createSynchronizer(walletId, config?)`

Public state:

- `status`
- `progress`
- `syncStatus`
- `latestBirthdayHeight`
- `balance`
- `transactions`
- `lastError`

Methods:

- `currentSnapshot()`
- `isRunning()`
- `isSyncing()`
- `isComplete()`
- `start()`
- `stop()`
- `refresh()`
- `close()`
- `subscribe(callbacks?)`

`stop()` and `close()` both cancel backend sync for the wallet. In React Native code,
`await synchronizer.close()` instead of treating `close()` as a local timer-only cleanup step.

A synchronizer is scoped to one wallet ID. Create one synchronizer per wallet
when running multi-wallet sync.

Callback hooks:

- `onStatusChanged`
- `onUpdate`
- `onError`

## Install in a React Native app

Install the package in the app and run CocoaPods as usual:

```bash
npm install react-native-pirate-wallet
cd ios && pod install
```

On Android, npm installs the exact-version ARM and x86_64 companions
automatically. The wrapper autolinks as a standard React Native native module
and adds their JNI libraries to the build.

On macOS, npm installs the exact-version device and simulator companions.
During `pod install`, the podspec assembles and links
`PirateWalletNative.xcframework` from those packages.
