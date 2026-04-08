# pirate-qortal-cli

`pirate-qortal-cli` is the Qortal compatibility adapter for the unified Pirate wallet backend.

## Mnemonic language support

The Qortal-specific commands do not create or restore wallets directly, so they do not need mnemonic-language flags.

However, this binary shares the same general wallet-command path from `pirate-cli-core`. That means mnemonic-language-aware wallet commands are available whenever `pirate-qortal-cli` is invoked through the general CLI surface instead of the Qortal-only subcommands.

Supported mnemonic language values:

- `english`
- `chinese-simplified`
- `chinese-traditional`
- `french`
- `italian`
- `japanese`
- `korean`
- `spanish`

## General wallet command examples

### Generate mnemonic

```bash
pirate-qortal-cli generate-mnemonic --word-count 24 --mnemonic-language spanish
```

### Validate mnemonic

```bash
pirate-qortal-cli validate-mnemonic "..." --mnemonic-language japanese
```

### Create wallet

```bash
pirate-qortal-cli wallet create "My Wallet" --birthday 1800000 --mnemonic-language french
```

### Restore wallet

```bash
pirate-qortal-cli wallet restore "Recovered Wallet" "..." --birthday 1800000 --mnemonic-language korean
```

### Export seed

```bash
pirate-qortal-cli seed --wallet-id <wallet-id> --mnemonic-language italian
```

## Qortal-specific commands

These commands are unchanged by mnemonic-language support:

- `syncstatus`
- `balance`
- `list`
- `sendp2sh`
- `redeemp2sh`

## Compatibility notes

- when used through the general wallet surface, this binary is now cross-compatible with non-English BIP39 recovery phrases
- when used through the Qortal-only subcommands, behavior is unchanged
