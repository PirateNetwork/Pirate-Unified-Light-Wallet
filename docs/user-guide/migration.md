# Move from Treasure Chest or Pirate Wallet Lite

[Previous: Keys and accounts](keys-and-accounts.md) | [Guide contents](README.md) | [Next: Network and synchronisation](network-and-sync.md)

Moving a wallet means importing its seed phrase and any separate keys into Stashi Wallet, then confirming that the expected balance and transaction history are present. Do not remove the previous wallet until you have completed these checks.

## Before you begin

1. Open the previous wallet on a trusted device.
2. Confirm that you have the correct seed phrase in the correct order.
3. Record the approximate date or block height of the wallet's first transaction.
4. Record whether the previous wallet used multiple accounts from the seed phrase.
5. Record whether it contained separately imported Sapling spending keys or viewing keys.
6. Update and synchronise the previous wallet so that you have a useful comparison.

## Import the seed phrase

1. Install Stashi Wallet from the official release.
2. Select **Get started > Import existing wallet**.
3. Select the seed phrase language if required.
4. Enter the 24 words of the seed phrase in order.
5. Set a local passphrase and optional biometrics.
6. Enter a birthday height from before the oldest expected transaction.
7. Finish the setup.
8. Keep the application open until the historical scan is complete.
9. Compare the balance and Activity page with the previous wallet.

The standard seed account is account 0. During recovery, Stashi Wallet also checks the five legacy Sapling lookahead accounts, accounts 1 through 5. Unused lookahead accounts are not retained after the discovery scan. This keeps later synchronisation efficient.

## Find funds in higher seed accounts

If the previous wallet used account 6 or higher, or if its account layout is uncertain:

1. Open **Settings > Keys & addresses**.
2. Find **Seed accounts**.
3. Read the **Next seed account** number shown by Stashi Wallet.
4. Select **Add 5 accounts** to extend the search in a small group, or select **Add next account** when you know the exact next index.
5. Confirm the account range.
6. Allow the scan started by Stashi Wallet to finish.
7. Check the balance and Activity page.
8. Add another group only if the expected history is still missing.

| Mobile | Desktop |
|---|---|
| ![Seed account controls on a mobile device](images/keys-phone.png) | ![Seed account controls on a desktop computer](images/keys-desktop.png) |

The manually added account sequence is saved. These accounts remain available even if the first scan finds no notes. Each manually added seed account includes Sapling and Ironwood support where available.

## Import separate keys

A seed phrase cannot recreate a private key that was imported separately into Treasure Chest or Pirate Wallet Lite.

For each separately imported key:

1. Open **Settings > Keys & addresses**.
2. Select **Spending Key** or **Viewing Key**, as appropriate.
3. Enter the key and a birthday height from before its first use.
4. Confirm the import.
5. Allow the automatic rescan to finish.
6. Check the key entry, balance, and Activity page.

Do not use **Add next account** for an imported key. Seed accounts and imported keys use different recovery methods.

## If the previous wallet shows more history

Check the following points in order:

1. Confirm that Stashi Wallet has reached the current block height.
2. Confirm that the seed phrase controls the destination address.
3. Confirm that the birthday height is earlier than the missing transaction.
4. Add any required higher seed accounts.
5. Import any separate spending or viewing keys again.
6. Confirm that Activity is not filtered to a different wallet or key.
7. Allow Auto node mode to select an endpoint that serves current block data.

Then run **Settings > Advanced > Rescan blockchain** from a suitable height. If the transaction is still absent, follow [Missing balance or transaction](troubleshooting.md#missing-balance-or-transaction).

## Finish moving the wallet safely

Keep the previous wallet until all of the following statements are true:

- The expected confirmed balance is present.
- Important incoming and outgoing transactions are visible.
- You have identified every imported key that still holds funds.
- You have tested a small receive and send, if practical.
- You have verified the Stashi Wallet seed phrase backup offline.
