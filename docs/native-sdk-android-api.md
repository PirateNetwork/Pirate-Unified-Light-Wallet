# Android SDK API Reference

This page lists the public Android SDK surface in:

- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSdk.kt`
- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSdkModels.kt`
- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSynchronizer.kt`

## Main entry points

- `PirateWalletSdk`
- `PirateWalletSynchronizer`
- `PirateWalletAdvancedKeyManagement`

## PirateWalletSdk

Core:

- `invoke(requestJson, pretty = false)`
- `createSynchronizer(walletId, config = PirateWalletSynchronizer.Config())`
- `buildInfoJson(pretty = false)`
- `buildInfo()`

Wallet lifecycle:

- `walletRegistryExists()`
- `listWallets()`
- `getActiveWalletId()`
- `getActiveWallet()`
- `getWallet(walletId)`
- `createWallet(request)`
- `createWallet(name, birthdayHeight = null, mnemonicLanguage = null)`
- `restoreWallet(request)`
- `restoreWallet(name, mnemonic, birthdayHeight = null, mnemonicLanguage = null)`
- `importViewingWallet(request)`
- `importViewingWallet(name, saplingViewingKey = null, orchardViewingKey = null, birthdayHeight)`
- `switchWallet(walletId)`
- `renameWallet(walletId, newName)`
- `deleteWallet(walletId)`
- `setWalletBirthdayHeight(walletId, birthdayHeight)`
- `getLatestBirthdayHeight(walletId)`

Mnemonic and formatting:

- `generateMnemonic(wordCount = null, mnemonicLanguage = null)`
- `validateMnemonic(mnemonic, mnemonicLanguage = null)`
- `inspectMnemonic(mnemonic)`
- `getNetworkInfo()`
- `formatAmount(arrrtoshis)`
- `parseAmount(arrr)`

Validation:

- `isValidShieldedAddr(address)`
- `validateAddress(address)`
- `validateConsensusBranch(walletId)`

Addresses:

- `getCurrentReceiveAddress(walletId)`
- `getCurrentAddress(walletId)`
- `getNextReceiveAddress(walletId)`
- `getNextAddress(walletId)`
- `listAddresses(walletId)`
- `listAddressBalances(walletId, keyId = null)`

Balances and transaction inspection:

- `getBalance(walletId)`
- `getShieldedPoolBalances(walletId)`
- `getSpendabilityStatus(walletId)`
- `listTransactions(walletId, limit = null)`
- `fetchTransactionMemo(walletId, txId, outputIndex = null)`
- `getTransactionDetails(walletId, txId)`
- `getFeeInfo()`

Sync:

- `startSync(request)`
- `startSync(walletId, mode = SyncMode.Compact)`
- `getSyncStatus(walletId)`
- `cancelSync(walletId)`
- `rescan(request)`
- `rescan(walletId, fromHeight)`

Send flow:

- `buildTransaction(request)`
- `buildTransaction(walletId, outputs, fee = null)`
- `buildTransaction(walletId, output, fee = null)`
- `signTransaction(walletId, pending)`
- `broadcastTransaction(signed)`
- `send(walletId, outputs, fee = null)`
- `send(walletId, output, fee = null)`

Change-address selection is automatic. Sapling-only change uses legacy
same-address change before Orchard activation and Sapling internal change after
activation; Orchard spends or outputs use Orchard internal change.

Viewing key and watch-only:

- `exportSaplingViewingKey(walletId)`
- `exportOrchardViewingKey(walletId)`
- `importSaplingViewingKeyAsWatchOnly(request)`
- `importSaplingViewingKeyAsWatchOnly(name, saplingViewingKey, birthdayHeight)`
- `getWatchOnlyCapabilities(walletId)`

Advanced key management:

- `advancedKeyManagement.listKeyGroups(walletId)`
- `advancedKeyManagement.exportKeyGroupKeys(walletId, keyId)`
- `advancedKeyManagement.importSpendingKey(request)`
- `advancedKeyManagement.importSpendingKey(walletId, birthdayHeight, saplingSpendingKey = null, orchardSpendingKey = null)`
- `advancedKeyManagement.exportSeed(walletId, mnemonicLanguage = null)`

## PirateWalletSynchronizer

Public state:

- `status`
- `progress`
- `syncStatus`
- `latestBirthdayHeight`
- `balance`
- `transactions`
- `lastError`
- `snapshot`

Methods:

- `currentSnapshot()`
- `isRunning()`
- `isSyncing()`
- `isComplete()`
- `start()`
- `stop()`
- `refresh()`
- `close()`

Config:

- `PirateWalletSynchronizer.Config`
  - `syncMode`
  - `syncingPollIntervalMs`
  - `syncedPollIntervalMs`
  - `errorPollIntervalMs`
  - `transactionLimit`

Snapshot:

- `PirateWalletSynchronizer.Snapshot`
  - `walletId`
  - `status`
  - `progressPercent`
  - `syncStatus`
  - `latestBirthdayHeight`
  - `balance`
  - `transactions`
  - `updatedAtMillis`
  - `lastError`

## Main public model types

Wallet and sync:

- `BuildInfo`
- `WalletMeta`
- `NetworkType`
- `MnemonicLanguage`
- `MnemonicInspection`
- `SyncMode`
- `SyncStage`
- `SyncStatus`
- `CheckpointInfo`

Requests and transaction types:

- `CreateWalletRequest`
- `RestoreWalletRequest`
- both include optional `mnemonicLanguage`
- `ImportViewingWalletRequest`
- `ImportWatchOnlyWalletRequest`
- `ImportSpendingKeyRequest`
- `TransactionOutput`
- `BuildTransactionRequest`
- `RescanRequest`
- `SyncRequest`
- `PendingTransaction`
- `SignedTransaction`

Balances and addresses:

- `Balance`
- `ShieldedPoolBalances`
- `AddressInfo`
- `AddressBalanceInfo`
- `SpendabilityStatus`

Validation and watch-only:

- `ShieldedAddressType`
- `AddressValidation`
- `ConsensusBranchValidation`
- `WatchOnlyCapabilities`

Key management:

- `KeyTypeInfo`
- `KeyGroupInfo`
- `KeyExportInfo`

Transaction detail:

- `TransactionInfo`
- `TransactionRecipient`
- `TransactionDetails`

## Notes

- The Android SDK keeps high-risk seed and spending-key operations under `advancedKeyManagement`.
- `inspectMnemonic(mnemonic)` is the language-detection helper for higher-level UX.
