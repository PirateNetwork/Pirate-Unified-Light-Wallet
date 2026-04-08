# Pirate Wallet iOS SDK

`bindings/ios-sdk` exposes the unified Pirate wallet service to Swift callers.

## Mnemonic language support

The SDK now supports these BIP39 seed phrase languages:

- `english`
- `chineseSimplified`
- `chineseTraditional`
- `french`
- `italian`
- `japanese`
- `korean`
- `spanish`

The public enum is `MnemonicLanguage`.

## Public mnemonic APIs

### Create and restore wallets

`CreateWalletRequest` and `RestoreWalletRequest` now include:

- `mnemonicLanguage: MnemonicLanguage?`

Examples:

```swift
let walletId = try sdk.createWallet(
    request: CreateWalletRequest(
        name: "Spanish Wallet",
        birthdayHeight: 1_800_000,
        mnemonicLanguage: .spanish
    )
)

let restored = try sdk.restoreWallet(
    request: RestoreWalletRequest(
        name: "Recovered Wallet",
        mnemonic: phrase,
        birthdayHeight: 1_800_000,
        mnemonicLanguage: .japanese
    )
)
```

If `mnemonicLanguage` is omitted during restore, the backend attempts autodetection.

### Generate, validate, inspect

Available sync APIs:

- `generateMnemonic(wordCount: Int? = nil, mnemonicLanguage: MnemonicLanguage? = nil) throws -> String`
- `validateMnemonic(_ mnemonic: String, mnemonicLanguage: MnemonicLanguage? = nil) throws -> Bool`
- `inspectMnemonic(_ mnemonic: String) throws -> MnemonicInspection`

Available async APIs:

- `generateMnemonicAsync(wordCount: Int? = nil, mnemonicLanguage: MnemonicLanguage? = nil) async throws -> String`
- `validateMnemonicAsync(_ mnemonic: String, mnemonicLanguage: MnemonicLanguage? = nil) async throws -> Bool`
- `inspectMnemonicAsync(_ mnemonic: String) async throws -> MnemonicInspection`

`MnemonicInspection` contains:

- `isValid`
- `detectedLanguage`
- `ambiguousLanguages`
- `wordCount`

### Export seed phrase

Advanced key management now supports display-language selection:

- `exportSeed(walletId: String, mnemonicLanguage: MnemonicLanguage? = nil) throws -> String`
- `exportSeedAsync(walletId: String, mnemonicLanguage: MnemonicLanguage? = nil) async throws -> String`

Behavior:

- omitted language uses the wallet's original stored mnemonic language
- provided language re-renders the same underlying seed in that language for export/display

Example:

```swift
let native = try sdk.advancedKeyManagement.exportSeed(walletId: walletId)
let italian = try sdk.advancedKeyManagement.exportSeed(
    walletId: walletId,
    mnemonicLanguage: .italian
)
```

## Compatibility notes

- existing English-only wallets are migrated to stored `english` seed language automatically
- non-English BIP39 phrases created elsewhere can be restored through the same API
- mnemonic display language does not change wallet identity or derived spending keys
