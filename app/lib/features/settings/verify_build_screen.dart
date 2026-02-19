import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../ui/atoms/p_button.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/generated/api.dart' as api;
import 'providers/preferences_providers.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// Verify My Build Screen - Shows reproducible build verification steps
class VerifyBuildScreen extends ConsumerStatefulWidget {
  const VerifyBuildScreen({super.key});

  @override
  ConsumerState<VerifyBuildScreen> createState() => _VerifyBuildScreenState();
}

enum ReleaseVerificationStatus {
  idle,
  checking,
  match,
  mismatch,
  noRelease,
  noChecksums,
  noLocalArtifact,
  noMatchingChecksum,
  error,
}

class _ReleaseAsset {
  const _ReleaseAsset({required this.name, required this.url});

  final String name;
  final String url;
}

class _ReleaseInfo {
  const _ReleaseInfo({
    required this.tagName,
    required this.name,
    required this.url,
    required this.isDraft,
    required this.isPrerelease,
    required this.assets,
  });

  final String tagName;
  final String name;
  final String url;
  final bool isDraft;
  final bool isPrerelease;
  final List<_ReleaseAsset> assets;
}

class _LocalArtifact {
  const _LocalArtifact({
    required this.path,
    required this.name,
    required this.sha256,
  });

  final String path;
  final String name;
  final String sha256;
}

class _ChecksumResult {
  const _ChecksumResult({required this.entries, required this.sourceName});

  final Map<String, String> entries;
  final String? sourceName;
}

class _VerifyBuildScreenState extends ConsumerState<VerifyBuildScreen> {
  Map<String, String>? _buildInfo;
  bool _isLoading = true;
  String? _error;
  ReleaseVerificationStatus _verificationStatus =
      ReleaseVerificationStatus.idle;
  String? _verificationMessage;
  String? _releaseTag;
  String? _releaseUrl;
  String? _checksumAssetName;
  String? _signatureAssetName;
  String? _localArtifactPath;
  String? _localArtifactName;
  String? _localHash;
  String? _expectedHash;
  String? _matchedChecksumName;
  DateTime? _lastCheckedAt;

  @override
  void initState() {
    super.initState();
    _loadBuildInfo();
  }

  Future<void> _loadBuildInfo() async {
    try {
      final info = await api.getBuildInfo();

      if (!mounted) return;
      setState(() {
        _buildInfo = {
          'version': info.version,
          'gitCommit': info.gitCommit,
          'buildDate': info.buildDate,
          'rustVersion': info.rustVersion,
          'targetTriple': info.targetTriple,
        };
        _error = null;
        _isLoading = false;
      });
      await _checkReleaseVerification();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = 'Failed to load build info: $e';
        _isLoading = false;
      });
    }
  }

  Future<void> _checkReleaseVerification() async {
    if (kIsWeb) {
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage = 'Release verification is not supported on web.';
      });
      return;
    }

    final allowGithubApis = ref.read(allowGithubApisProvider);
    if (!allowGithubApis) {
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage =
            'Outbound GitHub checks are disabled in Settings > Outbound API Calls.';
      });
      return;
    }

    setState(() {
      _verificationStatus = ReleaseVerificationStatus.checking;
      _verificationMessage = null;
      _releaseTag = null;
      _releaseUrl = null;
      _checksumAssetName = null;
      _signatureAssetName = null;
      _localArtifactPath = null;
      _localArtifactName = null;
      _localHash = null;
      _expectedHash = null;
      _matchedChecksumName = null;
      _lastCheckedAt = DateTime.now();
    });

    try {
      final localArtifacts = await _loadLocalArtifacts();
      final primaryLocalArtifact = localArtifacts.isEmpty
          ? null
          : localArtifacts.first;
      if (!mounted) return;
      setState(() {
        _localArtifactPath = primaryLocalArtifact?.path;
        _localArtifactName = primaryLocalArtifact?.name;
        _localHash = primaryLocalArtifact?.sha256;
      });

      final releases = await _fetchReleases();
      if (!mounted) return;
      if (releases.isEmpty) {
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noRelease;
          _verificationMessage =
              'No GitHub releases found yet. This page will verify hashes once releases are published.';
        });
        return;
      }

      final release = _selectRelease(releases);
      final signatureAsset = _findSignatureAsset(release.assets);

      setState(() {
        _releaseTag = release.tagName.isEmpty ? release.name : release.tagName;
        _releaseUrl = release.url;
        _signatureAssetName = signatureAsset?.name;
      });

      final checksums = await _fetchChecksums(release.assets);
      if (!mounted) return;

      setState(() {
        _checksumAssetName = checksums.sourceName;
      });

      if (checksums.entries.isEmpty) {
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noChecksums;
          _verificationMessage =
              'This release cannot be verified because no readable checksums were published. Only install builds from official PirateNetwork release assets.';
        });
        return;
      }

      if (primaryLocalArtifact == null) {
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noLocalArtifact;
          _verificationMessage =
              'Local build artifact could not be accessed on this platform.';
        });
        return;
      }

      final localArtifact = _selectLocalArtifactForChecksums(
        localArtifacts,
        checksums.entries,
      );
      final expectedHash = _lookupChecksum(
        checksums.entries,
        localArtifact.name,
      );
      if (expectedHash == null) {
        final sampledNames = localArtifacts
            .take(3)
            .map((artifact) => artifact.name)
            .join(', ');
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noMatchingChecksum;
          _verificationMessage =
              'This build is not verified against the selected official release. '
              'Published checksums were found, but none match local artifacts'
              '${sampledNames.isEmpty ? '.' : ' ($sampledNames).'}';
        });
        return;
      }

      final normalizedExpected = _normalizeHash(expectedHash);
      final normalizedLocal = _normalizeHash(localArtifact.sha256);
      final matched = normalizedExpected == normalizedLocal;
      final matchedName = _lookupChecksumName(
        checksums.entries,
        localArtifact.name,
      );

      setState(() {
        _localArtifactPath = localArtifact.path;
        _localArtifactName = localArtifact.name;
        _localHash = localArtifact.sha256;
        _expectedHash = expectedHash;
        _matchedChecksumName = matchedName;
        _verificationStatus = matched
            ? ReleaseVerificationStatus.match
            : ReleaseVerificationStatus.mismatch;
        _verificationMessage = matched
            ? 'Local build hash matches the published release.'
            : 'Local build hash does not match the published release. Do not trust this build for funds unless you can independently verify its source and build pipeline.';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage = 'Verification failed: $e';
      });
    }
  }

  Future<List<_LocalArtifact>> _loadLocalArtifacts() async {
    final artifacts = <_LocalArtifact>[];
    final seenPaths = <String>{};

    Future<void> addArtifact(String path) async {
      final file = File(path);
      if (!file.existsSync()) return;
      final absolutePath = file.absolute.path;
      if (seenPaths.contains(absolutePath)) return;
      final hash = await _hashFile(file);
      artifacts.add(
        _LocalArtifact(
          path: absolutePath,
          name: absolutePath.split(Platform.pathSeparator).last,
          sha256: hash,
        ),
      );
      seenPaths.add(absolutePath);
    }

    try {
      final resolvedExecutable = Platform.resolvedExecutable;
      if (resolvedExecutable.isNotEmpty) {
        await addArtifact(resolvedExecutable);
      }
    } catch (_) {
      // Ignore and continue with path probing.
    }

    for (final path in _candidateArtifactPaths()) {
      await addArtifact(path);
    }

    return artifacts;
  }

  Set<String> _candidateArtifactPaths() {
    final candidates = <String>{};
    final roots = <String>{Directory.current.absolute.path};

    try {
      final executable = File(Platform.resolvedExecutable).absolute;
      Directory dir = executable.parent;
      for (var i = 0; i < 8; i++) {
        roots.add(dir.path);
        final parent = dir.parent;
        if (parent.path == dir.path) break;
        dir = parent;
      }
    } catch (_) {
      // Ignore.
    }

    const relativeCandidates = <String>[
      'dist/windows/pirate-unified-wallet-windows-installer.exe',
      'dist/windows/pirate-unified-wallet-windows-installer-unsigned.exe',
      'dist/windows/pirate-unified-wallet-windows-portable.zip',
      'dist/macos/pirate-unified-wallet-macos.dmg',
      'dist/macos/pirate-unified-wallet-macos-unsigned.dmg',
      'dist/linux/pirate-unified-wallet-linux-x86_64.AppImage',
      'dist/linux/pirate-unified-wallet-amd64.deb',
    ];

    for (final root in roots) {
      for (final relativePath in relativeCandidates) {
        candidates.add(_joinPath(root, relativePath.split('/')).absolute.path);
      }
    }

    return candidates;
  }

  File _joinPath(String root, List<String> parts) {
    return File([root, ...parts].join(Platform.pathSeparator));
  }

  _LocalArtifact _selectLocalArtifactForChecksums(
    List<_LocalArtifact> localArtifacts,
    Map<String, String> checksums,
  ) {
    for (final localArtifact in localArtifacts) {
      if (_lookupChecksum(checksums, localArtifact.name) != null) {
        return localArtifact;
      }
    }
    return localArtifacts.first;
  }

  String? _lookupChecksum(Map<String, String> checksums, String localName) {
    final canonicalLocal = _canonicalAssetName(localName);
    for (final entry in checksums.entries) {
      if (_canonicalAssetName(entry.key) == canonicalLocal) {
        return entry.value;
      }
    }
    return null;
  }

  String? _lookupChecksumName(Map<String, String> checksums, String localName) {
    final canonicalLocal = _canonicalAssetName(localName);
    for (final entry in checksums.entries) {
      if (_canonicalAssetName(entry.key) == canonicalLocal) {
        return entry.key;
      }
    }
    return null;
  }

  String _canonicalAssetName(String value) {
    final normalizedPath = value
        .replaceAll(String.fromCharCode(92), '/')
        .trim();
    final parts = normalizedPath.split('/');
    return parts.last.toLowerCase();
  }

  Future<String> _hashFile(File file) async {
    final digest = await sha256.bind(file.openRead()).first;
    return digest.toString();
  }

  bool _isDirectChecksumAsset(String name) {
    final lower = name.toLowerCase();
    return lower.endsWith('.sha256') ||
        lower.endsWith('.sha256sum') ||
        lower.contains('checksums');
  }

  bool _isChecksumBundleAsset(String name) {
    final lower = name.toLowerCase();
    return lower.endsWith('.zip') &&
        (lower.contains('metadata') ||
            lower.contains('checksum') ||
            lower.contains('sha256'));
  }

  Future<_ChecksumResult> _fetchChecksums(List<_ReleaseAsset> assets) async {
    final entries = <String, String>{};
    String? sourceName;

    final checksumAssets = assets.where(
      (asset) => _isDirectChecksumAsset(asset.name),
    );
    for (final asset in checksumAssets) {
      final text = await _downloadText(asset.url);
      final parsed = _parseChecksums(text, asset.name);
      if (parsed.isNotEmpty) {
        entries.addAll(parsed);
        sourceName ??= asset.name;
      }
    }

    final checksumBundles = assets.where(
      (asset) => _isChecksumBundleAsset(asset.name),
    );
    for (final asset in checksumBundles) {
      final bundleBytes = await _downloadBytes(asset.url);
      final parsed = _parseChecksumsFromZip(bundleBytes);
      if (parsed.isNotEmpty) {
        entries.addAll(parsed);
        sourceName ??= asset.name;
      }
    }

    return _ChecksumResult(entries: entries, sourceName: sourceName);
  }

  Future<Uint8List> _downloadBytes(String url) async {
    final client = HttpClient();
    try {
      final request = await client.getUrl(Uri.parse(url));
      request.headers.set('User-Agent', 'PirateWallet');
      final response = await request.close();
      if (response.statusCode >= 400) {
        throw Exception('Failed to download checksum asset');
      }
      final bytes = await consolidateHttpClientResponseBytes(response);
      return bytes;
    } finally {
      client.close(force: true);
    }
  }

  Map<String, String> _parseChecksumsFromZip(Uint8List bytes) {
    final map = <String, String>{};
    Archive archive;
    try {
      archive = ZipDecoder().decodeBytes(bytes);
    } catch (_) {
      return map;
    }

    for (final file in archive.files) {
      if (!file.isFile || !_isDirectChecksumAsset(file.name)) {
        continue;
      }

      try {
        final text = utf8.decode(
          file.content as List<int>,
          allowMalformed: true,
        );
        map.addAll(_parseChecksums(text, file.name));
      } catch (_) {
        // Ignore malformed entries and keep parsing the rest.
      }
    }
    return map;
  }

  Future<String> _downloadText(String url) async {
    final bytes = await _downloadBytes(url);
    return utf8.decode(bytes, allowMalformed: true);
  }

  Map<String, String> _parseChecksums(String text, String assetName) {
    final map = <String, String>{};
    final lines = LineSplitter.split(text);
    for (final line in lines) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || trimmed.startsWith('#')) {
        continue;
      }

      final parts = trimmed.split(RegExp(r'\s+'));
      if (parts.length == 1) {
        final fallbackName = assetName.replaceAll(
          RegExp(r'\.sha256(sum)?$', caseSensitive: false),
          '',
        );
        map[_canonicalAssetName(fallbackName)] = parts.first;
        continue;
      }

      final hash = parts.first.trim();
      final filename = parts.sublist(1).join(' ').replaceFirst('*', '').trim();
      if (hash.isNotEmpty && filename.isNotEmpty) {
        map[_canonicalAssetName(filename)] = hash;
      }
    }
    return map;
  }

  String _normalizeHash(String value) {
    return value.trim().toLowerCase();
  }

  Future<List<_ReleaseInfo>> _fetchReleases() async {
    final client = HttpClient();
    try {
      final uri = Uri.parse(
        'https://api.github.com/repos/PirateNetwork/Pirate-Unified-Light-Wallet/releases',
      );
      final request = await client.getUrl(uri);
      request.headers.set('Accept', 'application/vnd.github+json');
      request.headers.set('User-Agent', 'PirateWallet');
      final response = await request.close();
      if (response.statusCode == 404) {
        return [];
      }
      if (response.statusCode >= 400) {
        throw Exception('GitHub API returned ${response.statusCode}');
      }

      final body = await response.transform(utf8.decoder).join();
      final data = jsonDecode(body);
      if (data is! List) {
        throw Exception('Unexpected GitHub API response');
      }

      return data.whereType<Map<String, dynamic>>().map<_ReleaseInfo>((entry) {
        final assets = <_ReleaseAsset>[];
        final rawAssets = entry['assets'];
        if (rawAssets is List) {
          for (final asset in rawAssets) {
            if (asset is Map<String, dynamic>) {
              final name = asset['name']?.toString() ?? '';
              final url = asset['browser_download_url']?.toString() ?? '';
              if (name.isNotEmpty && url.isNotEmpty) {
                assets.add(_ReleaseAsset(name: name, url: url));
              }
            }
          }
        }

        return _ReleaseInfo(
          tagName: entry['tag_name']?.toString() ?? '',
          name: entry['name']?.toString() ?? '',
          url: entry['html_url']?.toString() ?? '',
          isDraft: entry['draft'] == true,
          isPrerelease: entry['prerelease'] == true,
          assets: assets,
        );
      }).toList();
    } finally {
      client.close(force: true);
    }
  }

  _ReleaseInfo _selectRelease(List<_ReleaseInfo> releases) {
    final version = _buildInfo?['version'];
    if (version != null && version.isNotEmpty) {
      final normalized = version.startsWith('v') ? version : 'v$version';
      for (final release in releases) {
        if (release.tagName == version ||
            release.tagName == normalized ||
            release.name == version ||
            release.name == normalized) {
          return release;
        }
      }
    }

    for (final release in releases) {
      if (!release.isDraft && !release.isPrerelease) {
        return release;
      }
    }

    return releases.first;
  }

  _ReleaseAsset? _findSignatureAsset(List<_ReleaseAsset> assets) {
    for (final asset in assets) {
      final name = asset.name.toLowerCase();
      if (name.endsWith('.sig') ||
          name.endsWith('.asc') ||
          name.endsWith('.minisig')) {
        return asset;
      }
    }
    return null;
  }

  Color _statusColor(ReleaseVerificationStatus status) {
    switch (status) {
      case ReleaseVerificationStatus.match:
        return AppColors.success;
      case ReleaseVerificationStatus.mismatch:
        return AppColors.error;
      case ReleaseVerificationStatus.checking:
        return AppColors.warning;
      case ReleaseVerificationStatus.error:
        return AppColors.error;
      case ReleaseVerificationStatus.noRelease:
      case ReleaseVerificationStatus.noChecksums:
      case ReleaseVerificationStatus.noMatchingChecksum:
      case ReleaseVerificationStatus.noLocalArtifact:
        return AppColors.warning;
      case ReleaseVerificationStatus.idle:
        return AppColors.textTertiary;
    }
  }

  String _statusLabel(ReleaseVerificationStatus status) {
    switch (status) {
      case ReleaseVerificationStatus.match:
        return 'Match';
      case ReleaseVerificationStatus.mismatch:
        return 'Mismatch';
      case ReleaseVerificationStatus.checking:
        return 'Checking';
      case ReleaseVerificationStatus.noRelease:
        return 'No Releases';
      case ReleaseVerificationStatus.noChecksums:
        return 'No Checksums';
      case ReleaseVerificationStatus.noMatchingChecksum:
        return 'Unverified Build';
      case ReleaseVerificationStatus.noLocalArtifact:
        return 'No Local Artifact';
      case ReleaseVerificationStatus.error:
        return 'Error';
      case ReleaseVerificationStatus.idle:
        return 'Not Checked';
    }
  }

  String _formatTimestamp(DateTime timestamp) {
    final local = timestamp.toLocal();
    String two(int value) => value.toString().padLeft(2, '0');
    return '${local.year}-${two(local.month)}-${two(local.day)} '
        '${two(local.hour)}:${two(local.minute)}';
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Verify My Build'.tr,
      appBar: PAppBar(
        title: 'Verify My Build'.tr,
        subtitle: 'Release verification and build metadata'.tr,
        showBackButton: true,
      ),
      body: _isLoading
          ? const Center(child: CircularProgressIndicator())
          : SingleChildScrollView(
              padding: PSpacing.screenPadding(
                MediaQuery.of(context).size.width,
                vertical: PSpacing.xl,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _buildHeader(),
                  SizedBox(height: PSpacing.xl),
                  _buildSummaryCards(),
                  if (_error != null) ...[
                    SizedBox(height: PSpacing.lg),
                    Container(
                      padding: EdgeInsets.all(PSpacing.md),
                      decoration: BoxDecoration(
                        color: AppColors.errorBackground,
                        border: Border.all(color: AppColors.errorBorder),
                        borderRadius: BorderRadius.circular(12),
                      ),
                      child: Text(
                        _error!,
                        style: PTypography.bodyMedium(color: AppColors.error),
                        textAlign: TextAlign.center,
                      ),
                    ),
                  ],
                  SizedBox(height: PSpacing.xl),
                  _buildResourceLinks(),
                ],
              ),
            ),
    );
  }

  Widget _buildHeader() {
    return Column(
      children: [
        Container(
          width: 80,
          height: 80,
          decoration: BoxDecoration(
            color: AppColors.successBackground,
            shape: BoxShape.circle,
            border: Border.all(color: AppColors.successBorder, width: 2),
          ),
          child: Icon(Icons.verified_user, size: 40, color: AppColors.success),
        ),
        SizedBox(height: PSpacing.lg),
        Text(
          'Reproducible Builds'.tr,
          style: PTypography.heading2(color: AppColors.textPrimary),
          textAlign: TextAlign.center,
        ),
        SizedBox(height: PSpacing.md),
        Text(
          'Verify that this app matches our official source code'.tr,
          style: PTypography.bodyMedium(color: AppColors.textSecondary),
          textAlign: TextAlign.center,
        ),
      ],
    );
  }

  Widget _buildSummaryCards() {
    return LayoutBuilder(
      builder: (context, constraints) {
        final useTwoColumns = constraints.maxWidth >= 980;
        if (!useTwoColumns) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _buildReleaseVerificationCard(),
              SizedBox(height: PSpacing.lg),
              _buildBuildInfoCard(),
            ],
          );
        }

        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(flex: 5, child: _buildReleaseVerificationCard()),
            SizedBox(width: PSpacing.lg),
            Expanded(flex: 3, child: _buildBuildInfoCard()),
          ],
        );
      },
    );
  }

  Widget _buildReleaseVerificationCard() {
    final statusColor = _statusColor(_verificationStatus);
    final statusLabel = _statusLabel(_verificationStatus);
    final statusBackground = statusColor.withValues(alpha: 0.15);
    final stronglyUnverified =
        _verificationStatus == ReleaseVerificationStatus.mismatch ||
        _verificationStatus == ReleaseVerificationStatus.noMatchingChecksum ||
        _verificationStatus == ReleaseVerificationStatus.noChecksums;
    final officialReleaseUrl =
        _releaseUrl ??
        'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/releases';

    return _buildSurfaceCard(
      title: 'Official Release Verification'.tr,
      subtitle:
          'Checks the selected GitHub release for published hashes and compares them to local artifacts.'
              .tr,
      trailing: Container(
        padding: EdgeInsets.symmetric(
          horizontal: PSpacing.sm,
          vertical: PSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: statusBackground,
          borderRadius: BorderRadius.circular(999),
          border: Border.all(color: statusColor.withValues(alpha: 0.4)),
        ),
        child: Text(
          statusLabel,
          style: PTypography.bodySmall(
            color: statusColor,
          ).copyWith(fontWeight: FontWeight.w700),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_verificationStatus == ReleaseVerificationStatus.checking) ...[
            SizedBox(height: PSpacing.sm),
            LinearProgressIndicator(
              minHeight: 3,
              color: AppColors.accentPrimary,
              backgroundColor: AppColors.backgroundPanel,
            ),
            SizedBox(height: PSpacing.md),
          ],
          _buildDataRow(label: 'Release'.tr, value: _releaseTag ?? 'Not found'),
          if (_releaseUrl != null)
            _buildDataRow(
              label: 'Release URL'.tr,
              value: _releaseUrl!,
              copyable: true,
              monospace: true,
            ),
          _buildDataRow(
            label: 'Local Artifact'.tr,
            value: _localArtifactName ?? 'Unavailable',
          ),
          if (_localArtifactPath != null)
            _buildDataRow(
              label: 'Local Path'.tr,
              value: _localArtifactPath!,
              copyable: true,
              monospace: true,
            ),
          if (_localHash != null)
            _buildDataRow(
              label: 'Local SHA256'.tr,
              value: _localHash!,
              copyable: true,
              monospace: true,
            ),
          if (_expectedHash != null)
            _buildDataRow(
              label: 'Expected SHA256'.tr,
              value: _expectedHash!,
              copyable: true,
              monospace: true,
            ),
          if (_matchedChecksumName != null)
            _buildDataRow(
              label: 'Matched Checksum Entry'.tr,
              value: _matchedChecksumName!,
              monospace: true,
            ),
          if (_checksumAssetName != null)
            _buildDataRow(
              label: 'Checksum Source'.tr,
              value: _checksumAssetName!,
              copyable: true,
              monospace: true,
            ),
          if (_signatureAssetName != null)
            _buildDataRow(
              label: 'Signature Asset'.tr,
              value: _signatureAssetName!,
              copyable: true,
              monospace: true,
            ),
          if (_lastCheckedAt != null)
            _buildDataRow(
              label: 'Last Checked'.tr,
              value: _formatTimestamp(_lastCheckedAt!),
            ),
          if (_verificationMessage != null) ...[
            SizedBox(height: PSpacing.sm),
            Container(
              padding: EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: statusColor.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: statusColor.withValues(alpha: 0.35)),
              ),
              child: Text(
                _verificationMessage!,
                style: PTypography.bodySmall(
                  color: statusColor.withValues(alpha: 0.95),
                ),
              ),
            ),
          ],
          if (stronglyUnverified) ...[
            SizedBox(height: PSpacing.sm),
            Container(
              padding: EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.error.withValues(alpha: 0.10),
                borderRadius: BorderRadius.circular(12),
                border: Border.all(
                  color: AppColors.error.withValues(alpha: 0.35),
                ),
              ),
              child: Text(
                'Warning: this build is not currently verified against an official PirateNetwork checksum. '
                        'Use official release downloads before storing funds.'
                    .tr,
                style: PTypography.bodySmall(
                  color: AppColors.error.withValues(alpha: 0.95),
                ),
              ),
            ),
          ],
          SizedBox(height: PSpacing.lg),
          LayoutBuilder(
            builder: (context, constraints) {
              final stackedButtons = constraints.maxWidth < 560;
              if (stackedButtons) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    PButton(
                      onPressed:
                          _verificationStatus ==
                              ReleaseVerificationStatus.checking
                          ? null
                          : _checkReleaseVerification,
                      text: 'Check GitHub',
                      variant: PButtonVariant.outline,
                      loading:
                          _verificationStatus ==
                          ReleaseVerificationStatus.checking,
                      fullWidth: true,
                    ),
                    if (_localHash != null) ...[
                      SizedBox(height: PSpacing.sm),
                      PButton(
                        onPressed: () => _copyToClipboard(_localHash!),
                        text: 'Copy Local Hash',
                        variant: PButtonVariant.secondary,
                        fullWidth: true,
                      ),
                    ],
                  ],
                );
              }

              return Row(
                children: [
                  Expanded(
                    child: PButton(
                      onPressed:
                          _verificationStatus ==
                              ReleaseVerificationStatus.checking
                          ? null
                          : _checkReleaseVerification,
                      text: 'Check GitHub',
                      variant: PButtonVariant.outline,
                      loading:
                          _verificationStatus ==
                          ReleaseVerificationStatus.checking,
                      fullWidth: true,
                    ),
                  ),
                  if (_localHash != null) ...[
                    SizedBox(width: PSpacing.md),
                    Expanded(
                      child: PButton(
                        onPressed: () => _copyToClipboard(_localHash!),
                        text: 'Copy Local Hash',
                        variant: PButtonVariant.secondary,
                        fullWidth: true,
                      ),
                    ),
                  ],
                ],
              );
            },
          ),
          if (stronglyUnverified) ...[
            SizedBox(height: PSpacing.sm),
            PButton(
              onPressed: () => _openLink(officialReleaseUrl),
              text: 'Open Official Releases',
              variant: PButtonVariant.outline,
              fullWidth: true,
            ),
          ],
          if (_signatureAssetName != null) ...[
            SizedBox(height: PSpacing.md),
            Text(
              'Signature verification in-app will be enabled once release signing keys are publicly published.'
                  .tr,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildSurfaceCard({
    required String title,
    required Widget child,
    String? subtitle,
    Widget? trailing,
  }) {
    return Container(
      padding: EdgeInsets.all(PSpacing.lg),
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(18),
        border: Border.all(color: AppColors.borderSubtle),
        boxShadow: [
          BoxShadow(
            color: AppColors.backgroundPanel.withValues(alpha: 0.35),
            blurRadius: 24,
            offset: const Offset(0, 10),
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: PTypography.bodyLarge(
                        color: AppColors.textPrimary,
                      ).copyWith(fontWeight: FontWeight.w700),
                    ),
                    if (subtitle != null) ...[
                      SizedBox(height: PSpacing.xs),
                      Text(
                        subtitle,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (trailing != null) ...[SizedBox(width: PSpacing.sm), trailing],
            ],
          ),
          SizedBox(height: PSpacing.md),
          child,
        ],
      ),
    );
  }

  Widget _buildDataRow({
    required String label,
    required String value,
    bool copyable = false,
    bool monospace = false,
  }) {
    return Padding(
      padding: EdgeInsets.only(bottom: PSpacing.sm),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            label,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.xs),
          Container(
            width: double.infinity,
            padding: EdgeInsets.symmetric(
              horizontal: PSpacing.sm,
              vertical: PSpacing.sm,
            ),
            decoration: BoxDecoration(
              color: AppColors.backgroundPanel.withValues(alpha: 0.55),
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: AppColors.borderSubtle),
            ),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: SelectionArea(
                    child: SelectableText(
                      value,
                      style: PTypography.bodySmall(
                        color: AppColors.textPrimary,
                      ).copyWith(fontFamily: monospace ? 'monospace' : null),
                    ),
                  ),
                ),
                if (copyable)
                  IconButton(
                    icon: Icon(
                      Icons.copy,
                      size: 16,
                      color: AppColors.textTertiary,
                    ),
                    padding: EdgeInsets.zero,
                    constraints: const BoxConstraints(
                      minWidth: 24,
                      minHeight: 24,
                    ),
                    onPressed: () => _copyToClipboard(value),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildBuildInfoCard() {
    if (_buildInfo == null) {
      return _buildSurfaceCard(
        title: 'Build Information'.tr,
        child: Text(
          'Build information unavailable.'.tr,
          style: PTypography.bodyMedium(color: AppColors.textSecondary),
        ),
      );
    }

    return _buildSurfaceCard(
      title: 'Build Information'.tr,
      subtitle: 'Compile metadata from the bundled Rust FFI library.'.tr,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _buildDataRow(label: 'Version'.tr, value: _buildInfo!['version']!),
          _buildDataRow(
            label: 'Git Commit'.tr,
            value: _buildInfo!['gitCommit']!,
            copyable: true,
            monospace: true,
          ),
          _buildDataRow(
            label: 'Build Date'.tr,
            value: _buildInfo!['buildDate']!,
            monospace: true,
          ),
          _buildDataRow(
            label: 'Rust Version'.tr,
            value: _buildInfo!['rustVersion']!,
            monospace: true,
          ),
          _buildDataRow(
            label: 'Target'.tr,
            value: _buildInfo!['targetTriple']!,
            monospace: true,
          ),
        ],
      ),
    );
  }

  Widget _buildResourceLinks() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Resources'.tr,
          style: PTypography.heading4(color: AppColors.textPrimary),
        ),
        SizedBox(height: PSpacing.md),
        _buildLinkCard(
          icon: Icons.article,
          title: 'Verification Guide'.tr,
          description: 'Complete documentation on reproducible builds'.tr,
          url:
              'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/blob/main/docs/verify-build.md',
        ),
        SizedBox(height: PSpacing.sm),
        _buildLinkCard(
          icon: Icons.code,
          title: 'Source Code'.tr,
          description: 'View the full source code on GitHub'.tr,
          url: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet',
        ),
        SizedBox(height: PSpacing.sm),
        _buildLinkCard(
          icon: Icons.security,
          title: 'Security Practices'.tr,
          description: 'Learn about our security model'.tr,
          url:
              'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/blob/main/docs/security.md',
        ),
      ],
    );
  }

  Widget _buildLinkCard({
    required IconData icon,
    required String title,
    required String description,
    required String url,
  }) {
    return InkWell(
      onTap: () => _openLink(url),
      onLongPress: () => _copyToClipboard(url),
      borderRadius: BorderRadius.circular(12),
      child: Container(
        padding: EdgeInsets.all(PSpacing.md),
        decoration: BoxDecoration(
          color: AppColors.backgroundSurface,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: AppColors.borderSubtle),
        ),
        child: Row(
          children: [
            Icon(icon, color: AppColors.accentPrimary, size: 24),
            SizedBox(width: PSpacing.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: PTypography.bodyMedium(
                      color: AppColors.textPrimary,
                    ).copyWith(fontWeight: FontWeight.w500),
                  ),
                  Text(
                    description,
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                ],
              ),
            ),
            Icon(Icons.open_in_new, color: AppColors.textTertiary, size: 18),
          ],
        ),
      ),
    );
  }

  Future<void> _copyToClipboard(String text) async {
    await Clipboard.setData(ClipboardData(text: text));

    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Copied to clipboard'.tr),
          backgroundColor: AppColors.success,
          duration: Duration(seconds: 1),
        ),
      );
    }
  }

  Future<void> _openLink(String url) async {
    final uri = Uri.tryParse(url);
    if (uri == null) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Invalid link'.tr),
            backgroundColor: AppColors.error,
            duration: Duration(seconds: 2),
          ),
        );
      }
      return;
    }

    try {
      final launched = await launchUrl(
        uri,
        mode: LaunchMode.externalApplication,
      );
      if (!launched) {
        await _copyToClipboard(url);
      }
    } catch (_) {
      if (mounted) {
        await _copyToClipboard(url);
      }
    }
  }
}
