# Backups and wallet security

[Previous: Network and synchronisation](network-and-sync.md) | [Guide contents](README.md) | [Next: Settings and release verification](settings-and-verification.md)

## What protects the funds

Cryptographic keys control ARRR. The seed phrase can recreate seed-derived keys. A spending key that was imported separately must have a separate backup.

The local passphrase protects access to the wallet database on the device. It does not replace the seed phrase and cannot recover a lost seed phrase.

## Back up the seed phrase

1. Confirm that no other person can see or record the seed phrase.
2. Open **Settings > Backups > Backup seed phrase**.
3. Authenticate with the requested biometric method or local passphrase.
4. Write the words in order on an offline medium, such as paper with permanent ink or a metal seed phrase backup.
5. Check every word and its position.
6. Store the backup separately from the unlocked device.
7. Leave the seed phrase screen and confirm that no photograph, clipboard entry, or print job remains.

Never enter a seed phrase into a support form, website, Telegram bot, Discord bot, browser extension, or remote support session.

## Back up imported keys

The seed phrase does not include keys that were imported separately.

1. Open **Settings > Keys & addresses**.
2. Open each imported spending key.
3. Select **Export keys** and authenticate.
4. Record the key offline and label the backup without exposing the key itself.
5. Record a birthday height or approximate first-use date.
6. Test the backup in a separate offline profile if your security process allows it.

A viewing key can also be backed up for monitoring. It cannot spend funds, but it reveals wallet activity within its scope.

## Local passphrase

Use **Settings > Security > Change passphrase** to change the local unlock passphrase. Select a long, unique passphrase that meets the requirements shown by Stashi Wallet.

A password manager can store the local passphrase. Store the seed phrase offline unless your security plan explicitly accounts for the risks of an online vault.

Changing the local passphrase does not change blockchain keys or addresses.

## Biometrics

Biometric unlock uses the device security system. Its security depends on the device, operating system, enrolled fingerprints or faces, and platform secure storage.

- Keep the local passphrase available as a fallback.
- Remove biometric access before giving the unlocked device to another person.
- If secure storage fails, use the local passphrase and review the device's Keychain, Keystore, or credential settings.

## Duress passphrase

The optional duress passphrase opens a separate empty decoy wallet when it is entered during unlock. It does not erase the primary wallet or move funds.

Before enabling a duress passphrase:

1. Understand the difference between the real and decoy passphrases.
2. Test both the real and decoy passphrases while the real seed phrase is safely backed up.
3. Do not reuse either passphrase elsewhere.
4. Remember that a decoy does not guarantee protection against every physical or forensic threat.

## Before deleting or resetting anything

Confirm the following points before deleting or resetting Stashi Wallet:

- You have the correct seed phrase.
- You have every separately imported spending key.
- You know the seed phrase language if it is not English.
- You have a birthday date or height early enough for recovery.
- You have a record of any higher seed account indices that were used.

A wallet database backup can preserve labels and local history, but it must not be the only recovery method.
