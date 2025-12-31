import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../ui/atoms/p_button.dart';
import '../../design/theme.dart';
import '../../design/compat.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/api.dart' as api;

/// Verify My Build Screen - Shows reproducible build verification steps
class VerifyBuildScreen extends ConsumerStatefulWidget {
  const VerifyBuildScreen({Key? key}) : super(key: key);

  @override
  ConsumerState<VerifyBuildScreen> createState() => _VerifyBuildScreenState();
}

class _VerifyBuildScreenState extends ConsumerState<VerifyBuildScreen> {
  Map<String, String>? _buildInfo;
  bool _isLoading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadBuildInfo();
  }

  Future<void> _loadBuildInfo() async {
    try {
      final info = await api.getBuildInfo();

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
    } catch (e) {
      setState(() {
        _error = 'Failed to load build info: ${e.toString()}';
        _isLoading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Verify My Build',
      appBar: const PAppBar(
        title: 'Verify My Build',
        subtitle: 'Reproducible build checklist (legacy showcase)',
        showBackButton: true,
      ),
      body: Container(
        color: Colors.black,
        child: _isLoading
            ? const Center(child: CircularProgressIndicator())
            : SingleChildScrollView(
                padding: EdgeInsets.all(PirateSpacing.xxl),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _buildHeader(),
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
    return Padding(
      padding: EdgeInsets.only(bottom: PirateSpacing.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 100,
            child: Text(
              label,
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
    final commit = _buildInfo?['gitCommit'] ?? 'unknown';
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
          'Follow these steps to verify this build matches our official source:',
          style: PirateTypography.body.copyWith(color: Colors.grey[400]),
        ),
        SizedBox(height: PirateSpacing.lg),
        _buildStep(
          number: 1,
          title: 'Install Nix with Flakes',
          description: 'Install Nix package manager with flakes support:\n'
              'sh <(curl -L https://nixos.org/nix/install) --daemon\n'
              'Enable flakes in ~/.config/nix/nix.conf',
          code: 'experimental-features = nix-command flakes',
        ),
        _buildStep(
          number: 2,
          title: 'Clone Repository',
          description: 'Clone the Pirate Unified Wallet repository and checkout the commit:',
          code: 'git clone https://github.com/pirate/wallet.git\n'
              'cd wallet\n'
              'git checkout $commit',
        ),
        _buildStep(
          number: 3,
          title: 'Build with Nix',
          description: 'Build the application using our Nix flake:',
          code: _getBuildCommand(),
        ),
        _buildStep(
          number: 4,
          title: 'Compare Hashes',
          description: 'Generate and compare the SHA-256 hash of your build:',
          code: _getHashCommand(),
        ),
        _buildStep(
          number: 5,
          title: 'Verify SBOM',
          description: 'Check the Software Bill of Materials (SBOM) and Sigstore provenance:',
          code: 'cat result/sbom.json | jq .\n'
              'cosign verify-attestation result/provenance.json',
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
          url: 'https://pirate.black/docs/verify',
        ),
        SizedBox(height: PirateSpacing.sm),
        _buildLinkCard(
          icon: Icons.code,
          title: 'Source Code',
          description: 'View the full source code on GitHub',
          url: 'https://github.com/pirate/wallet',
        ),
        SizedBox(height: PirateSpacing.sm),
        _buildLinkCard(
          icon: Icons.security,
          title: 'Security Practices',
          description: 'Learn about our security model',
          url: 'https://pirate.black/security',
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
      onTap: () => _copyToClipboard(url),
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

  String _getBuildCommand() {
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

  String _getHashCommand() {
    final platform = _buildInfo?['targetTriple'] ?? 'unknown';
    
    if (platform.contains('android')) {
      return 'sha256sum result/*.apk\n# Compare with official release hash';
    } else if (platform.contains('ios')) {
      return 'shasum -a 256 result/*.ipa\n# Compare with official release hash';
    } else if (platform.contains('windows')) {
      return 'certutil -hashfile result/*.msix SHA256\n# Compare with official release hash';
    } else if (platform.contains('darwin')) {
      return 'shasum -a 256 result/*.dmg\n# Compare with official release hash';
    } else {
      return 'sha256sum result/*.AppImage\n# Compare with official release hash';
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
}
