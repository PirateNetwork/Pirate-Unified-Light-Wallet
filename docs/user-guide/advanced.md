# Advanced use

[Previous: Troubleshooting](troubleshooting.md) | [Guide contents](README.md)

## Manage several wallets

Use the wallet selector at the top of Home to change the active wallet. The active selection controls balances, receiving addresses, key imports, rescans, and sends.

Double-check the wallet name before a sensitive action. A key imported into a wallet is not automatically added to another wallet.

## Exact birthdays and controlled rescans

An expert recovery normally uses a known block height before the first relevant transaction. This minimises scanning work without excluding transaction history.

- Use an exact height when a transaction record or previous wallet provides it.
- Use an earlier estimate when the first-use height is uncertain.
- Treat a birthday update and a rescan as separate decisions. Save the value, then accept the rescan prompt when the wallet information must be rebuilt immediately.
- Use the birthday stored with an imported key for its historical scan.

## Manual light-server configuration

Manual node settings are intended for users who operate or explicitly trust a lightwalletd endpoint.

1. Open **Settings > Privacy and Network > Node**.
2. Turn off automatic endpoint selection.
3. Enter the endpoint as a host and port.
4. Select TLS when the endpoint supports it.
5. If you use an SPKI pin, obtain and independently confirm the expected pin.
6. Save the endpoint and monitor its health and blockchain height.

Return to Auto if the endpoint stops advancing. A valid TLS connection does not confirm that a server is current or complete.

## SOCKS5 and I2P details

For SOCKS5, the proxy must be reachable from the Stashi Wallet process. Localhost means the same device, not another computer on the network.

For I2P, use an endpoint and route that are compatible with the current I2P configuration. I2P destinations cannot be reached through ordinary Direct networking.

## Address management

Open a key under **Keys & addresses** to complete the following tasks:

- Generate a new address for a supported shielded pool.
- Copy an existing address.
- Label addresses and apply colour tags.
- Archive addresses that you no longer want in the main list.
- Consolidate or sweep spendable balances.
- Export key material after authentication.

Archiving an address does not invalidate it. A payment sent to an archived address can still belong to the wallet.

## Seed accounts and sparse account layouts

Stashi Wallet adds seed accounts consecutively. This avoids a manual field in which a mistyped number could create a confusing sparse account layout.

If another wallet used a distant account index, add five accounts at a time and allow each scan to finish until you reach the expected account. Account additions are saved and derive both supported shielded key types from the parent seed phrase.

Automatic recovery checks the standard account and the bounded Sapling lookahead used by previous wallets. It does not scan every possible ZIP-32 account because the account space is large and shielded ownership requires each key to test the relevant outputs.

## Payment disclosure

Payment disclosure tools can prove selected facts about a transaction to a person that you choose. Disclosure data can reveal information that is otherwise shielded.

1. Open the payment disclosure tool from Wallets.
2. Read the information that the proof or disclosure will reveal.
3. Verify the transaction and recipient.
4. Share the disclosure only with the intended recipient.

Do not confuse a payment disclosure with a seed phrase or viewing key. Do not provide broader authority when a transaction proof is sufficient.

## Swaps

When swaps are available in your Stashi Wallet version, the swap interface uses the local KDF engine and external order book and quote services.

- Enable the **Komodo Swaps** outbound permission.
- Check the selected wallet and funding balance.
- Review both assets, the rate, minimums, fees, and timeout conditions.
- Keep the application running while an active swap requires it.
- Do not assume that a displayed quote is guaranteed until the order is accepted.

Swaps have risks beyond a normal ARRR transfer.

## Local privacy data

Wallet labels, colour tags, address book notes, preferences, cached blocks, and debug logs can contain sensitive information even when they cannot spend funds.

- Encrypt device backups.
- Remove debug logs after support work.
- Treat viewing keys as private financial information.
- Review application data before transferring a computer to another person.

## Independent release verification

Use unsigned release files for close byte comparison because platform signing, notarisation, and store packaging can change the final package bytes. Check the signed checksum manifest first, then compare a locally reproduced unsigned file with the matching published unsigned file.

See [Verify Builds](../verify-build.md) for package names, compilation scripts, checksum commands, provenance information, and Software Bill of Materials (SBOM) details.
