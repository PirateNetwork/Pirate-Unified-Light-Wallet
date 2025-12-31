TODO
====

High priority
-------------
- Investigate app crash after Orchard send (suspect state rebuild after endpoint switch; check logs).
- Investigate slow sync on first start and restart.
- Investigate sync pausing when trial decrypt hits and `GetTransaction` runs; parallelize so block fetch never pauses.
- Investigate 200-block batches on mainnet and decide if we can safely increase or make adaptive by block size.
- Fix overlapping/overflowing text in mobile UI.
- Fix Rust crate warnings.

Medium
------
- During send, decide UI copy: "Generating Sapling Proof" vs "Generating Orchard Proof" vs "Generating Cryptographic Proof".
- Make passphrase setup happen once on first wallet creation; remove from subsequent seeds.
- Add ability to import private keys into existing wallets.
- Add configurable fee selection (slider or similar).
- Work on Tor (Arti) integration.
- Work on SOCKS5 + DNSCrypt integration.
- Work on I2P integration (and UI).
- Work on gated WireGuard integration (UI for Windows/Linux/Android only).
- Fix memo handling in UI.
- Integrate Panic PIN.
- Test background sync on real device.
- Fully implement screenshot protection.
- Add support for .pirate Unstoppable Domains (UI/UX plan).

Low priority
------------
- Sweep/consolidate functionality + UI.
- UI work for hardware wallet.
- Double-check RPC commands needed for hardware wallet.
- UI/UX polish.
- Comprehensive testing suite.
- Nix reproducible builds.
- Documentation.
- Performance optimization.
- Edge case handling.
- MM2 integration (talk to CIPI).
- Add USD price API + UI.
- Add TOS.
- Register a Co for app store publishing.
- Tor light server.
- Talk to Flexa.
- Talk to card providers.
- Explore alt packaging/publishing (F-Droid, APT for Linux).
- Decide with team on an app name + logo/icon
