# iOS SDK API Reference

This page lists the public iOS SDK surface in:

- `bindings/ios-sdk/Sources/PirateWalletSDK/PirateWalletSDK.swift`
- `bindings/ios-sdk/Sources/PirateWalletSDK/PirateWalletSDKModels.swift`
- `bindings/ios-sdk/Sources/PirateWalletSDK/PirateWalletSynchronizer.swift`

For non-blocking integration, the typed SDK also exposes async counterparts with the same base name plus an `Async` suffix. The async surface is the preferred path for app code.

## Main entry points

- `PirateWalletSDK`
- `PirateWalletSynchronizer`
- `PirateWalletAdvancedKeyManagement`

## PirateWalletSDK

Core:

- `invoke(requestJson:pretty:)`
- `invokeAsync(requestJson:pretty:)`
- `createSynchronizer(walletId:config:)`
- `buildInfoJson(pretty:)`
- `buildInfo()`
- `buildInfoJsonAsync(pretty:)`
- `buildInfoAsync()`

Wallet lifecycle:

- `walletRegistryExists()`
- `listWallets()`
- `getActiveWalletId()`
- `getActiveWallet()`
- `getWallet(walletId:)`
- `createWallet(request:)`
- `createWallet(name:birthdayHeight:)`
- `restoreWallet(request:)`
- `restoreWallet(name:mnemonic:birthdayHeight:)`
- `importViewingWallet(request:)`
- `importViewingWallet(name:saplingViewingKey:orchardViewingKey:birthdayHeight:)`
- `switchWallet(walletId:)`
- `renameWallet(walletId:newName:)`
- `deleteWallet(walletId:)`
- `setWalletBirthdayHeight(walletId:birthdayHeight:)`
- `getLatestBirthdayHeight(walletId:)`
- `walletRegistryExistsAsync()`
- `listWalletsAsync()`
- `getActiveWalletIdAsync()`
- `getActiveWalletAsync()`
- `getWalletAsync(walletId:)`
- `createWalletAsync(request:)`
- `createWalletAsync(name:birthdayHeight:)`
- `restoreWalletAsync(request:)`
- `restoreWalletAsync(name:mnemonic:birthdayHeight:)`
- `importViewingWalletAsync(request:)`
- `importViewingWalletAsync(name:saplingViewingKey:orchardViewingKey:birthdayHeight:)`
- `switchWalletAsync(walletId:)`
- `renameWalletAsync(walletId:newName:)`
- `deleteWalletAsync(walletId:)`
- `setWalletBirthdayHeightAsync(walletId:birthdayHeight:)`
- `getLatestBirthdayHeightAsync(walletId:)`

Mnemonic and formatting:

- `generateMnemonic(wordCount:)`
- `validateMnemonic(_:)`
- `getNetworkInfo()`
- `formatAmount(_:)`
- `parseAmount(_:)`
- `generateMnemonicAsync(wordCount:)`
- `validateMnemonicAsync(_:)`
- `getNetworkInfoAsync()`
- `formatAmountAsync(_:)`
- `parseAmountAsync(_:)`

Validation:

- `isValidShieldedAddr(_:)`
- `validateAddress(_:)`
- `validateConsensusBranch(walletId:)`
- `isValidShieldedAddrAsync(_:)`
- `validateAddressAsync(_:)`
- `validateConsensusBranchAsync(walletId:)`

Addresses:

- `getCurrentReceiveAddress(walletId:)`
- `getCurrentAddress(walletId:)`
- `getNextReceiveAddress(walletId:)`
- `getNextAddress(walletId:)`
- `listAddresses(walletId:)`
- `listAddressBalances(walletId:keyId:)`
- `getCurrentReceiveAddressAsync(walletId:)`
- `getCurrentAddressAsync(walletId:)`
- `getNextReceiveAddressAsync(walletId:)`
- `getNextAddressAsync(walletId:)`
- `listAddressesAsync(walletId:)`
- `listAddressBalancesAsync(walletId:keyId:)`

Balances and transaction inspection:

- `getBalance(walletId:)`
- `getShieldedPoolBalances(walletId:)`
- `getSpendabilityStatus(walletId:)`
- `listTransactions(walletId:limit:)`
- `fetchTransactionMemo(walletId:txId:outputIndex:)`
- `getTransactionDetails(walletId:txId:)`
- `getFeeInfo()`
- `getBalanceAsync(walletId:)`
- `getShieldedPoolBalancesAsync(walletId:)`
- `getSpendabilityStatusAsync(walletId:)`
- `listTransactionsAsync(walletId:limit:)`
- `fetchTransactionMemoAsync(walletId:txId:outputIndex:)`
- `getTransactionDetailsAsync(walletId:txId:)`
- `getFeeInfoAsync()`

Sync:

- `startSync(request:)`
- `startSync(walletId:mode:)`
- `getSyncStatus(walletId:)`
- `cancelSync(walletId:)`
- `rescan(request:)`
- `rescan(walletId:fromHeight:)`
- `startSyncAsync(request:)`
- `startSyncAsync(walletId:mode:)`
- `getSyncStatusAsync(walletId:)`
- `cancelSyncAsync(walletId:)`
- `rescanAsync(request:)`
- `rescanAsync(walletId:fromHeight:)`

Send flow:

- `buildTransaction(request:)`
- `buildTransaction(walletId:outputs:fee:)`
- `buildTransaction(walletId:output:fee:)`
- `signTransaction(walletId:pending:)`
- `broadcastTransaction(_:)`
- `send(walletId:outputs:fee:)`
- `send(walletId:output:fee:)`
- `buildTransactionAsync(request:)`
- `buildTransactionAsync(walletId:outputs:fee:)`
- `buildTransactionAsync(walletId:output:fee:)`
- `signTransactionAsync(walletId:pending:)`
- `broadcastTransactionAsync(_:)`
- `sendAsync(walletId:outputs:fee:)`
- `sendAsync(walletId:output:fee:)`

Viewing key and watch-only:

- `exportSaplingViewingKey(walletId:)`
- `exportOrchardViewingKey(walletId:)`
- `importSaplingViewingKeyAsWatchOnly(request:)`
- `importSaplingViewingKeyAsWatchOnly(name:saplingViewingKey:birthdayHeight:)`
- `getWatchOnlyCapabilities(walletId:)`
- `exportSaplingViewingKeyAsync(walletId:)`
- `exportOrchardViewingKeyAsync(walletId:)`
- `importSaplingViewingKeyAsWatchOnlyAsync(request:)`
- `importSaplingViewingKeyAsWatchOnlyAsync(name:saplingViewingKey:birthdayHeight:)`
- `getWatchOnlyCapabilitiesAsync(walletId:)`

Advanced key management:

- `advancedKeyManagement.listKeyGroups(walletId:)`
- `advancedKeyManagement.exportKeyGroupKeys(walletId:keyId:)`
- `advancedKeyManagement.importSpendingKey(request:)`
- `advancedKeyManagement.importSpendingKey(walletId:birthdayHeight:saplingSpendingKey:orchardSpendingKey:)`
- `advancedKeyManagement.exportSeed(walletId:)`
- `advancedKeyManagement.listKeyGroupsAsync(walletId:)`
- `advancedKeyManagement.exportKeyGroupKeysAsync(walletId:keyId:)`
- `advancedKeyManagement.importSpendingKeyAsync(request:)`
- `advancedKeyManagement.importSpendingKeyAsync(walletId:birthdayHeight:saplingSpendingKey:orchardSpendingKey:)`
- `advancedKeyManagement.exportSeedAsync(walletId:)`

## PirateWalletSynchronizer

Published state:

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

`stop()` and `close()` use the same shutdown path. `close()` returns the same `Task<Void, Never>`
shape as `stop()` and also cancels backend sync instead of only stopping local polling.

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

- `PirateWalletSdkError`
- `BuildInfo`
- `WalletMeta`
- `NetworkType`
- `SyncMode`
- `SyncStage`
- `SyncStatus`
- `CheckpointInfo`

Requests and transaction types:

- `CreateWalletRequest`
- `RestoreWalletRequest`
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

- The iOS SDK keeps high-risk seed and spending-key operations under `advancedKeyManagement`.
