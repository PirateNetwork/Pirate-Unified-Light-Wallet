import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../ui/atoms/p_button.dart';
import '../../design/theme.dart';
import '../../design/compat.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/generated/api.dart' as api;

/// Verify My Build Screen - Shows reproducible build verification steps
class VerifyBuildScreen extends ConsumerStatefulWidget {
  const VerifyBuildScreen({Key? key}) : super(key: key);

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
  const _ReleaseAsset({
    required this.name,
    required this.url,
  });

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
  const _ChecksumResult({
    required this.entries,
    required this.sourceName,
  });

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
        _error = 'Failed to load build info: ${e.toString()}';
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
      _lastCheckedAt = DateTime.now();
    });

    try {
      final localArtifact = await _loadLocalArtifact();
      if (!mounted) return;
      setState(() {
        _localArtifactPath = localArtifact?.path;
        _localArtifactName = localArtifact?.name;
        _localHash = localArtifact?.sha256;
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
              'Release found, but no checksum assets were published.';
        });
        return;
      }

      if (localArtifact == null) {
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noLocalArtifact;
          _verificationMessage =
              'Local build artifact could not be accessed on this platform.';
        });
        return;
      }

      final expectedHash = checksums.entries[localArtifact.name];
      if (expectedHash == null) {
        setState(() {
          _verificationStatus = ReleaseVerificationStatus.noMatchingChecksum;
          _verificationMessage =
              'No published checksum matches the local executable name.';
        });
        return;
      }

      final normalizedExpected = _normalizeHash(expectedHash);
      final normalizedLocal = _normalizeHash(localArtifact.sha256);
      final matched = normalizedExpected == normalizedLocal;

      setState(() {
        _expectedHash = expectedHash;
        _verificationStatus = matched
            ? ReleaseVerificationStatus.match
            : ReleaseVerificationStatus.mismatch;
        _verificationMessage = matched
            ? 'Local build hash matches the published release.'
            : 'Local build hash does not match the published release.';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage = 'Verification failed: ${e.toString()}';
      });
    }
  }

  Future<_LocalArtifact?> _loadLocalArtifact() async {
    try {
      final path = Platform.resolvedExecutable;
      if (path.isEmpty) return null;
      final file = File(path);
      if (!await file.exists()) return null;

      final hash = await _hashFile(file);
      return _LocalArtifact(
        path: path,
        name: path.split(Platform.pathSeparator).last,
        sha256: hash,
      );
    } catch (_) {
      return null;
    }
  }

  Future<String> _hashFile(File file) async {
    final digest = await sha256.bind(file.openRead()).first;
    return digest.toString();
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

      return data.map<_ReleaseInfo>((entry) {
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

  bool _isChecksumAsset(String name) {
    final lower = name.toLowerCase();
    return lower.endsWith('.sha256') ||
        lower.endsWith('.sha256sum') ||
        lower.contains('checksums');
  }

  Future<_ChecksumResult> _fetchChecksums(List<_ReleaseAsset> assets) async {
    final checksumAssets = assets.where((asset) => _isChecksumAsset(asset.name));
    final entries = <String, String>{};
    String? sourceName;

    for (final asset in checksumAssets) {
      final text = await _downloadText(asset.url);
      final parsed = _parseChecksums(text, asset.name);
      if (parsed.isNotEmpty) {
        entries.addAll(parsed);
        sourceName ??= asset.name;
      }
    }

    return _ChecksumResult(entries: entries, sourceName: sourceName);
  }

  Future<String> _downloadText(String url) async {
    final client = HttpClient();
    try {
      final request = await client.getUrl(Uri.parse(url));
      request.headers.set('User-Agent', 'PirateWallet');
      final response = await request.close();
      if (response.statusCode >= 400) {
        throw Exception('Failed to download checksum file');
      }
      return response.transform(utf8.decoder).join();
    } finally {
      client.close(force: true);
    }
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
        final fallbackName = assetName
            .replaceAll(RegExp(r'\.sha256(sum)?$', caseSensitive: false), '');
        map[fallbackName] = parts.first;
        continue;
      }

      final hash = parts.first;
      final filename = parts.sublist(1).join(' ').replaceFirst('*', '');
      if (hash.isNotEmpty && filename.isNotEmpty) {
        map[filename] = hash;
      }
    }
    return map;
  }

  String _normalizeHash(String value) {
    return value.trim().toLowerCase();
  }

  Color _statusColor(ReleaseVerificationStatus status) {
    switch (status) {
      case ReleaseVerificationStatus.match:
        return Colors.green;
      case ReleaseVerificationStatus.mismatch:
        return Colors.red;
      case ReleaseVerificationStatus.checking:
        return Colors.orange;
      case ReleaseVerificationStatus.error:
        return Colors.red;
      case ReleaseVerificationStatus.noRelease:
      case ReleaseVerificationStatus.noChecksums:
      case ReleaseVerificationStatus.noMatchingChecksum:
      case ReleaseVerificationStatus.noLocalArtifact:
        return Colors.amber;
      case ReleaseVerificationStatus.idle:
        return Colors.grey;
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
        return 'No Hash';
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
    final two = (int value) => value.toString().padLeft(2, '0');
    return '${local.year}-${two(local.month)}-${two(local.day)} '
        '${two(local.hour)}:${two(local.minute)}';
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Verify My Build',
      appBar: const PAppBar(
        title: 'Verify My Build',
        subtitle: 'Release verification and reproducible build steps',
        showBackButton: true,
      ),
      body: Container(
        color: Colors.black,
        child: _isLoading
            ? const Center(child: CircularProgressIndicator())
            : SingleChildScrollView(
                padding: PirateSpacing.screenPadding(
                  MediaQuery.of(context).size.width,
                  vertical: PirateSpacing.xl,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _buildHeader(),
                    SizedBox(height: PirateSpacing.xl),
                    _buildReleaseVerificationCard(),
                    if (_error != null) ...[
                      SizedBox(height: PirateSpacing.lg),
                      Container(
                        padding: EdgeInsets.all(PirateSpacing.md),
                        decoration: BoxDecoration(
                          color: Colors.red.withValues(alpha: 0.1),
                          border: Border.all(color: Colors.red.withValues(alpha: 0.3)),
                          borderRadius: BorderRadius.circular(12),
                        ),
                        child: Text(
                          _error!,
                          style: PirateTypography.body.copyWith(color: Colors.red[300]),
                          textAlign: TextAlign.center,
                        ),
                      ),
                    ],
                    SizedBox(height: PirateSpacing.xl),
                    _buildBuildInfoCard(),
                    SizedBox(height: PirateSpacing.xl),
                    _buildVerificationSteps(),
                    SizedBox(height: PirateSpacing.xl),
                    _buildResourceLinks(),
                  ],
                ),
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
            color: Colors.green.withValues(alpha: 0.1),
            shape: BoxShape.circle,
            border: Border.all(color: Colors.green.withValues(alpha: 0.3), width: 2),
          ),
          child: Icon(
            Icons.verified_user,
            size: 40,
            color: Colors.green,
          ),
        ),
        SizedBox(height: PirateSpacing.lg),
        Text(
          'Reproducible Builds',
          style: PirateTypography.h2.copyWith(color: Colors.white),
          textAlign: TextAlign.center,
        ),
        SizedBox(height: PirateSpacing.md),
        Text(
          'Verify that this app matches our official source code',
          style: PirateTypography.body.copyWith(color: Colors.grey[400]),
          textAlign: TextAlign.center,
        ),
      ],
    );
  }

  Widget _buildReleaseVerificationCard() {
    final statusColor = _statusColor(_verificationStatus);
    final statusLabel = _statusLabel(_verificationStatus);
    final statusBackground = statusColor.withValues(alpha: 0.15);

    return Container(
      padding: EdgeInsets.all(PirateSpacing.lg),
      decoration: BoxDecoration(
        color: Colors.grey[900],
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: Colors.grey[800]!),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  'Official Release Verification',
                  style: PirateTypography.bodyLarge.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Container(
                padding: EdgeInsets.symmetric(
                  horizontal: PirateSpacing.sm,
                  vertical: PirateSpacing.xs,
                ),
                decoration: BoxDecoration(
                  color: statusBackground,
                  borderRadius: BorderRadius.circular(999),
                  border: Border.all(color: statusColor.withValues(alpha: 0.4)),
                ),
                child: Text(
                  statusLabel,
                  style: PirateTypography.bodySmall.copyWith(
                    color: statusColor,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
          SizedBox(height: PirateSpacing.sm),
          Text(
            'Checks the latest GitHub release for published hashes and compares them to this build.',
            style: PirateTypography.bodySmall.copyWith(color: Colors.grey[500]),
          ),
          if (_verificationStatus == ReleaseVerificationStatus.checking) ...[
            SizedBox(height: PirateSpacing.md),
            LinearProgressIndicator(
              minHeight: 2,
              color: PirateTheme.accentColor,
              backgroundColor: Colors.grey[800],
            ),
          ],
          SizedBox(height: PirateSpacing.lg),
          _buildVerificationRow('Release', _releaseTag ?? 'Not found'),
          if (_releaseUrl != null)
            _buildVerificationRow(
              'Release URL',
              _releaseUrl!,
              copyable: true,
              monospace: true,
            ),
          _buildVerificationRow(
            'Local Artifact',
            _localArtifactName ?? 'Unavailable',
          ),
          if (_localArtifactPath != null)
            _buildVerificationRow(
              'Local Path',
              _localArtifactPath!,
              copyable: true,
              monospace: true,
            ),
          if (_localHash != null)
            _buildVerificationRow(
              'Local SHA256',
              _localHash!,
              copyable: true,
              monospace: true,
            ),
          if (_expectedHash != null)
            _buildVerificationRow(
              'Expected SHA256',
              _expectedHash!,
              copyable: true,
              monospace: true,
            ),
          if (_checksumAssetName != null)
            _buildVerificationRow(
              'Checksum Source',
              _checksumAssetName!,
              copyable: true,
              monospace: true,
            ),
          if (_signatureAssetName != null)
            _buildVerificationRow(
              'Signature Asset',
              _signatureAssetName!,
              copyable: true,
              monospace: true,
            ),
          if (_lastCheckedAt != null)
            _buildVerificationRow(
              'Last Checked',
              _formatTimestamp(_lastCheckedAt!),
            ),
          if (_verificationMessage != null) ...[
            SizedBox(height: PirateSpacing.md),
            Text(
              _verificationMessage!,
              style: PirateTypography.bodySmall.copyWith(
                color: statusColor.withValues(alpha: 0.9),
              ),
            ),
          ],
          SizedBox(height: PirateSpacing.lg),
          Row(
            children: [
              Expanded(
                child: PButton(
                  onPressed: _verificationStatus ==
                          ReleaseVerificationStatus.checking
                      ? null
                      : _checkReleaseVerification,
                  text: 'Check GitHub',
                  variant: PButtonVariant.outline,
                  loading: _verificationStatus ==
                      ReleaseVerificationStatus.checking,
                  fullWidth: true,
                ),
              ),
              if (_localHash != null) ...[
                SizedBox(width: PirateSpacing.md),
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
          ),
          if (_signatureAssetName != null) ...[
            SizedBox(height: PirateSpacing.md),
            Text(
              'Signature verification in-app requires the release signing key and will be enabled once official signing is published.',
              style: PirateTypography.bodySmall.copyWith(
                color: Colors.grey[500],
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildVerificationRow(
    String label,
    String value, {
    bool copyable = false,
    bool monospace = false,
  }) {
    final screenWidth = MediaQuery.of(context).size.width;
    final labelWidth = screenWidth < 360 ? 90.0 : 120.0;
    return Padding(
      padding: EdgeInsets.only(bottom: PirateSpacing.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxWidth: labelWidth),
            child: Text(
              label,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: PirateTypography.bodySmall.copyWith(color: Colors.grey[500]),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: PirateTypography.bodySmall.copyWith(
                color: Colors.white,
                fontFamily: monospace ? 'monospace' : null,
              ),
            ),
          ),
          if (copyable)
            IconButton(
              icon: Icon(Icons.copy, size: 16, color: Colors.grey[500]),
              padding: EdgeInsets.zero,
              constraints: BoxConstraints(),
              onPressed: () => _copyToClipboard(value),
            ),
        ],
      ),
    );
  }

  Widget _buildBuildInfoCard() {
    if (_buildInfo == null) {
      return Container(
        padding: EdgeInsets.all(PirateSpacing.lg),
        decoration: BoxDecoration(
          color: Colors.grey[900],
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: Colors.grey[800]!),
        ),
        child: Text(
          'Build information unavailable.',
          style: PirateTypography.body.copyWith(color: Colors.grey[400]),
        ),
      );
    }

    return Container(
      padding: EdgeInsets.all(PirateSpacing.lg),
      decoration: BoxDecoration(
        color: Colors.grey[900],
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: Colors.grey[800]!),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Build Information',
            style: PirateTypography.bodyLarge.copyWith(
              color: Colors.white,
              fontWeight: FontWeight.w600,
            ),
          ),
          SizedBox(height: PirateSpacing.md),
          _buildInfoRow('Version', _buildInfo!['version']!),
          _buildInfoRow('Git Commit', _buildInfo!['gitCommit']!, copyable: true),
          _buildInfoRow('Build Date', _buildInfo!['buildDate']!),
          _buildInfoRow('Rust Version', _buildInfo!['rustVersion']!),
          _buildInfoRow('Target', _buildInfo!['targetTriple']!),
        ],
      ),
    );
  }

  Widget _buildInfoRow(String label, String value, {bool copyable = false}) {
    final screenWidth = MediaQuery.of(context).size.width;
    final labelWidth = screenWidth < 360 ? 72.0 : 100.0;
    return Padding(
      padding: EdgeInsets.only(bottom: PirateSpacing.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxWidth: labelWidth),
            child: Text(
              label,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: PirateTypography.bodySmall.copyWith(color: Colors.grey[500]),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: PirateTypography.bodySmall.copyWith(
                color: Colors.white,
                fontFamily: 'monospace',
              ),
            ),
          ),
          if (copyable)
            IconButton(
              icon: Icon(Icons.copy, size: 16, color: Colors.grey[500]),
              padding: EdgeInsets.zero,
              constraints: BoxConstraints(),
              onPressed: () => _copyToClipboard(value),
            ),
        ],
      ),
    );
  }

  Widget _buildVerificationSteps() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Verification Steps',
          style: PirateTypography.h4.copyWith(
            color: Colors.white,
            fontWeight: FontWeight.w600,
          ),
        ),
        SizedBox(height: PirateSpacing.md),
        Text(
          'Use these steps to verify against the published release or reproduce locally:',
          style: PirateTypography.body.copyWith(color: Colors.grey[400]),
        ),
        SizedBox(height: PirateSpacing.lg),
        _buildStep(
          number: 1,
          title: 'Download Release + Checksums',
          description: 'Grab the official asset and its .sha256 file from GitHub Releases.',
          code: _getReleaseDownloadCommand(),
        ),
        _buildStep(
          number: 2,
          title: 'Verify Checksum',
          description: 'Compare the downloaded hash with the published checksum.',
          code: _getChecksumVerifyCommand(),
        ),
        _buildStep(
          number: 3,
          title: 'Reproduce with Nix Flake',
          description: 'Build the app using the pinned Nix flake.',
          code: _getNixBuildCommand(),
        ),
        _buildStep(
          number: 4,
          title: 'Generate SBOM (Optional)',
          description: 'Generate SBOMs (Rust + Flutter) into dist/sbom/',
          code: 'scripts/generate-sbom.sh dist/sbom',
        ),
        _buildStep(
          number: 5,
          title: 'Generate Provenance (Optional)',
          description: 'Generate provenance and signatures for a local artifact.',
          code: 'scripts/generate-provenance.sh <artifact> dist/provenance\n'
              'cosign verify-blob --bundle dist/provenance/<artifact>.sigstore.bundle <artifact>',
        ),
      ],
    );
  }

  Widget _buildStep({
    required int number,
    required String title,
    required String description,
    required String code,
  }) {
    return Container(
      margin: EdgeInsets.only(bottom: PirateSpacing.lg),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                width: 32,
                height: 32,
                decoration: BoxDecoration(
                  color: PirateTheme.accentColor.withValues(alpha: 0.2),
                  shape: BoxShape.circle,
                  border: Border.all(color: PirateTheme.accentColor, width: 2),
                ),
                child: Center(
                  child: Text(
                    '$number',
                    style: PirateTypography.body.copyWith(
                      color: PirateTheme.accentColor,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
              ),
              SizedBox(width: PirateSpacing.md),
              Expanded(
                child: Text(
                  title,
                  style: PirateTypography.bodyLarge.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
          SizedBox(height: PirateSpacing.sm),
          Padding(
            padding: EdgeInsets.only(left: 44),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  description,
                  style: PirateTypography.body.copyWith(color: Colors.grey[400]),
                ),
                SizedBox(height: PirateSpacing.md),
                Container(
                  padding: EdgeInsets.all(PirateSpacing.md),
                  decoration: BoxDecoration(
                    color: Colors.grey[900],
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: Colors.grey[800]!),
                  ),
                  child: Row(
                    children: [
                      Expanded(
                        child: SelectableText(
                          code,
                          style: PirateTypography.bodySmall.copyWith(
                            color: Colors.green[300],
                            fontFamily: 'monospace',
                          ),
                        ),
                      ),
                      IconButton(
                        icon: Icon(Icons.copy, size: 18, color: Colors.grey[500]),
                        onPressed: () => _copyToClipboard(code),
                      ),
                    ],
                  ),
                ),
              ],
            ),
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
          'Resources',
          style: PirateTypography.h4.copyWith(
            color: Colors.white,
            fontWeight: FontWeight.w600,
          ),
        ),
        SizedBox(height: PirateSpacing.md),
        _buildLinkCard(
          icon: Icons.article,
          title: 'Verification Guide',
          description: 'Complete documentation on reproducible builds',
          url: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/blob/main/docs/verify-build.md',
        ),
        SizedBox(height: PirateSpacing.sm),
        _buildLinkCard(
          icon: Icons.code,
          title: 'Source Code',
          description: 'View the full source code on GitHub',
          url: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet',
        ),
        SizedBox(height: PirateSpacing.sm),
        _buildLinkCard(
          icon: Icons.security,
          title: 'Security Practices',
          description: 'Learn about our security model',
          url: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/blob/main/docs/security.md',
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
        padding: EdgeInsets.all(PirateSpacing.md),
        decoration: BoxDecoration(
          color: Colors.grey[900],
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: Colors.grey[800]!),
        ),
        child: Row(
          children: [
            Icon(icon, color: PirateTheme.accentColor, size: 24),
            SizedBox(width: PirateSpacing.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: PirateTypography.body.copyWith(
                      color: Colors.white,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  Text(
                    description,
                    style: PirateTypography.bodySmall.copyWith(
                      color: Colors.grey[500],
                    ),
                  ),
                ],
              ),
            ),
            Icon(Icons.open_in_new, color: Colors.grey[600], size: 18),
          ],
        ),
      ),
    );
  }

  String _getReleaseDownloadCommand() {
    final releaseTag = _releaseTag ?? '<tag>';
    return 'gh release download $releaseTag -R PirateNetwork/Pirate-Unified-Light-Wallet\n'
        '# or\n'
        'curl -L -O <release-asset-url>\n'
        'curl -L -O <checksums-url>';
  }

  String _getChecksumVerifyCommand() {
    final platform = _buildInfo?['targetTriple'] ?? 'unknown';

    if (platform.contains('windows')) {
      return 'Get-FileHash <artifact> -Algorithm SHA256\n'
          'Get-Content <checksums>\n'
          '# Compare the hash entry for your artifact';
    }

    if (platform.contains('darwin')) {
      return 'shasum -a 256 -c <checksums>\n'
          '# Or verify manually:\n'
          'shasum -a 256 <artifact>';
    }

    return 'sha256sum -c <checksums>\n'
        '# Or verify manually:\n'
        'sha256sum <artifact>';
  }

  String _getNixBuildCommand() {
    final platform = _buildInfo?['targetTriple'] ?? 'unknown';

    if (platform.contains('android')) {
      return 'nix build .#android-apk\n# or\nnix build .#android-bundle';
    } else if (platform.contains('ios')) {
      return 'nix build .#ios-ipa';
    } else if (platform.contains('windows')) {
      return 'nix build .#windows-msix';
    } else if (platform.contains('darwin')) {
      return 'nix build .#macos-dmg';
    } else {
      return 'nix build .#linux-appimage\n# or\nnix build .#linux-deb';
    }
  }

  Future<void> _copyToClipboard(String text) async {
    await Clipboard.setData(ClipboardData(text: text));
    
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Copied to clipboard'),
          backgroundColor: Colors.green[700],
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
            content: Text('Invalid link'),
            backgroundColor: Colors.red[700],
            duration: Duration(seconds: 2),
          ),
        );
      }
      return;
    }

    try {
      final launched = await launchUrl(uri, mode: LaunchMode.externalApplication);
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
