# Legacy seed account recovery review

The reported bug remained present after v1.2.0. Discovery created temporary
Sapling account candidates, and `list_key_groups` intentionally hid them until
finalization. Finalization was attached to successful termination of the service
sync task. Normal foreground sync follows the tip indefinitely, so a fully
scanned wallet could still hide recovered accounts and reject account additions.

The sync engine now finalizes discovery at the completed tip, after persistence
work is acknowledged. It retains accounts with historical notes (including spent
notes), retires unused candidates, and refreshes the running trial decryptor.
Explicit partial repair ranges and cancelled or incomplete scans do not trigger
this finalization. The service's existing completion finalizer remains idempotent.

Receive now prepares the first address for restored spending groups as well as
imported spending keys. Existing addresses are preserved. Address history retains
key IDs and seed indices, displays account labels, and combines a key-group
filter with search, sorting, and archive selection. Funded address cards place
actions below content on phones to avoid balance overflow.

Keys shows recovered groups before recovery controls and refreshes while open.
Account-addition buttons explain and respect an active scan. Send already loaded
all spendable key groups; removing the discovery visibility blockage makes these
groups available to Auto and individual source selection. Unlabelled restored
groups now have explicit seed-account names in Send.

## Scope and validation

- Automatic discovery still checks seed accounts 1–5, in addition to account 0.
  A restore with account 6 or higher can use Add next account / Add 5 accounts
  after the scan completes. This change does not introduce unlimited discovery.
- 24 Flutter tests pass across Receive, Keys, Send, and the restored-wallet review
  fixture. These cover live key refresh, group identity, address preparation
  without rotation, filtering, source visibility, scan controls, and layouts.
- The sync regression checks incomplete/cancelled scans and live finalization
  through the persistence worker, including retained internal/external decryption
  scopes and idempotence.
- Both storage seed-account tests pass: discovery retention/retirement and
  consecutive, atomic, durable user-added accounts.
- Targeted Flutter analysis reports no issues.
- These are actual Flutter widget captures using deterministic mock bridge data.
  Addresses are deliberately non-payment sample strings. No real wallet or
  network was used, and no live Skull Island seed restore was performed.
- New English copy is registered in the English catalog; other locales use the
  existing fallback until translated.

## Screenshots

1. [Recovered groups on Keys](01-keys.png)
2. [Receive filtered to seed account 1](02-receive-filter.png)
3. [Restored Send sources](03-send-sources.png)
4. [Account recovery during scanning](04-account-recovery.png)

## Reproduce captures

From `app`, set `PIRATE_UI_CAPTURE_DIR` to an existing output directory and
`PIRATE_MATERIAL_ICONS_FONT` to the Flutter SDK's
`bin/cache/artifacts/material_fonts/materialicons-regular.otf`. Then run:

```sh
flutter test --no-pub test/restored_wallet_review_test.dart
```

Without the output variable, the same fixture runs assertions without writing
screenshots. The fixture loads bundled Sora and JetBrains Mono fonts explicitly.
