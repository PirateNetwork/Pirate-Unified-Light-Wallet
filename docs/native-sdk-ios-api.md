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
- `createWallet(name:birthdayHeight:mnemonicLanguage:)`
- `restoreWallet(request:)`
- `restoreWallet(name:mnemonic:birthdayHeight:mnemonicLanguage:)`
- `importViewingWallet(request:)`
- `importViewingWallet(name:saplingViewingKey:ironwoodViewingKey:birthdayHeight:)`
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
- `createWalletAsync(name:birthdayHeight:mnemonicLanguage:)`
- `restoreWalletAsync(request:)`
- `restoreWalletAsync(name:mnemonic:birthdayHeight:mnemonicLanguage:)`
- `importViewingWalletAsync(request:)`
- `importViewingWalletAsync(name:saplingViewingKey:ironwoodViewingKey:birthdayHeight:)`
- `switchWalletAsync(walletId:)`
- `renameWalletAsync(walletId:newName:)`
- `deleteWalletAsync(walletId:)`
- `setWalletBirthdayHeightAsync(walletId:birthdayHeight:)`
- `getLatestBirthdayHeightAsync(walletId:)`

The active wallet is a backend wallet-registry selection for the currently
selected/default wallet. Most SDK methods accept an explicit `walletId`, so
third-party apps can manage wallet selection directly. `switchWallet(walletId:)`
persists the active-wallet selection and cancels sync for the previously active
wallet.

Mnemonic and formatting:

- `generateMnemonic(wordCount:mnemonicLanguage:)`
- `validateMnemonic(_:mnemonicLanguage:)`
- `inspectMnemonic(_:)`
- `getNetworkInfo()`
- `formatAmount(_:)`
- `parseAmount(_:)`
- `generateMnemonicAsync(wordCount:mnemonicLanguage:)`
- `validateMnemonicAsync(_:mnemonicLanguage:)`
- `inspectMnemonicAsync(_:)`
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

Address access is split into explicit shielded receive-address APIs.
`getCurrentAddress` returns the current external receive address without rotating it,
`getNextAddress` rotates to and returns the next external receive address,
`listAddresses` returns generated external receive addresses, and
`listAddressBalances` returns per-address balance entries. Without a key ID it
returns external receive-address rows only; with a key ID it also includes
internal change-address rows for that key group. Internal change is always
included in `getBalance`, which should be used for wallet totals. Newly generated
addresses use Sapling before Ironwood activation and Ironwood after activation.
At activation, both current- and next-address calls select Ironwood; existing
Sapling addresses remain valid and continue to appear in address lists.

Balances and transaction inspection:

- `getBalance(walletId:)`
- `getShieldedPoolBalances(walletId:)`
- `getSpendabilityStatus(walletId:)`
- `listTransactions(walletId:limit:)`
- `fetchTransactionMemo(walletId:txId:outputIndex:)`
- `getTransactionDetails(walletId:txId:)`
- `exportPaymentDisclosures(walletId:txId:)`
- `exportSaplingPaymentDisclosure(walletId:txId:outputIndex:)`
- `exportIronwoodPaymentDisclosure(walletId:txId:actionIndex:)`
- `verifyPaymentDisclosure(walletId:disclosure:)`
- `getFeeInfo()`
- `getBalanceAsync(walletId:)`
- `getShieldedPoolBalancesAsync(walletId:)`
- `getSpendabilityStatusAsync(walletId:)`
- `listTransactionsAsync(walletId:limit:)`
- `fetchTransactionMemoAsync(walletId:txId:outputIndex:)`
- `getTransactionDetailsAsync(walletId:txId:)`
- `exportPaymentDisclosuresAsync(walletId:txId:)`
- `exportSaplingPaymentDisclosureAsync(walletId:txId:outputIndex:)`
- `exportIronwoodPaymentDisclosureAsync(walletId:txId:actionIndex:)`
- `verifyPaymentDisclosureAsync(walletId:disclosure:)`
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

Sync state is tracked per wallet in the backend. Apps can sync multiple wallets
concurrently by starting sync with explicit wallet IDs and separate
synchronizers. Sync tasks share device, network, and lightwalletd resources, and
the sync engine uses a shared compact-block cache per endpoint so later syncs
for another wallet on the same endpoint can reuse fetched block ranges.

Send flow:

- `buildTransaction(request:)`
- `buildTransaction(walletId:outputs:fee:)`
- `buildTransaction(walletId:output:fee:)`
- `signTransaction(walletId:pending:)`
- `broadcastTransaction(walletId:signed:)`
- `broadcastTransaction(_:)` (deprecated compatibility overload)
- `send(walletId:outputs:fee:)`
- `send(walletId:output:fee:)`
- `buildTransactionAsync(request:)`
- `buildTransactionAsync(walletId:outputs:fee:)`
- `buildTransactionAsync(walletId:output:fee:)`
- `signTransactionAsync(walletId:pending:)`
- `broadcastTransactionAsync(walletId:signed:)`
- `broadcastTransactionAsync(_:)` (deprecated compatibility overload)
- `sendAsync(walletId:outputs:fee:)`
- `sendAsync(walletId:output:fee:)`

Use the wallet-scoped broadcast overloads so endpoint selection, failover,
repair state, and accepted-transaction persistence stay attached to the wallet
that produced the signed transaction. The one-argument overloads remain for
source compatibility and use the transaction's in-memory origin context when
available.

Change-address selection is automatic. Sapling-only change uses legacy
same-address change before Ironwood activation and Sapling internal change after
activation; Ironwood spends or outputs use Ironwood internal change.

Viewing key and watch-only:

- `exportSaplingViewingKey(walletId:)`
- `exportIronwoodViewingKey(walletId:)`
- `importSaplingViewingKeyAsWatchOnly(request:)`
- `importSaplingViewingKeyAsWatchOnly(name:saplingViewingKey:birthdayHeight:)`
- `getWatchOnlyCapabilities(walletId:)`
- `exportSaplingViewingKeyAsync(walletId:)`
- `exportIronwoodViewingKeyAsync(walletId:)`
- `importSaplingViewingKeyAsWatchOnlyAsync(request:)`
- `importSaplingViewingKeyAsWatchOnlyAsync(name:saplingViewingKey:birthdayHeight:)`
- `getWatchOnlyCapabilitiesAsync(walletId:)`

Advanced key management:

- `advancedKeyManagement.listKeyGroups(walletId:)`
- `advancedKeyManagement.exportKeyGroupKeys(walletId:keyId:)`
- `advancedKeyManagement.importSpendingKey(request:)`
- `advancedKeyManagement.importSpendingKey(walletId:birthdayHeight:saplingSpendingKey:ironwoodSpendingKey:)`
- `advancedKeyManagement.exportSeed(walletId:mnemonicLanguage:)`
- `advancedKeyManagement.listKeyGroupsAsync(walletId:)`
- `advancedKeyManagement.exportKeyGroupKeysAsync(walletId:keyId:)`
- `advancedKeyManagement.importSpendingKeyAsync(request:)`
- `advancedKeyManagement.importSpendingKeyAsync(walletId:birthdayHeight:saplingSpendingKey:ironwoodSpendingKey:)`
- `advancedKeyManagement.exportSeedAsync(walletId:mnemonicLanguage:)`

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
- `PaymentDisclosure`
- `PaymentDisclosureVerification`

## Notes

- The iOS SDK keeps high-risk seed and spending-key operations under `advancedKeyManagement`.
