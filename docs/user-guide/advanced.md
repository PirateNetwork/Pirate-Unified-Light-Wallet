# Advanced use

[Previous: Troubleshooting](troubleshooting.md) | [Guide contents](README.md)

## Manage several wallets

Use the wallet selector at the top of Home to change the active wallet. The active selection controls balances, receiving addresses, key imports, rescans, and sends.

Before a sensitive action, check the wallet name twice. A key imported into one wallet is not automatically added to another.

## Exact birthdays and controlled rescans

An expert recovery normally uses a known block height before the first relevant transaction. This minimizes scan work without excluding history.

- Use an exact height when a transaction record or old wallet provides one.
- Use an earlier estimate when the first-use height is uncertain.
- A birthday update and a rescan are separate decisions. Save the value, then accept the rescan prompt when you need rebuilt state immediately.
- Imported keys carry their own birthday for historical scanning.

## Manual light-server configuration

Manual node settings are intended for users who operate or explicitly trust a lightwalletd endpoint.

1. Open **Settings > Privacy and Network > Node**.
2. Turn off automatic endpoint selection.
3. Enter the endpoint as host and port.
4. Select TLS when the endpoint supports it.
5. If using an SPKI pin, fetch and independently confirm the expected pin.
6. Save and watch the health and block-height status.

Return to Auto if the endpoint stalls. A valid TLS connection does not prove that a server is current or complete.

## SOCKS5 and I2P details

For SOCKS5, the proxy must be reachable from the wallet process. Localhost means the same device, not another computer on the network.

For I2P, use an endpoint and route compatible with the current I2P setup. I2P destinations cannot be reached through ordinary direct networking.

## Address management

Open a key under **Keys & addresses** to:

- Generate a new Sapling or Ironwood address when supported.
- Copy an existing address.
- Label and color-tag addresses.
- Archive addresses you no longer want in the main list.
- Consolidate or sweep spendable balances.
- Export key material after authentication.

Archiving an address does not invalidate it. A payment sent to an archived address can still belong to the wallet.

## Seed accounts and sparse account layouts

Stashi Wallet deliberately adds seed accounts consecutively. This avoids a manual field where a mistyped number could create a confusing sparse layout.

If another wallet used a distant account index, add five at a time and complete each scan until the expected account is reached. Account additions are durable and derive both supported shielded key types from the parent phrase.

Automatic restore discovery checks the standard account and the old wallet's bounded Sapling lookahead. It does not scan every possible ZIP-32 account because the account space is intentionally large and shielded ownership requires trial decryption.

## Payment disclosure

Payment disclosure tools can prove selected facts about a transaction to someone you choose. Disclosure data can reveal information that is otherwise shielded.

1. Open the payment disclosure tool from Actions.
2. Read exactly what the proof or disclosure will reveal.
3. Verify the transaction and recipient.
4. Share it only with the intended party.

Do not confuse a payment disclosure with a recovery phrase or viewing key. Never provide broader authority when a narrow transaction proof is enough.

## Swaps

When the build enables swaps, the swap interface uses the local KDF engine and external order-book and quote services.

- Enable the Komodo Swaps outbound permission.
- Check the selected wallet and funding balance.
- Review both assets, rate, minimums, fees, and timeout conditions.
- Keep the app running while an active swap requires it.
- Do not assume a displayed quote is guaranteed until the order is accepted.

Swaps have risks beyond a normal ARRR transfer. Start small and keep the swap record until settlement finishes.

## Local privacy data

Wallet labels, color tags, address-book notes, preferences, cached blocks, and debug logs can contain sensitive metadata even when they cannot spend funds.

- Encrypt device backups.
- Remove debug logs after support work.
- Treat viewing keys as private financial data.
- Review application data before transferring a computer to someone else.

## Release verification for reproducible-build work

Use unsigned artifacts for close byte comparison because platform signing, notarization, and store packaging can change the final package bytes. Check the signed checksum manifest first, then compare a locally reproduced unsigned output with the matching published unsigned artifact.

See [Verify Builds](../verify-build.md) for package names, build scripts, checksum commands, provenance, and SBOM details.
