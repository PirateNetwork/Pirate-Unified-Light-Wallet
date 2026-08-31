# Backups and wallet security

[Previous: Network and sync](network-and-sync.md) | [Guide contents](README.md) | [Next: Settings and verification](settings-and-verification.md)

## What protects the funds

ARRR is controlled by cryptographic keys, not by the app installation. The recovery phrase can recreate seed-derived keys. A separately imported spending key must be backed up separately.

The app passphrase protects local access to the wallet database. It is not a replacement for the recovery phrase and it cannot recover a lost phrase.

## Back up the recovery phrase

1. Disconnect screen sharing and make sure the device is private.
2. Open **Settings > Backups > Backup seed phrase**.
3. Authenticate with the requested biometric or app passphrase.
4. Write the words in order on an offline medium.
5. Check every word and its position.
6. Store the backup somewhere separate from the unlocked device.
7. Leave the seed screen and confirm that no photo, clipboard entry, or print job remains.

For a large balance, keep two protected copies in different physical locations. A metal backup can be more resistant to fire and water than paper.

Never enter a recovery phrase into a support form, website, Telegram bot, Discord bot, browser extension, or remote-support session.

## Back up imported keys

The phrase does not include keys that were imported separately.

1. Open **Settings > Keys & addresses**.
2. Open each imported spending key.
3. Use **Export keys** and authenticate.
4. Record the key offline and label it without exposing the key itself.
5. Record a birthday height or approximate first-use date.
6. Test the backup on an offline or disposable profile if your security process allows it.

A viewing key can also be backed up for monitoring, but remember that it reveals wallet activity within its scope.

## App passphrase

Use **Settings > Security > Change passphrase** to change the local unlock passphrase. Choose a long, unique phrase. A password manager is suitable for the app passphrase, but the wallet recovery phrase should normally remain outside an online password vault unless you have deliberately chosen and secured that threat model.

Changing the app passphrase does not change blockchain keys or addresses.

## Biometrics

Biometric unlock uses the device security system. It is convenient, but its security depends on the device, operating system, enrolled fingerprints or faces, and platform secure storage.

- Keep the app passphrase available as a fallback.
- Remove biometric access before giving the unlocked device to another person.
- If secure storage fails, use the app passphrase and review the device's Keychain, Keystore, or credential settings.

## Duress passphrase

The optional duress passphrase opens a separate empty decoy wallet when entered at unlock. It does not erase the real wallet or move funds.

Before enabling it:

1. Understand how the real and decoy passphrases differ.
2. Test both while the real recovery phrase is safely backed up.
3. Do not reuse either passphrase elsewhere.
4. Remember that a decoy is not a guarantee against every physical or forensic threat.

## Device security

- Install operating system security updates.
- Use full-disk encryption and a device login.
- Do not run unknown wallet helpers, key checkers, or cracked software.
- Keep browser extensions and remote-access tools to a minimum on a wallet device.
- Lock the wallet before leaving the device.
- Review copied addresses for clipboard replacement malware.
- Prefer a dedicated device for a balance that would be painful to lose.

## Before deleting or resetting anything

Confirm that you have:

- The correct recovery phrase.
- Every separately imported spending key.
- The phrase language when it is not English.
- A birthday date or height early enough for recovery.
- A record of any higher seed account indices that were used.

A wallet database backup can help preserve labels and local history, but it must not be the only recovery method.
