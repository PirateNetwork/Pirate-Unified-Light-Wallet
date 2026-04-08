# Pirate Wallet Android SDK

`bindings/android-sdk` exposes the unified Pirate wallet service to Kotlin callers.

## Mnemonic language support

The SDK now supports the same BIP39 seed phrase languages as the Flutter app and native service:

- `English`
- `ChineseSimplified`
- `ChineseTraditional`
- `French`
- `Italian`
- `Japanese`
- `Korean`
- `Spanish`

Use `MnemonicLanguage` anywhere you want to generate, validate, inspect, create, restore, or export a mnemonic in a specific language.

## Public mnemonic APIs

### Create and restore wallets

`CreateWalletRequest` and `RestoreWalletRequest` now accept:

- `mnemonicLanguage: MnemonicLanguage?`

Examples:

```kotlin
val walletId = sdk.createWallet(
    CreateWalletRequest(
        name = "Spanish Wallet",
        birthdayHeight = 1800000,
        mnemonicLanguage = MnemonicLanguage.Spanish,
    )
)

val restored = sdk.restoreWallet(
    RestoreWalletRequest(
        name = "Recovered Wallet",
        mnemonic = phrase,
        birthdayHeight = 1800000,
        mnemonicLanguage = MnemonicLanguage.Japanese,
    )
)
```

If `mnemonicLanguage` is omitted on restore, the backend will autodetect the mnemonic language when possible.

### Generate, validate, inspect

Available methods:

- `generateMnemonic(wordCount: Int? = null, mnemonicLanguage: MnemonicLanguage? = null): String`
- `validateMnemonic(mnemonic: String, mnemonicLanguage: MnemonicLanguage? = null): Boolean`
- `inspectMnemonic(mnemonic: String): MnemonicInspection`

`inspectMnemonic` returns:

- `isValid`
- `detectedLanguage`
- `ambiguousLanguages`
- `wordCount`

Example:

```kotlin
val phrase = sdk.generateMnemonic(
    wordCount = 24,
    mnemonicLanguage = MnemonicLanguage.French,
)

val inspection = sdk.inspectMnemonic(phrase)
val isValid = sdk.validateMnemonic(
    phrase,
    mnemonicLanguage = inspection.detectedLanguage,
)
```

### Export seed phrase

Advanced key management now supports display-language selection:

- `sdk.advancedKeyManagement.exportSeed(walletId, mnemonicLanguage = null): String`

Behavior:

- if `mnemonicLanguage` is omitted, export uses the wallet's original stored mnemonic language
- if `mnemonicLanguage` is provided, the service re-renders the same seed entropy in that language for display/export

Example:

```kotlin
val englishSeed = sdk.advancedKeyManagement.exportSeed(walletId)
val spanishSeed = sdk.advancedKeyManagement.exportSeed(
    walletId,
    mnemonicLanguage = MnemonicLanguage.Spanish,
)
```

## Compatibility notes

- existing English-only wallets are migrated to stored `english` seed language automatically
- restore is cross-compatible with non-English BIP39 phrases from other Pirate wallet surfaces
- the underlying wallet seed remains the same across language renderings; language only changes phrase encoding and display
