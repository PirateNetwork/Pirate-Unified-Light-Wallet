# Troubleshooting

[Previous: Settings and release verification](settings-and-verification.md) | [Guide contents](README.md) | [Next: Advanced use](advanced.md)

Start with these three checks:

1. Confirm the selected wallet at the top of Home.
2. Confirm that synchronisation has reached the current chain height.
3. Confirm that the receiving address belongs to a key loaded by the selected wallet.

## Missing balance or transaction

Complete the following checks in order:

1. Open Activity and remove any filters.
2. Wait for Stashi Wallet to show **Synced**.
3. Compare the destination address and transaction ID with the sending wallet or a block explorer.
4. Confirm that the transaction is confirmed and was not sent to a change address controlled only by the sender.
5. Confirm that the imported seed phrase is correct.
6. If you are moving from another wallet, add higher seed accounts under **Settings > Keys & addresses**.
7. Import any separate spending or viewing key again.
8. Confirm that the wallet birthday height is before the transaction block.
9. In Auto node mode, allow Stashi Wallet to replace an endpoint that is connected but not serving current blocks.
10. Rescan from a height before the missing transaction.

## Stuck on Preparing sync

1. Wait several minutes after an update, local data upgrade, key import, or seed-account addition.
2. Confirm that the device clock is correct and that storage is available.
3. Try another transport.
4. Try another light server manually.
5. Return to Auto and allow failover.
6. Restart the application.
7. Enable debug logging and reproduce the delay.

If a server is reachable but its block height is stale, Auto endpoint selection should treat it as degraded and move to another endpoint.

## Fiat value is blank

The fiat value is for information only. ARRR funds and transaction construction do not depend on it.

1. Confirm that **Allow non-lightserver API calls** is enabled under **Settings > Privacy and Network > Outbound API Calls**.
2. Confirm that **Live Price Feeds** is enabled.
3. Check the internet connection and selected transport.
4. Leave the Home screen open while the price refreshes.
5. Change the selected fiat currency and change it back if the preference appears out of date.

Stashi Wallet tries CoinGecko first and uses CoinPaprika and CoinMarketCap as alternatives. All providers can be temporarily unavailable or rate-limited.

## Insufficient funds when the total balance is larger

1. Include the network fee in the required total.
2. Wait for incoming funds to confirm.
3. Select **Auto (all keys)** in the source selector.
4. If you select a specific key, compare its spendable balance with the amount.
5. Wait for any transaction using the same notes to finish or fail.
6. If the wallet has many small notes, enable auto consolidation or send a smaller amount first.

The wallet-wide balance can include notes from several keys. A selected key cannot spend notes owned by another key.

## Verify build cannot download files

1. Open **Settings > Privacy and Network > Outbound API Calls**.
2. Enable **Allow non-lightserver API calls** and **Verify Build GitHub Checks**.
3. Check the selected transport. GitHub must be reachable through it.
4. Try Tor, SOCKS5, or Direct if I2P or the selected transport cannot reach GitHub.
5. Select **Verify now** again.

A download error is not a mismatch. If online verification remains unavailable, use the signed files from the release page and follow [Verify the downloaded release files](settings-and-verification.md#verify-the-downloaded-release-files).

## AppImage does not open

1. Confirm that you downloaded the AppImage for x86_64 Linux.
2. Make it executable in the file manager, or run the following command:

```bash
chmod +x Stashi-Wallet-linux-x86_64.AppImage
```

3. Start it from a terminal to view any error:

```bash
./Stashi-Wallet-linux-x86_64.AppImage
```

4. If the distribution blocks FUSE, use the AppImage extract-and-run support:

```bash
./Stashi-Wallet-linux-x86_64.AppImage --appimage-extract-and-run
```

5. Use the DEB or Flatpak package if either format is more suitable for the distribution.

Verify the checksum and signature before changing permissions or running the file.

## Interface looks too large on a laptop

Stashi Wallet follows the operating system display and text scaling. It also changes to a more compact desktop layout when the available window height is limited.

1. Install the latest Stashi Wallet version.
2. Maximise the window or make it taller if the desktop allows it.
3. Check the operating system display scale and accessibility text size. Retain a larger setting if you need it for readability.
4. Scroll the page if controls do not fit vertically. The compact layout does not remove wallet functions.

Do not reduce an accessibility text setting only to make Stashi Wallet match a screenshot. The layout is designed to keep scaled text readable.

## Send failed

1. Check the recipient address and amount.
2. Check the confirmed spendable funds and fee.
3. Confirm that Stashi Wallet shows **Synced**.
4. Confirm that the light server is healthy.
5. If the error occurred before broadcast, correct the problem and try again.
6. If the broadcast status is uncertain, check Activity and the transaction ID before sending again.

Do not create a duplicate payment until you know whether the first transaction was broadcast.

## Information for a support report

Include the following information:

- Stashi Wallet version
- Operating system and version
- Package type, such as DMG, AppImage, DEB, Flatpak, installer, or APK
- Selected network transport and whether node selection is Auto or Manual
- Current wallet height and target height
- Exact error text
- Time of the problem, including the time zone
- Transaction ID, when relevant
- Debug log captured immediately after reproducing the problem

Do not include seed words, spending keys, local passphrases, or unredacted personal information.
