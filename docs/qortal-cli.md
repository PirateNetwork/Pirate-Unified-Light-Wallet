# Qortal CLI Adapter

`pirate-qortal-cli` is a command-line test adapter for the Qortal response
schema. Qortal Core itself embeds `pirate-qortal-jni`; see
`docs/qortal-handoff.md` for the native artifacts, storage migration, Java
changes, and command contract.

## Build

```bash
cd crates
cargo build --release --locked -p pirate-qortal-cli
```

## Commands

Every command accepts `--wallet-id <WALLET_ID>` and uses the active wallet when
the flag is omitted.

```bash
pirate-qortal-cli syncstatus --wallet-id <WALLET_ID>
pirate-qortal-cli balance --wallet-id <WALLET_ID>
pirate-qortal-cli list --wallet-id <WALLET_ID> --limit 20
pirate-qortal-cli sendp2sh '<REQUEST_JSON>' --wallet-id <WALLET_ID>
pirate-qortal-cli redeemp2sh '<REQUEST_JSON>' --wallet-id <WALLET_ID>
```

Use `--format json` for compact JSON or `--format pretty` for formatted output.
The CLI and JNI adapters both call the Qortal operations in
`pirate-wallet-service`, so their balance, history, sync-status, and P2SH
behavior do not drift apart.
