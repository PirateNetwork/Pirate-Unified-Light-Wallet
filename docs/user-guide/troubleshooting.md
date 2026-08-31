# Troubleshooting

[Previous: Settings and verification](settings-and-verification.md) | [Guide contents](README.md) | [Next: Advanced use](advanced.md)

Start with three checks:

1. Confirm the selected wallet at the top of Home.
2. Confirm the scan has reached the current chain height.
3. Confirm the receiving address belongs to a key that this wallet has loaded.

## Missing balance or transaction

Work through this list in order:

1. Open Activity and remove any filters.
2. Wait for the wallet to report synced.
3. Compare the destination address and transaction ID with the sending wallet or block explorer.
4. Confirm the transaction is confirmed and was not sent to a change address owned only by the sender.
5. Confirm the restored recovery phrase is the correct phrase.
6. If migrating, add higher seed accounts under **Settings > Keys & addresses**.
7. Reimport any separately imported spending or viewing key.
8. Check that the wallet birthday is before the transaction block.
9. In Auto node mode, let the wallet replace an endpoint that is connected but not serving current blocks.
10. Rescan from before the missing transaction.

A transaction ID proves that a transaction exists. It does not prove that the active wallet controls its destination. Never publish a viewing key to prove ownership.

## Stuck on Preparing sync

1. Wait several minutes after an upgrade, database migration, new key import, or seed-account addition.
2. Check that the computer's clock is correct and that storage is not full.
3. Try another transport.
4. Try another light server manually.
5. Return to Auto and allow failover.
6. Restart the app once.
7. Enable debug logging and reproduce the stall.

If one server is reachable but its block height is stale, a current Auto endpoint pool should treat it as degraded and move on.

## Fiat value is blank

The fiat figure is informational. ARRR funds and transaction construction do not depend on it.

1. Confirm **Settings > Privacy and Network > Outbound API Calls** is enabled.
2. Confirm **Live Price Feeds** is enabled.
3. Check the internet connection and selected transport.
4. Leave Home open briefly while the price refreshes.
5. Change the selected fiat currency and change it back if the preference appears stale.

The wallet tries CoinGecko first and has CoinPaprika and CoinMarketCap backups. All providers can still be temporarily unavailable or rate limited.

## Insufficient funds when the total balance is larger

1. Include the network fee in the required total.
2. Wait for incoming funds to confirm.
3. In the source selector, choose **Auto (all keys)**.
4. If using a specific key, compare its spendable balance with the amount.
5. Wait for any transaction already using the same notes to finish or fail.
6. If the wallet has many small notes, enable auto consolidation or send a smaller amount first.

The wallet-wide balance can include notes from several keys. A manually selected key cannot spend notes owned by another key.

## Verify Build cannot download files

1. Open **Settings > Privacy and Network > Outbound API Calls**.
2. Enable the master switch and **Verify Build GitHub Checks**.
3. Check the selected transport. GitHub must be reachable through it.
4. Try Tor, SOCKS5, or Direct if the current route blocks GitHub.
5. Select **Verify now** again.

A download error is not a mismatch. If online verification remains unavailable, use the signed files from the release page and follow [Verify the downloaded release files](settings-and-verification.md#verify-the-downloaded-release-files).

## AppImage does not open

1. Confirm you downloaded the AppImage for x86_64 Linux.
2. Make it executable in the file manager, or run:

```bash
chmod +x Stashi-Wallet-linux-x86_64.AppImage
```

3. Start it from a terminal to see an error:

```bash
./Stashi-Wallet-linux-x86_64.AppImage
```

4. If the distribution blocks FUSE, use the AppImage's extract-and-run support:

```bash
./Stashi-Wallet-linux-x86_64.AppImage --appimage-extract-and-run
```

5. Prefer the DEB or Flatpak package if that fits the distribution better.

Verify the checksum and signature before changing permissions or running the file.

## Interface looks too large on a laptop

Stashi Wallet follows the operating system's display and text scaling. It also switches to a more compact desktop layout when the usable window is short.

1. Install the latest wallet version.
2. Maximize the window or make it slightly taller if the desktop allows it.
3. Check the operating system display scale and accessibility text size. Keep a larger setting if you need it for readability.
4. Scroll the page if controls do not fit vertically. No wallet function is removed in the compact layout.

Do not reduce an accessibility text setting only to make the wallet match a screenshot. The layout is designed to keep scaled text readable.

## Text or panels flicker on an Intel Mac

Install the latest release and restart the app. Current macOS builds use the stable renderer on Intel and Apple Silicon Macs. Rendering corruption does not change wallet keys or blockchain data.

## macOS says a required entitlement is missing

This can affect Keychain-backed preferences or biometric features on an incorrectly packaged build.

1. Confirm the app came from the official signed and notarized DMG.
2. Move **Stashi Wallet.app** into Applications before opening it.
3. Do not run it directly from the mounted DMG.
4. Check that Keychain is unlocked and the user session has a login keychain.
5. Install the latest wallet release.
6. Use the app passphrase if biometric secure storage is unavailable.

If the official current release still reports the entitlement error, save the exact error and macOS version for support.

## Send failed

1. Check the recipient address and amount.
2. Check confirmed spendable funds and the fee.
3. Confirm the wallet is synced.
4. Confirm the light server is healthy.
5. If the error occurred before broadcast, correct it and retry once.
6. If broadcast status is uncertain, check Activity and the transaction ID before sending again.

Do not create a duplicate payment until you know whether the first transaction was broadcast.

## What to include in a support report

- Stashi Wallet version.
- Operating system and version.
- Package type, such as DMG, AppImage, DEB, Flatpak, installer, or APK.
- Selected network transport and whether node selection is Auto or Manual.
- Current wallet height and target height.
- Exact error text.
- The time the problem occurred, including time zone.
- Transaction ID when relevant.
- A debug log captured immediately after reproducing the issue.

Do not include recovery words, spending keys, app passphrases, or unredacted personal information.
