# piratewallet-cli

`piratewallet-cli` is the general command-line interface for the unified Pirate wallet backend.

## Mnemonic language support

The CLI now accepts explicit BIP39 seed phrase language selection for create, restore, generate, validate, and seed export flows.

Supported values:

- `english`
- `chinese-simplified`
- `chinese-traditional`
- `french`
- `italian`
- `japanese`
- `korean`
- `spanish`

These values map to the shared wallet service `MnemonicLanguage` enum.

## Updated commands

### Generate mnemonic

```bash
piratewallet-cli generate-mnemonic --word-count 24 --mnemonic-language spanish
```

### Validate mnemonic

```bash
piratewallet-cli validate-mnemonic "..." --mnemonic-language japanese
```

If `--mnemonic-language` is omitted, validation uses backend autodetect.

### Create wallet

```bash
piratewallet-cli wallet create "My Wallet" --birthday 1800000 --mnemonic-language french
```

### Restore wallet

```bash
piratewallet-cli wallet restore "Recovered Wallet" "..." --birthday 1800000 --mnemonic-language korean
```

If `--mnemonic-language` is omitted on restore, the backend attempts autodetection first.

### Export seed

```bash
piratewallet-cli seed --wallet-id <wallet-id> --mnemonic-language italian
```

Behavior:

- no language flag: export uses the wallet's original stored mnemonic language
- language flag present: export re-renders the same seed entropy in the requested language

## Compatibility notes

- existing English-only wallets are migrated automatically to stored `english`
- cross-wallet recovery is now explicit and deterministic for non-English BIP39 phrases
- changing export language does not change the wallet or its derived keys
