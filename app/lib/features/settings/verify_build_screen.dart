// The release tag is injected by the packaging workflow so prerelease builds
// verify against their exact GitHub release rather than a nearby stable tag.
// ignore_for_file: do_not_use_environment

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/security/release_verification_service.dart';
import '../../ui/atoms/p_button.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/generated/api.dart' as api;
import 'providers/preferences_providers.dart';
import '../../core/i18n/arb_text_localizer.dart';

typedef VerifyBuildInfoLoader = Future<Map<String, String>> Function();
typedef VerifyReleaseRunner = Future<ReleaseVerificationResult> Function(
  String appVersion,
  String embeddedReleaseTag,
);

/// Verify My Build Screen - Shows reproducible build verification steps
class VerifyBuildScreen extends ConsumerStatefulWidget {
  const VerifyBuildScreen({
    super.key,
    this.buildInfoLoader,
    this.releaseVerifier,
  });

  final VerifyBuildInfoLoader? buildInfoLoader;
  final VerifyReleaseRunner? releaseVerifier;

  @override
  ConsumerState<VerifyBuildScreen> createState() => _VerifyBuildScreenState();
}

class _VerifyBuildScreenState extends ConsumerState<VerifyBuildScreen> {
  static const _embeddedReleaseTag = String.fromEnvironment(
    'PIRATE_RELEASE_TAG',
  );
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
      final buildInfo = await (widget.buildInfoLoader ?? _readBuildInfo)();

      if (!mounted) return;
      setState(() {
        _buildInfo = buildInfo;
        _error = null;
        _isLoading = false;
      });
      await _checkReleaseVerification();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = 'Failed to load build info: {error}'.trArgs({'error': e});
        _isLoading = false;
      });
    }
  }

  Future<Map<String, String>> _readBuildInfo() async {
    final info = await api.getBuildInfo();
    final packageInfo = await PackageInfo.fromPlatform();
    return {
      'version': packageInfo.version,
      'gitCommit': info.gitCommit,
      'buildDate': info.buildDate,
      'rustVersion': info.rustVersion,
      'targetTriple': info.targetTriple,
    };
  }

  Future<void> _checkReleaseVerification() async {
    if (kIsWeb) {
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage =
            'Release verification is not supported on web.'.tr;
      });
      return;
    }

    if (!ref.read(allowGithubApisProvider)) {
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage = 'Outbound GitHub checks are disabled in Settings > Outbound API Calls.'
            .tr;
      });
      return;
    }

    final version = _buildInfo?['version'];
    if (version == null || version.isEmpty) {
      setState(() {
        _verificationStatus = ReleaseVerificationStatus.error;
        _verificationMessage = 'Build information unavailable.'.tr;
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

    final result = widget.releaseVerifier == null
        ? await ReleaseVerificationService().verify(
            version,
            embeddedReleaseTag: _embeddedReleaseTag,
          )
        : await widget.releaseVerifier!(version, _embeddedReleaseTag);
    if (!mounted) return;
    setState(() {
      _verificationStatus = result.status;
      _verificationMessage = _verificationMessageFor(result.reason);
      _releaseTag = result.releaseTag;
      _releaseUrl = result.releaseUrl;
      _checksumAssetName = result.checksumAssetName;
      _signatureAssetName = result.signatureAssetName;
      _localArtifactPath = result.localArtifactPath;
      _localArtifactName = result.localArtifactName;
      _localHash = result.localHash;
      _expectedHash = result.expectedHash;
      _matchedChecksumName = result.matchedChecksumName;
    });
  }

  Color _statusColor(ReleaseVerificationStatus status) {
    switch (status) {
      case ReleaseVerificationStatus.match:
        return AppColors.success;
      case ReleaseVerificationStatus.mismatch:
        return AppColors.error;
      case ReleaseVerificationStatus.checking:
        return AppColors.warning;
      case ReleaseVerificationStatus.unavailable:
        return AppColors.warning;
      case ReleaseVerificationStatus.error:
        return AppColors.error;
      case ReleaseVerificationStatus.noRelease:
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
        return 'Match'.tr;
      case ReleaseVerificationStatus.mismatch:
        return 'Mismatch'.tr;
      case ReleaseVerificationStatus.checking:
        return 'Checking'.tr;
      case ReleaseVerificationStatus.unavailable:
        return 'Check unavailable'.tr;
      case ReleaseVerificationStatus.noRelease:
        return 'No Releases'.tr;
      case ReleaseVerificationStatus.noMatchingChecksum:
        return 'Unverified Build'.tr;
      case ReleaseVerificationStatus.noLocalArtifact:
        return 'No Local Artifact'.tr;
      case ReleaseVerificationStatus.error:
        return 'Error'.tr;
      case ReleaseVerificationStatus.idle:
        return 'Not Checked'.tr;
    }
  }

  String _verificationMessageFor(ReleaseVerificationReason reason) {
    switch (reason) {
      case ReleaseVerificationReason.none:
        return 'This installed app matches the PGP-signed official release manifest.'
            .tr;
      case ReleaseVerificationReason.releaseFilesUnavailable:
        return 'Official verification files are not available for this version.'
            .tr;
      case ReleaseVerificationReason.downloadFailed:
        return 'GitHub could not be reached through the current network connection. This does not mean the app failed verification. Check Network Privacy and try again.'
            .tr;
      case ReleaseVerificationReason.networkModeUnsupported:
        return 'GitHub release files cannot be reached through the current network mode. Switch Network Privacy to Tor, SOCKS5, or Direct, then try again.'
            .tr;
      case ReleaseVerificationReason.localArtifactUnavailable:
        return 'This platform does not give the app access to the installed package. Verify the downloaded file with the checksums and PGP signatures on the release page.'
            .tr;
      case ReleaseVerificationReason.checksumNotPublished:
        return 'The signed release does not contain a checksum for this installed payload.'
            .tr;
      case ReleaseVerificationReason.checksumMismatch:
        return 'This installed app does not match the PGP-signed official release manifest. Do not use it with funds.'
            .tr;
      case ReleaseVerificationReason.signatureInvalid:
        return 'The checksum manifest signature is invalid. Do not trust this release.'
            .tr;
      case ReleaseVerificationReason.invalidVerificationFiles:
        return 'Official verification files are invalid or incomplete.'.tr;
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
    final headerStatus = _headerStatus;
    return Column(
      children: [
        Container(
          width: 80,
          height: 80,
          decoration: BoxDecoration(
            color: headerStatus.color.withValues(alpha: 0.12),
            shape: BoxShape.circle,
            border: Border.all(
              color: headerStatus.color.withValues(alpha: 0.35),
              width: 2,
            ),
          ),
          child: Icon(headerStatus.icon, size: 40, color: headerStatus.color),
        ),
        SizedBox(height: PSpacing.lg),
        Text(
          'Check your installation'.tr,
          style: PTypography.heading2(color: AppColors.textPrimary),
          textAlign: TextAlign.center,
        ),
        SizedBox(height: PSpacing.md),
        Text(
          'Compare this app with the PGP-signed files from the official release.'
              .tr,
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
        _verificationStatus == ReleaseVerificationStatus.noMatchingChecksum;
    final officialReleaseUrl =
        _releaseUrl ??
        'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/releases';

    return _buildSurfaceCard(
      title: 'Official Release Verification'.tr,
      subtitle: 'Verifies the PGP signature, then checks this installed app against the signed release manifest.'
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
          style: PTypography.bodySmall(color: statusColor)
              .copyWith(fontWeight: FontWeight.w700),
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
          _buildDataRow(
            label: 'Release'.tr,
            value: _releaseTag ?? 'Not found'.tr,
          ),
          _buildDataRow(
            label: 'Local Artifact'.tr,
            value: _localArtifactName ?? 'Unavailable'.tr,
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
          if (_hasTechnicalDetails) ...[
            SizedBox(height: PSpacing.sm),
            Theme(
              data: Theme.of(context).copyWith(
                dividerColor: Colors.transparent,
                splashColor: Colors.transparent,
                highlightColor: Colors.transparent,
              ),
              child: Material(
                color: Colors.transparent,
                child: ExpansionTile(
                  tilePadding: EdgeInsets.zero,
                  childrenPadding: EdgeInsets.zero,
                  iconColor: AppColors.textSecondary,
                  collapsedIconColor: AppColors.textSecondary,
                  title: Text(
                    'Technical details'.tr,
                    style: PTypography.bodyMedium(color: AppColors.textPrimary)
                        .copyWith(fontWeight: FontWeight.w600),
                  ),
                  subtitle: Text(
                    'Hashes, file paths, and signed manifest files'.tr,
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  children: [_buildTechnicalDetails()],
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
                      text: 'Verify now'.tr,
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
                        text: 'Copy Local Hash'.tr,
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
                      text: 'Verify now'.tr,
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
                        text: 'Copy Local Hash'.tr,
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
              text: 'Open Official Releases'.tr,
              variant: PButtonVariant.outline,
              fullWidth: true,
            ),
          ],
        ],
      ),
    );
  }

  ({Color color, IconData icon}) get _headerStatus {
    switch (_verificationStatus) {
      case ReleaseVerificationStatus.match:
        return (color: AppColors.success, icon: Icons.verified_user);
      case ReleaseVerificationStatus.mismatch:
      case ReleaseVerificationStatus.error:
        return (color: AppColors.error, icon: Icons.gpp_bad_outlined);
      case ReleaseVerificationStatus.checking:
        return (color: AppColors.accentPrimary, icon: Icons.shield_outlined);
      case ReleaseVerificationStatus.unavailable:
      case ReleaseVerificationStatus.noRelease:
      case ReleaseVerificationStatus.noLocalArtifact:
      case ReleaseVerificationStatus.noMatchingChecksum:
        return (color: AppColors.warning, icon: Icons.gpp_maybe_outlined);
      case ReleaseVerificationStatus.idle:
        return (color: AppColors.textTertiary, icon: Icons.shield_outlined);
    }
  }

  bool get _hasTechnicalDetails =>
      _releaseUrl != null ||
      _localArtifactPath != null ||
      _localHash != null ||
      _expectedHash != null ||
      _matchedChecksumName != null ||
      _checksumAssetName != null ||
      _signatureAssetName != null ||
      _lastCheckedAt != null;

  Widget _buildTechnicalDetails() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (_releaseUrl != null)
          _buildDataRow(
            label: 'Release URL'.tr,
            value: _releaseUrl!,
            copyable: true,
            monospace: true,
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
        if (_signatureAssetName != null) ...[
          SizedBox(height: PSpacing.sm),
          Text(
            'The release signature bundle includes the signed checksum manifest and official public key.'
                .tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
        ],
      ],
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
                      style: PTypography.bodyLarge(color: AppColors.textPrimary)
                          .copyWith(fontWeight: FontWeight.w700),
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
                      style: PTypography.bodySmall(color: AppColors.textPrimary)
                          .copyWith(fontFamily: monospace ? 'monospace' : null),
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
            value: _displayBuildValue(_buildInfo!['gitCommit']),
            copyable: true,
            monospace: true,
          ),
          _buildDataRow(
            label: 'Build Date'.tr,
            value: _displayBuildValue(_buildInfo!['buildDate']),
            monospace: true,
          ),
          _buildDataRow(
            label: 'Rust Version'.tr,
            value: _displayBuildValue(_buildInfo!['rustVersion']),
            monospace: true,
          ),
          _buildDataRow(
            label: 'Target'.tr,
            value: _displayBuildValue(_buildInfo!['targetTriple']),
            monospace: true,
          ),
        ],
      ),
    );
  }

  String _displayBuildValue(String? value) {
    if (value == null || value.trim().isEmpty || value == 'unknown') {
      return 'Not embedded in this build'.tr;
    }
    return value;
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
          url: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/blob/main/docs/verify-build.md',
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
                    style: PTypography.bodyMedium(color: AppColors.textPrimary)
                        .copyWith(fontWeight: FontWeight.w500),
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
