# Windows release investigation and packaging (1.2.1)

## Evidence reviewed

The supplied VirusTotal screenshots identify SHA-256
`cc45faadfc8a8778f1c595f238b9484707ca2da18f455a6551b4a966ac223352`,
Stashi-Wallet-windows-installer.exe, 147.75 MB, analyzed September 4, 2026.
They show 17/67 vendor detections. ESET labels I2PD as riskware, CTX names
I2PD, and Rising names I2PD as a hacktool. Other vendors use generic Trojan
labels. Tencent reports FalseSign. The certificate is explicitly self-signed,
with an untrusted root. The separately displayed Stashi Wallet.exe has 0/70
detections. These observations implicate bundled I2PD and certificate reputation;
they do not establish the cause of every vendor verdict or prove absence of malware.

Tor relay/router traffic rules appear in the behavior report. This is consistent
with the wallet's built-in Tor client. The displayed LSASS unsigned-image and
new-root-certificate rules do not include the responsible DLL, certificate, or
attributed event. Searches of the Windows runner and packaging scripts found no
root-certificate import or process-injection commands. The screenshots alone
cannot resolve those rules; retain the full sandbox event trace for vendor review.
The report's process list includes operating-system background activity, so do
not attribute every listed process or action to Stashi.

## Packaging and authentication

Windows remains self-signed by project policy. No paid certificate or root-store
installation is required. Authenticode public trust is not used as the updater's
release authority. The updater requires an exact SHA-256 match in a manifest
authenticated with the pinned Stashi PGP release key before launching anything.

The installer excludes I2PD, Snowflake and obfs4proxy from its embedded payload.
It offers them as explicit, preselected optional downloads. Ordinary Tor is the
Rust Arti client and stays built into the wallet. Each optional executable is a
separate release asset whose SHA-256 is compiled into the installer. Inno Setup
checks that hash before copying the downloaded file. There is no unpinned latest
URL or silent fallback to an unchecked download. A failed component download must
be retried or the user must deliberately deselect that component. The compact
installation and the offline portable archive remain available for different needs.
Existing optional tools are not deleted by deselecting a component during upgrade.

CI signs installed application modules before constructing the installer, then
signs the installer and records the final installed executable hash. The signed
runtime manifest takes priority over the unsigned developer build manifest.
Privacy helpers retain their upstream/build bytes; they do not need the wallet's
self-signed Authenticode certificate. Verify My Build checks the available app
file, not the optional installation selection or the entire directory.

Publish all `Stashi-Wallet-windows-component-*.exe` assets together with the
installer. Their names, bytes and version-specific URLs must not change after
publication. CI includes them in VirusTotal scanning and the PGP signature bundle.
The VirusTotal report now records vendor names and exact detection labels for
each artifact, enabling comparison of the installer and individual components.

## Vendor remediation

No packaging change guarantees zero antivirus detections. Self-signed Windows
releases can continue to show reputation/untrusted-publisher warnings. Microsoft
does not offer a general false-positive prevention allowlist for developers.
Submit an actually detected final artifact as a software developer and retain its
submission ID and final determination. See Microsoft's
[developer FAQ](https://learn.microsoft.com/en-us/defender-xdr/developer-faq) and
[submission guide](https://learn.microsoft.com/en-us/unified-secops/submission-guide).
The supplied detection table does not establish a Microsoft Defender detection.
Do not report that a Defender submission happened unless it actually did.

For each disputed detection, provide the final artifact hash, release/source URL,
component inventory, detection label, and the explanation of optional privacy
tools. Re-scan the final signed release and individual helpers before claiming
an improvement. No antivirus exclusions, obfuscation or security-setting changes
are part of this remediation.

Inno download/hash behavior is documented in its
[Files reference](https://jrsoftware.org/ishelp/topic_filessection.htm).
