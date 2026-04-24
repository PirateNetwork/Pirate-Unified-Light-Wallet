# react-native-pirate-wallet

`react-native-pirate-wallet` is the React Native wrapper for the unified Pirate wallet backend.

It exposes one JS API over the same native service layer used by the Android SDK and iOS SDK.

The package is meant for React Native wallets such as Edge Wallet.

Repo-level build and integration notes:

- `docs/react-native-plugin.md`

## What it wraps

- Android: JNI bridge over `libpirate_ffi_native.so`
- iOS: Swift bridge over `PirateWalletNative.xcframework`
- JS: typed wallet wrapper plus a polling synchronizer

The JS surface mirrors the SDK boundary used by the native Android and iOS SDKs.

## Repo layout

- `android/`
- `example/`
- `ios/`
- `src/`

## Preparing native artifacts in this repo

Before testing or packaging this plugin from the monorepo, stage the native artifacts:

```bash
bash scripts/prepare-react-native-plugin.sh
```

That copies:

- Android JNI libraries from `bindings/android-sdk/src/main/jniLibs/`
- iOS XCFramework output from `bindings/ios-sdk/Frameworks/`

into this React Native package.

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
- `importViewingWallet(requestOrName, saplingViewingKey?, orchardViewingKey?, birthdayHeight)`
  - RPC: `import_viewing_wallet`
  - request fields:
    - `name`
    - optional `saplingViewingKey`
    - optional `orchardViewingKey`
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
- `formatAmount(arrrtoshis)`
  - RPC: `format_amount`
  - returns formatted string
- `parseAmount(arrr)`
  - RPC: `parse_amount`
  - returns integer arrrtoshis

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
  - returns per-address balance entries

These APIs return shielded receive addresses. Newly generated addresses use
Sapling before Orchard activation and Orchard after activation. The current
address can still be an older Sapling address until the wallet rotates, and
`listAddresses(walletId)` can contain both Sapling and Orchard receive
addresses over time.

- `getBalance(walletId)`
  - RPC: `get_balance`
  - returns:
    - `total`
    - `spendable`
    - `pending`
- `getShieldedPoolBalances(walletId)`
  - RPC: `get_shielded_pool_balances`
  - returns:
    - `sapling`
    - `orchard`
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
- `exportOrchardPaymentDisclosure(walletId, txId, actionIndex)`
  - RPC: `export_orchard_payment_disclosure`
  - returns one Orchard action disclosure string
- `verifyPaymentDisclosure(walletId, disclosure)`
  - RPC: `verify_payment_disclosure`
  - decrypts one Sapling or Orchard disclosure using the wallet's configured lightwalletd endpoint

`PaymentDisclosure` includes `disclosureType`, `txid`, `outputIndex`, `address`,
`amount`, optional `memo`, and the shareable `disclosure` string.
`verifyPaymentDisclosure` returns the same decrypted payment fields plus
`memoHex`.
- `getFeeInfo()`
  - RPC: `get_fee_info`
  - returns:
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
- `broadcastTransaction(signed)`
  - RPC: `broadcast_tx`
  - returns transaction id string
- `send(walletId, outputsOrOutput, fee?)`
  - helper over `buildTransaction()`, `signTransaction()`, and `broadcastTransaction()`
  - returns transaction id string

`buildTransaction()`, `signTransaction()`, and `send()` are wallet-scoped.
`broadcastTransaction(signed)` only receives the signed transaction payload; if
endpoint configuration is needed during broadcast, the service uses the active
wallet.

Change-address selection is automatic. Sapling-only change uses legacy
same-address change before Orchard activation and Sapling internal change after
activation; Orchard spends or outputs use Orchard internal change.

### Viewing keys and watch-only

- `exportSaplingViewingKey(walletId)`
  - RPC: `export_sapling_viewing_key`
  - returns Sapling viewing key string
- `exportOrchardViewingKey(walletId)`
  - RPC: `export_orchard_viewing_key`
  - returns Orchard viewing key string
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
    - `orchardViewingKey`
    - `saplingSpendingKey`
    - `orchardSpendingKey`
- `importSpendingKey(requestOrWalletId, birthdayHeight?, saplingSpendingKey?, orchardSpendingKey?)`
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

## Install in a React Native app

Install the package in the app and run CocoaPods as usual:

```bash
npm install react-native-pirate-wallet
cd ios && pod install
```

On Android, the package autolinks as a standard React Native native module.

On iOS, the podspec links the vendored `PirateWalletNative.xcframework`.
