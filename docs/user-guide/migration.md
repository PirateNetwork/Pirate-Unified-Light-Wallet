# Move from Treasure Chest or Pirate Wallet Lite

[Previous: Keys and accounts](keys-and-accounts.md) | [Guide contents](README.md) | [Next: Network and sync](network-and-sync.md)

Do not remove the old wallet until the Stashi Wallet balance, transaction history, and receiving keys have been checked.

## Before you begin

1. Open the old wallet on a trusted device.
2. Confirm that you have the correct recovery phrase in the correct order.
3. Record the approximate date or block height of the wallet's first transaction.
4. Record whether the old wallet used multiple accounts under the phrase.
5. Record whether it had any separately imported Sapling spending keys or viewing keys.
6. Update the old wallet's sync and let it finish so you have a useful comparison.

## Restore the phrase

1. Install Stashi Wallet from the official release.
2. Select **Get started > Import existing wallet**.
3. Enter the 24 recovery words.
4. Choose the phrase language if needed.
5. Set a local app passphrase and optional biometrics.
6. Enter a birthday height from before the oldest expected transaction.
7. Finish setup.
8. Keep the app open until the historical scan completes.
9. Compare the balance and Activity page with the old wallet.

The standard seed account is account 0. During recovery, Stashi Wallet also checks the five legacy Sapling lookahead accounts, accounts 1 through 5. Unused lookahead accounts are not kept after the discovery scan. This keeps later syncs efficient.

## Find funds in higher seed accounts

If the old wallet used account 6 or higher, or if its account layout is uncertain:

1. Open **Settings > Keys & addresses**.
2. Find **Seed accounts**.
3. Read the **Next seed account** number shown by the wallet.
4. Select **Add 5 accounts** to extend the search in a small batch, or **Add next account** when you know the exact next index.
5. Confirm the account range.
6. Wait for the scan started by the wallet to finish.
7. Check the balance and Activity page.
8. Repeat with another batch only if the expected history is still missing.

| Phone | Desktop |
|---|---|
| ![Seed account controls on a phone](images/keys-phone.png) | ![Seed account controls on desktop](images/keys-desktop.png) |

The manually added account sequence is saved. Those accounts remain available even if the first scan finds no notes. Each manually added seed account includes Sapling and Ironwood support where available.

## Restore separately imported keys

A recovery phrase cannot recreate a private key that was imported separately into Treasure Chest or Pirate Wallet Lite.

For each separately imported key:

1. Open **Settings > Keys & addresses**.
2. Choose **Spending Key** or **Viewing Key** as appropriate.
3. Enter the key and a birthday height before its first use.
4. Confirm the import.
5. Wait for the automatic rescan.
6. Check the key card, balance, and Activity page.

Do not use **Add next account** for an imported key. Seed accounts and imported keys are different recovery paths.

## If the old wallet shows more history

Check these items in order:

1. The Stashi Wallet scan has reached the current block height.
2. The phrase is the exact phrase used by the destination address.
3. The birthday is earlier than the missing transaction.
4. The required higher seed accounts have been added.
5. Any separately imported spending or viewing keys have been imported again.
6. Activity is not filtered to a different wallet or key.
7. Auto node mode has moved to an endpoint that is serving current block data.

Then run **Settings > Advanced > Rescan blockchain** from a suitable height. If the transaction is still absent, follow [Missing balance or transaction](troubleshooting.md#missing-balance-or-transaction).

## Finish the migration safely

Keep the old wallet until all of the following are true:

- The expected confirmed balance is present.
- Important incoming and outgoing transactions are visible.
- You have identified any imported keys that still hold funds.
- You have tested a small receive and send if practical.
- The Stashi Wallet recovery phrase backup has been verified offline.
