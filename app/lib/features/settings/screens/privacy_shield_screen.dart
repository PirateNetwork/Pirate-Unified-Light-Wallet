import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../design/theme.dart';
import '../../../design/compat.dart';
import '../../../design/tokens/colors.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';
import '../providers/transport_providers.dart';

/// Privacy Shield settings screen
/// 
/// Allows users to configure:
/// - Transport mode (Tor/SOCKS5/Direct)
/// - SOCKS5 proxy settings
/// - DNS resolver
/// - TLS certificate pins (SPKI)
/// - Test node connection
class PrivacyShieldScreen extends ConsumerStatefulWidget {
  const PrivacyShieldScreen({Key? key}) : super(key: key);

  @override
  ConsumerState<PrivacyShieldScreen> createState() => _PrivacyShieldScreenState();
}

class _PrivacyShieldScreenState extends ConsumerState<PrivacyShieldScreen> {
  final _spkiPinController = TextEditingController();
  bool _isTestingConnection = false;

  @override
  void dispose() {
    _spkiPinController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final transportConfig = ref.watch(transportConfigProvider);
    final transportMode = transportConfig.mode;
    final dnsProvider = transportConfig.dnsProvider;
    final socks5Config = transportConfig.socks5Config;
    final tlsPins = transportConfig.tlsPins;
    final endpointConfig = ref.watch(lightdEndpointConfigProvider);

    // Initialize SPKI pin from current config
    final currentPin = endpointConfig.maybeWhen(
      data: (config) => config.tlsPin,
      orElse: () => null,
    );
    if (_spkiPinController.text.isEmpty && currentPin != null) {
      _spkiPinController.text = currentPin;
    }

    return PScaffold(
      title: 'Privacy Shield',
      appBar: const PAppBar(
        title: 'Privacy Shield',
        subtitle: 'Network & tunneling',
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Warning if using Direct mode
            if (transportMode == 'direct')
              _buildWarningCard(
                'Privacy Warning',
                'Direct connection mode is NOT PRIVATE. All network traffic can be monitored. Use Tor or SOCKS5 for privacy.',
                Icons.warning,
                Colors.red,
              ),

            const SizedBox(height: 16),

            // Transport Mode
            _buildSectionTitle('Transport Mode'),
            const SizedBox(height: 8),
            _buildTransportModeSelector(context, ref, transportMode),

            const SizedBox(height: 24),

            // SOCKS5 Settings (if mode is SOCKS5)
            if (transportMode == 'socks5') ...[
              _buildSectionTitle('SOCKS5 Proxy Configuration'),
              const SizedBox(height: PirateSpacing.sm),
              _buildSocks5Settings(context, ref, socks5Config),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // Tor Settings (if mode is Tor)
            if (transportMode == 'tor') ...[
              _buildSectionTitle('Tor Settings'),
              const SizedBox(height: PirateSpacing.sm),
              _buildTorSettings(context, ref),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // DNS Resolver
            _buildSectionTitle('DNS Resolver'),
            const SizedBox(height: 8),
            _buildDnsSelector(context, ref, dnsProvider),

            const SizedBox(height: PirateSpacing.lg),

            // TLS Certificate Pinning with SPKI input
            _buildSectionTitle('TLS Certificate Pinning'),
            const SizedBox(height: PirateSpacing.sm),
            _buildTlsPinningWithInput(context, ref, tlsPins),

            const SizedBox(height: PirateSpacing.xl),

            // Test Connection Button
            SizedBox(
              width: double.infinity,
              child: PButton(
                text: _isTestingConnection ? 'Testing...' : 'Test Node Connection',
                onPressed: _isTestingConnection ? null : () => _testNodeConnection(context, ref),
                variant: PButtonVariant.secondary,
              ),
            ),
            
            const SizedBox(height: PirateSpacing.sm),
            
            Text(
              'Tests connection to lightwalletd using current transport and TLS settings',
              style: TextStyle(
                color: AppColors.textSecondary,
                fontSize: 12,
              ),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildSectionTitle(String title) {
    return Text(
      title,
      style: TextStyle(
        color: AppColors.textPrimary,
        fontSize: 18,
        fontWeight: FontWeight.bold,
      ),
    );
  }

  Widget _buildWarningCard(
    String title,
    String message,
    IconData icon,
    Color color,
  ) {
    return PCard(
      child: Row(
        children: [
          Icon(icon, color: color, size: 32),
          const SizedBox(width: 16),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: TextStyle(
                    color: color,
                    fontSize: 16,
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  message,
                  style: TextStyle(
                    color: AppColors.textSecondary,
                    fontSize: 14,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildTransportModeSelector(
    BuildContext context,
    WidgetRef ref,
    String currentMode,
  ) {
    return PCard(
      child: Column(
        children: [
          _buildModeOption(
            context,
            ref,
            'tor',
            'Tor (Most Private)',
            'All traffic routed through Tor network. Slowest but most private.',
            Icons.security,
            AppColors.accentPrimary,
            currentMode == 'tor',
          ),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildModeOption(
            context,
            ref,
            'socks5',
            'SOCKS5 Proxy',
            'Route traffic through custom SOCKS5 proxy. Privacy depends on proxy.',
            Icons.vpn_lock,
            AppColors.accentSecondary,
            currentMode == 'socks5',
          ),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildModeOption(
            context,
            ref,
            'direct',
            'Direct (Not Private)',
            'Direct connection without privacy protection. NOT RECOMMENDED.',
            Icons.warning,
            Colors.red,
            currentMode == 'direct',
          ),
        ],
      ),
    );
  }

  Widget _buildModeOption(
    BuildContext context,
    WidgetRef ref,
    String mode,
    String title,
    String description,
    IconData icon,
    Color color,
    bool isSelected,
  ) {
    return InkWell(
      onTap: () {
        ref.read(transportConfigProvider.notifier).setMode(mode);
      },
      child: Padding(
        padding: const EdgeInsets.all(PirateSpacing.md),
        child: Row(
          children: [
            Icon(icon, color: color, size: 28),
            const SizedBox(width: 16),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: TextStyle(
                      color: isSelected ? AppColors.accentPrimary : AppColors.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    description,
                    style: TextStyle(
                      color: AppColors.textSecondary,
                      fontSize: 13,
                    ),
                  ),
                ],
              ),
            ),
            if (isSelected)
              Icon(Icons.check_circle, color: AppColors.accentPrimary),
          ],
        ),
      ),
    );
  }

  Widget _buildSocks5Settings(
    BuildContext context,
    WidgetRef ref,
    Map<String, String?> config,
  ) {
    return PCard(
      child: Column(
        children: [
          PInput(
            label: 'Host',
            value: config['host'] ?? '',
            onChanged: (value) {
              ref.read(transportConfigProvider.notifier).setSocks5Config({
                ...config,
                'host': value,
              });
            },
          ),
          const SizedBox(height: 16),
          PInput(
            label: 'Port',
            value: config['port'] ?? '1080',
            keyboardType: TextInputType.number,
            onChanged: (value) {
              ref.read(transportConfigProvider.notifier).setSocks5Config({
                ...config,
                'port': value,
              });
            },
          ),
          const SizedBox(height: 16),
          PInput(
            label: 'Username (Optional)',
            value: config['username'] ?? '',
            onChanged: (value) {
              ref.read(transportConfigProvider.notifier).setSocks5Config({
                ...config,
                'username': value.isEmpty ? null : value,
              });
            },
          ),
          const SizedBox(height: 16),
          PInput(
            label: 'Password (Optional)',
            value: config['password'] ?? '',
            obscureText: true,
            onChanged: (value) {
              ref.read(transportConfigProvider.notifier).setSocks5Config({
                ...config,
                'password': value.isEmpty ? null : value,
              });
            },
          ),
        ],
      ),
    );
  }

  Widget _buildTorSettings(BuildContext context, WidgetRef ref) {
    final torStatus = ref.watch(torStatusProvider);

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Tor Status',
                style: TextStyle(
                  color: AppColors.textPrimary,
                  fontSize: 14,
                  fontWeight: FontWeight.bold,
                ),
              ),
              _buildTorStatusIndicator(torStatus),
            ],
          ),
          const SizedBox(height: PirateSpacing.md),
          Text(
            'Tor provides the strongest privacy by routing traffic through multiple relays, making it very difficult to trace.',
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 13,
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildTorStatusIndicator(String status) {
    Color color;
    String label;

    switch (status) {
      case 'ready':
        color = Colors.green;
        label = 'Ready';
        break;
      case 'bootstrapping':
        color = Colors.orange;
        label = 'Bootstrapping...';
        break;
      case 'error':
        color = Colors.red;
        label = 'Error';
        break;
      default:
        color = Colors.grey;
        label = 'Not Started';
    }

    return Row(
      children: [
        Container(
          width: 12,
          height: 12,
          decoration: BoxDecoration(
            color: color,
            shape: BoxShape.circle,
          ),
        ),
        const SizedBox(width: 4),
        Text(
          label,
          style: TextStyle(
            color: color,
            fontSize: 13,
            fontWeight: FontWeight.w500,
          ),
        ),
      ],
    );
  }

  Widget _buildDnsSelector(
    BuildContext context,
    WidgetRef ref,
    String currentProvider,
  ) {
    return PCard(
      child: Column(
        children: [
          _buildDnsOption(ref, 'cloudflare_doh', 'Cloudflare (1.1.1.1)', currentProvider),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(ref, 'quad9_doh', 'Quad9 (9.9.9.9)', currentProvider),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(ref, 'google_doh', 'Google (8.8.8.8)', currentProvider),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(ref, 'system', 'System (Not Private)', currentProvider),
        ],
      ),
    );
  }

  Widget _buildDnsOption(
    WidgetRef ref,
    String provider,
    String label,
    String currentProvider,
  ) {
    final isSelected = provider == currentProvider;

    return InkWell(
      onTap: () {
        ref.read(transportConfigProvider.notifier).setDnsProvider(provider);
      },
      child: Padding(
        padding: const EdgeInsets.symmetric(
          horizontal: PirateSpacing.md,
          vertical: PirateSpacing.sm,
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text(
              label,
              style: TextStyle(
                color: isSelected ? AppColors.accentPrimary : AppColors.textPrimary,
                fontSize: 14,
              ),
            ),
            if (isSelected)
              Icon(Icons.check, color: AppColors.accentPrimary, size: 20),
          ],
        ),
      ),
    );
  }

  Widget _buildTlsPinningWithInput(
    BuildContext context,
    WidgetRef ref,
    List<Map<String, String>> pins,
  ) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Certificate pinning prevents man-in-the-middle attacks by verifying the server\'s public key hash (SPKI).',
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 13,
            ),
          ),
          const SizedBox(height: PirateSpacing.md),
          
          // SPKI Pin Input Field
          TextField(
            controller: _spkiPinController,
            decoration: InputDecoration(
              labelText: 'SPKI Pin (Base64 SHA-256)',
              labelStyle: TextStyle(color: AppColors.textSecondary),
              hintText: 'sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=',
              hintStyle: TextStyle(color: AppColors.textSecondary.withValues(alpha: 0.5)),
              filled: true,
              fillColor: AppColors.backgroundSurface,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(8),
                borderSide: BorderSide(color: AppColors.borderDefault),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(8),
                borderSide: BorderSide(color: AppColors.borderDefault),
              ),
              focusedBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(8),
                borderSide: BorderSide(color: AppColors.accentPrimary, width: 2),
              ),
              suffixIcon: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    icon: Icon(Icons.content_paste, color: AppColors.textSecondary),
                    onPressed: () async {
                      final data = await Clipboard.getData(Clipboard.kTextPlain);
                      if (data?.text != null) {
                        setState(() {
                          _spkiPinController.text = data!.text!.trim();
                        });
                      }
                    },
                    tooltip: 'Paste from clipboard',
                  ),
                  if (_spkiPinController.text.isNotEmpty)
                    IconButton(
                      icon: Icon(Icons.clear, color: AppColors.textSecondary),
                      onPressed: () {
                        setState(() {
                          _spkiPinController.clear();
                        });
                      },
                      tooltip: 'Clear',
                    ),
                ],
              ),
            ),
            style: TextStyle(
              color: AppColors.textPrimary,
              fontFamily: 'monospace',
              fontSize: 12,
            ),
            maxLines: 2,
            onChanged: (value) {
              setState(() {});
            },
          ),
          
          const SizedBox(height: PirateSpacing.sm),
          
          Text(
            'Format: sha256/<base64-hash> or just the base64 hash (44 characters)',
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 11,
            ),
          ),
          
          const SizedBox(height: PirateSpacing.md),
          
          // How to get SPKI pin
          ExpansionTile(
            tilePadding: EdgeInsets.zero,
            title: Text(
              'How to get the SPKI pin',
              style: TextStyle(
                color: AppColors.accentPrimary,
                fontSize: 13,
                fontWeight: FontWeight.w500,
              ),
            ),
            children: [
              Container(
                padding: EdgeInsets.all(PirateSpacing.sm),
                decoration: BoxDecoration(
                  color: AppColors.surface,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Run this command to extract the SPKI pin:',
                      style: TextStyle(
                        color: AppColors.textSecondary,
                        fontSize: 12,
                      ),
                    ),
                    const SizedBox(height: PirateSpacing.xs),
                    Container(
                      padding: EdgeInsets.all(PirateSpacing.sm),
                      decoration: BoxDecoration(
                        color: AppColors.backgroundBase,
                        borderRadius: BorderRadius.circular(4),
                      ),
                      child: SelectableText(
                        'openssl s_client -connect lightd1.piratechain.com:9067 2>/dev/null | \\\n'
                        '  openssl x509 -pubkey -noout | \\\n'
                        '  openssl pkey -pubin -outform DER | \\\n'
                        '  openssl dgst -sha256 -binary | \\\n'
                        '  openssl enc -base64',
                        style: TextStyle(
                          color: AppColors.textPrimary,
                          fontFamily: 'monospace',
                          fontSize: 10,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          
          const SizedBox(height: PirateSpacing.md),
          
          // Existing pins
          if (pins.isNotEmpty) ...[
            Text(
              'Configured Pins:',
              style: TextStyle(
                color: AppColors.textPrimary,
                fontSize: 13,
                fontWeight: FontWeight.w500,
              ),
            ),
            const SizedBox(height: PirateSpacing.xs),
            ...pins.map((pin) => _buildPinItem(pin)),
          ],
        ],
      ),
    );
  }

  Widget _buildPinItem(Map<String, String> pin) {
    return Padding(
      padding: const EdgeInsets.only(bottom: PirateSpacing.sm),
      child: Row(
        children: [
          Icon(Icons.verified_user, color: AppColors.accentPrimary, size: 16),
          const SizedBox(width: PirateSpacing.xs),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  pin['host'] ?? '',
                  style: TextStyle(
                    color: AppColors.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                  ),
                ),
                Text(
                  pin['description'] ?? '',
                  style: TextStyle(
                    color: AppColors.textSecondary,
                    fontSize: 11,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _testNodeConnection(BuildContext context, WidgetRef ref) async {
    setState(() {
      _isTestingConnection = true;
    });

    try {
      // Get current endpoint
      final endpointConfig = await ref.read(lightdEndpointConfigProvider.future);
      final url = endpointConfig.url;
      final tlsPin = _spkiPinController.text.trim().isEmpty 
          ? null 
          : _spkiPinController.text.trim();

      // Test the node connection
      final result = await FfiBridge.testNode(
        url: url,
        tlsPin: tlsPin,
      );

      if (!mounted) return;

      // Show result dialog
      if (result.success) {
        _showSuccessDialog(context, result);
      } else {
        _showFailureDialog(context, result);
      }
    } catch (e) {
      if (!mounted) return;
      _showErrorDialog(context, e.toString());
    } finally {
      if (mounted) {
        setState(() {
          _isTestingConnection = false;
        });
      }
    }
  }

  void _showSuccessDialog(BuildContext context, NodeTestResult result) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(Icons.check_circle, color: Colors.green, size: 28),
            const SizedBox(width: 12),
            Text(
              'Connection Successful',
              style: TextStyle(color: AppColors.textPrimary),
            ),
          ],
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _buildResultRow('Transport', '${result.transportIcon} ${result.transportMode.toUpperCase()}'),
            _buildResultRow('TLS', result.tlsEnabled ? 'Enabled ✓' : 'Disabled'),
            if (result.tlsPinMatched != null)
              _buildResultRow(
                'Pin Verified',
                result.tlsPinMatched! ? 'Yes ✓' : 'MISMATCH ✗',
                valueColor: result.tlsPinMatched! ? Colors.green : Colors.red,
              ),
            if (result.latestBlockHeight != null)
              _buildResultRow('Latest Block', '#${result.latestBlockHeight}')
            else
              _buildResultRow(
                'Latest Block',
                'Unavailable (Connection Failed)',
                valueColor: AppColors.error,
              ),
            _buildResultRow('Response Time', '${result.responseTimeMs}ms'),
            if (result.serverVersion != null)
              _buildResultRow('Server', result.serverVersion!),
            if (result.chainName != null)
              _buildResultRow('Chain', result.chainName!),
          ],
        ),
        actions: [
          PTextButton(
            label: 'OK',
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
    );
  }

  void _showFailureDialog(BuildContext context, NodeTestResult result) {
    final isPinMismatch = result.tlsPinMatched == false;
    
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(
              isPinMismatch ? Icons.gpp_bad : Icons.error,
              color: Colors.red,
              size: 28,
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                isPinMismatch ? 'Certificate Pin Mismatch' : 'Connection Failed',
                style: TextStyle(color: AppColors.textPrimary),
              ),
            ),
          ],
        ),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _buildResultRow('Transport', '${result.transportIcon} ${result.transportMode.toUpperCase()}'),
              _buildResultRow('TLS', result.tlsEnabled ? 'Enabled' : 'Disabled'),
              _buildResultRow('Response Time', '${result.responseTimeMs}ms'),
              
              const SizedBox(height: 16),
              
              if (isPinMismatch) ...[
                Container(
                  padding: EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.red.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: Colors.red.withValues(alpha: 0.3)),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '⚠️ Security Warning',
                        style: TextStyle(
                          color: Colors.red,
                          fontWeight: FontWeight.bold,
                          fontSize: 14,
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'The server certificate does not match the expected pin. '
                        'This could indicate:\n'
                        '• A man-in-the-middle attack\n'
                        '• The server certificate has been rotated\n'
                        '• An incorrect pin was entered',
                        style: TextStyle(
                          color: AppColors.textSecondary,
                          fontSize: 12,
                        ),
                      ),
                      if (result.expectedPin != null) ...[
                        const SizedBox(height: 8),
                        Text(
                          'Expected:',
                          style: TextStyle(
                            color: AppColors.textSecondary,
                            fontSize: 11,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                        SelectableText(
                          result.expectedPin!,
                          style: TextStyle(
                            color: AppColors.textPrimary,
                            fontFamily: 'monospace',
                            fontSize: 10,
                          ),
                        ),
                      ],
                      if (result.actualPin != null) ...[
                        const SizedBox(height: 8),
                        Text(
                          'Actual (from server):',
                          style: TextStyle(
                            color: AppColors.textSecondary,
                            fontSize: 11,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                        SelectableText(
                          result.actualPin!,
                          style: TextStyle(
                            color: AppColors.textPrimary,
                            fontFamily: 'monospace',
                            fontSize: 10,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ] else if (result.errorMessage != null) ...[
                Container(
                  padding: EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: AppColors.backgroundBase,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Icon(
                            Icons.error_outline,
                            color: AppColors.error,
                            size: 20,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              result.errorMessage!,
                              style: TextStyle(
                                color: AppColors.textSecondary,
                                fontSize: 13,
                              ),
                            ),
                          ),
                        ],
                      ),
                      if (result.latestBlockHeight == null) ...[
                        const SizedBox(height: 8),
                        Text(
                          '⚠️ Latest block height not retrieved - connection failed before data could be fetched.',
                          style: TextStyle(
                            color: Colors.orange,
                            fontSize: 11,
                            fontStyle: FontStyle.italic,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ],
          ),
        ),
        actions: [
          if (isPinMismatch && result.actualPin != null)
            PTextButton(
              label: 'Copy Actual Pin',
              leadingIcon: Icons.copy,
              variant: PTextButtonVariant.subtle,
              onPressed: () {
                Clipboard.setData(ClipboardData(text: result.actualPin!));
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(content: Text('Actual pin copied to clipboard')),
                );
              },
            ),
          PTextButton(
            label: 'OK',
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
    );
  }

  void _showErrorDialog(BuildContext context, String error) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(Icons.error_outline, color: Colors.orange, size: 28),
            const SizedBox(width: 12),
            Text(
              'Error',
              style: TextStyle(color: AppColors.textPrimary),
            ),
          ],
        ),
        content: Text(
          error,
          style: TextStyle(color: AppColors.textSecondary),
        ),
        actions: [
          PTextButton(
            label: 'OK',
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
    );
  }

  Widget _buildResultRow(String label, String value, {Color? valueColor}) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(
            label,
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 13,
            ),
          ),
          Text(
            value,
            style: TextStyle(
              color: valueColor ?? AppColors.textPrimary,
              fontSize: 13,
              fontWeight: FontWeight.w500,
            ),
          ),
        ],
      ),
    );
  }
}
