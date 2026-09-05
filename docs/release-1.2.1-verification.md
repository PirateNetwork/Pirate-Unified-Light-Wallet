# 1.2.1 verification and integration notes

The public v1.1.9 and v1.2.0 PGP bundles were tested with the pinned repository
key, including a compiled Dart Windows probe. Their signatures validated here;
the other-device invalid-signature report has not been reproduced. Do not claim
that Windows Authenticode trust caused a PGP failure: they are separate systems.

The verifier now accepts binary and armored detached signatures, pins the full
release-key fingerprint, and identifies a device clock preceding the signature
as unavailable rather than a corrupt release. It compares content hashes before
filenames so renamed downloads and Android base.apk can match the original
published package. Actual signature or checksum failures still fail verification.

Windows, macOS and Linux use the signed installed-executable manifest or the
published distribution file where accessible. Android exposes the installed APK
path through a native channel when it is a single APK. Split packages cannot be
compared to one original APK. iOS store/TestFlight processing can alter files;
those installations do not claim a GitHub byte match. A missing payload checksum
is an unavailable comparison, not evidence of tampering. This feature checks an
available file, not every installed library or source-level reproducibility.

The desktop update host opens dialogs through the root navigator, selects the
highest supported stable semantic version after the one-hour release quarantine,
and distinguishes Later from Skip this version. Downloads require authenticated
PGP checksums, including on self-signed Windows releases. Android/iOS continue
to receive updates through their distribution channel, not desktop installers.

Auto server health checks probe eligible alternatives on the first failed
primary probe, queue requests made during an in-flight check, and migrate retired
automatic selections to the current same-route preset. Explicit manual selections
and certificate pins retain their existing behavior.

GUI: 1.2.1+10201. Backend/native SDKs: 0.3.4. React Native plugin and platform
packages: 0.3.5. The backend bump includes the preceding legacy account-discovery
fix: candidate discovery finalizes at the live chain tip, rather than waiting for
the continuous sync task to exit. Previously used accounts remain discoverable
even if their current balance is zero. No new RPC names or schema changes are
introduced by this release; hosts should refresh existing key-group/address
queries after recovery completes.
