import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import '../../../design/compat.dart';
import '../../../design/tokens/colors.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/atoms/p_toggle.dart';
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
/// - Test node connection
class PrivacyShieldScreen extends ConsumerStatefulWidget {
  const PrivacyShieldScreen({super.key});

  @override
  ConsumerState<PrivacyShieldScreen> createState() => _PrivacyShieldScreenState();
}

class _PrivacyShieldScreenState extends ConsumerState<PrivacyShieldScreen> {
  final _torBridgeLinesController = TextEditingController();
  final _torTransportPathController = TextEditingController();
  final _i2pEndpointController = TextEditingController();
  final _storage = const FlutterSecureStorage();
  static const String _i2pWarningKey = 'i2p_first_use_ack';
  bool _isTestingConnection = false;
  bool _torBridgeFieldsInitialized = false;
  bool _i2pFieldsInitialized = false;
  bool _isSavingI2pEndpoint = false;
  bool _useTorBridges = false;
  bool _fallbackToTorBridges = true;
  String _torBridgeTransport = 'snowflake';
  String? _torBridgeError;
  String? _i2pEndpointError;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isLinux || Platform.isMacOS;

  @override
  void dispose() {
    _torBridgeLinesController.dispose();
    _torTransportPathController.dispose();
    _i2pEndpointController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final transportConfig = ref.watch(transportConfigProvider);
    final basePadding =
        PirateSpacing.screenPadding(MediaQuery.of(context).size.width);
    final padding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    final transportMode = transportConfig.mode;
    final dnsProvider = transportConfig.dnsProvider;
    final socks5Config = transportConfig.socks5Config;
    final torBridgeConfig = transportConfig.torBridge;
    if (!_torBridgeFieldsInitialized) {
      _useTorBridges = torBridgeConfig.useBridges;
      _fallbackToTorBridges = torBridgeConfig.fallbackToBridges;
      _torBridgeTransport = torBridgeConfig.transport;
      _torBridgeLinesController.text = torBridgeConfig.bridgeLines.join('\n');
      _torTransportPathController.text = torBridgeConfig.transportPath ?? '';
      _torBridgeFieldsInitialized = true;
    }
    if (!_i2pFieldsInitialized) {
      _i2pEndpointController.text = transportConfig.i2pEndpoint;
      _i2pFieldsInitialized = true;
    }

    return PScaffold(
      title: 'Privacy Shield',
      appBar: const PAppBar(
        title: 'Privacy Shield',
        subtitle: 'Network & tunneling',
      ),
      body: SingleChildScrollView(
        padding: padding,
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

            // I2P Settings (desktop only)
            if (transportMode == 'i2p' && _isDesktop) ...[
              _buildSectionTitle('I2P Endpoint'),
              const SizedBox(height: PirateSpacing.sm),
              _buildI2pEndpointSettings(context, ref),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // DNS Resolver
            _buildSectionTitle('DNS Resolver'),
            const SizedBox(height: 8),
            _buildDnsSelector(context, ref, dnsProvider),

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
          if (_isDesktop) ...[
            Divider(height: 1, color: AppColors.borderDefault),
            _buildModeOption(
              context,
              ref,
              'i2p',
              'I2P (Desktop Only)',
              'Embedded I2P router with ephemeral identity. First startup may take a few minutes.',
              Icons.router,
              AppColors.accentSecondary,
              currentMode == 'i2p',
            ),
          ],
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
        _handleTransportSelection(context, ref, mode);
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

  Future<void> _handleTransportSelection(
    BuildContext context,
    WidgetRef ref,
    String mode,
  ) async {
    if (mode == 'i2p') {
      final proceed = await _confirmI2pFirstUse(context);
      if (!proceed) return;
    }
    await ref.read(transportConfigProvider.notifier).setMode(mode);
  }

  Future<bool> _confirmI2pFirstUse(BuildContext context) async {
    final seen = await _storage.read(key: _i2pWarningKey);
    if (!context.mounted) {
      return false;
    }
    if (seen == 'true') {
      return true;
    }

    final proceed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('I2P First Startup'),
          content: const Text(
            'The embedded I2P router uses a fresh, ephemeral identity each run. '
            'The first startup can take a few minutes while it bootstraps. '
            'Keep the app open until it connects.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: const Text('Continue'),
            ),
          ],
        );
      },
    );

    if (proceed ?? false) {
      await _storage.write(key: _i2pWarningKey, value: 'true');
    }
    return proceed ?? false;
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

  Widget _buildI2pEndpointSettings(BuildContext context, WidgetRef ref) {
    final endpoint = _i2pEndpointController.text.trim();
    final showMissingWarning = endpoint.isEmpty;

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'I2P endpoints use .i2p hostnames (often ending in .b32.i2p).',
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 13,
            ),
          ),
          const SizedBox(height: PirateSpacing.sm),
          PInput(
            controller: _i2pEndpointController,
            label: 'I2P Lightwalletd Endpoint',
            hint: 'http://<base32>.b32.i2p:9067',
            helperText: 'Example: http://<hash>.b32.i2p:9067',
            errorText: _i2pEndpointError,
            autocorrect: false,
            enableSuggestions: false,
            monospace: true,
            onChanged: (_) {
              if (_i2pEndpointError != null) {
                setState(() {
                  _i2pEndpointError = null;
                });
              }
            },
          ),
          if (showMissingWarning) ...[
            const SizedBox(height: PirateSpacing.xs),
            Text(
              'No I2P endpoint set. I2P mode will stay offline until you add one.',
              style: TextStyle(
                color: AppColors.warning,
                fontSize: 12,
              ),
            ),
          ],
          const SizedBox(height: PirateSpacing.md),
          SizedBox(
            width: double.infinity,
            child: PButton(
              text: _isSavingI2pEndpoint ? 'Saving...' : 'Save I2P Endpoint',
              onPressed: _isSavingI2pEndpoint
                  ? null
                  : () async {
                      final candidate = _i2pEndpointController.text.trim();
                      if (candidate.isEmpty) {
                        setState(() {
                          _i2pEndpointError = 'Enter an .i2p endpoint.';
                        });
                        return;
                      }
                      if (!_isValidI2pEndpoint(candidate)) {
                        setState(() {
                          _i2pEndpointError =
                              'Endpoint must use a .i2p hostname.';
                        });
                        return;
                      }
                      setState(() {
                        _isSavingI2pEndpoint = true;
                      });
                      try {
                        await ref
                            .read(transportConfigProvider.notifier)
                            .setI2pEndpoint(candidate);
                        if (!context.mounted) return;
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(
                            content: Text('I2P endpoint saved.'),
                          ),
                        );
                      } catch (e) {
                        if (!context.mounted) return;
                        ScaffoldMessenger.of(context).showSnackBar(
                          SnackBar(
                            content: Text('Failed to save I2P endpoint: $e'),
                            backgroundColor: AppColors.error,
                          ),
                        );
                      } finally {
                        if (mounted) {
                          setState(() {
                            _isSavingI2pEndpoint = false;
                          });
                        }
                      }
                    },
              variant: PButtonVariant.secondary,
            ),
          ),
        ],
      ),
    );
  }

  bool _isValidI2pEndpoint(String value) {
    var normalized = value.trim();
    if (normalized.isEmpty) {
      return false;
    }
    if (normalized.startsWith('https://')) {
      normalized = normalized.substring(8);
    } else if (normalized.startsWith('http://')) {
      normalized = normalized.substring(7);
    }
    if (normalized.endsWith('/')) {
      normalized = normalized.substring(0, normalized.length - 1);
    }
    final host = normalized.split(':').first;
    return host.endsWith('.i2p');
  }

  Widget _buildTorSettings(BuildContext context, WidgetRef ref) {
    final torStatus = ref.watch(torStatusProvider);
    final isBootstrapping = torStatus.status == 'bootstrapping';
    final progress = torStatus.progress;
    final routingSummary = _torRoutingSummary();

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  'Tor Status',
                  style: TextStyle(
                    color: AppColors.textPrimary,
                    fontSize: 14,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ),
              Wrap(
                spacing: PirateSpacing.sm,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  _buildTorStatusIndicator(torStatus),
                  PTextButton(
                    text: 'Switch exit node',
                    compact: true,
                    onPressed: torStatus.isReady
                        ? _switchTorExit
                        : null,
                  ),
                ],
              ),
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
          const SizedBox(height: PirateSpacing.xs),
          Text(
            routingSummary,
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 12,
            ),
          ),
          if (isBootstrapping) ...[
            const SizedBox(height: PirateSpacing.md),
            LinearProgressIndicator(
              value: progress == null ? null : (progress.clamp(0, 100) / 100.0),
              minHeight: 6,
              backgroundColor: AppColors.backgroundSurface,
              color: AppColors.accentPrimary,
            ),
            const SizedBox(height: PirateSpacing.xs),
            Text(
              progress == null
                  ? 'Bootstrapping...'
                  : 'Bootstrapping... $progress%',
              style: TextStyle(
                color: AppColors.textSecondary,
                fontSize: 12,
              ),
            ),
            if (torStatus.blocked != null && torStatus.blocked!.isNotEmpty) ...[
              const SizedBox(height: PirateSpacing.xs),
              Text(
                'Blocked: ${torStatus.blocked}',
                style: TextStyle(
                  color: AppColors.warning,
                  fontSize: 12,
                ),
              ),
            ],
          ],
          if (torStatus.status == 'error' && torStatus.error != null) ...[
            const SizedBox(height: PirateSpacing.xs),
            Text(
              torStatus.error!,
              style: TextStyle(
                color: AppColors.error,
                fontSize: 12,
              ),
            ),
          ],
          if (_isDesktop) ...[
            const SizedBox(height: PirateSpacing.md),
            _buildTorAdvancedControls(context, ref),
          ],
        ],
      ),
    );
  }

  Widget _buildTorStatusIndicator(TorStatusDetails status) {
    Color color;
    String label;

    switch (status.status) {
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

  Future<void> _switchTorExit() async {
    try {
      await FfiBridge.rotateTorExit();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Switched Tor exit node. Reconnecting...')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Failed to switch exit node: $e'),
          backgroundColor: AppColors.error,
        ),
      );
    }
  }

  Widget _buildTorAdvancedControls(BuildContext context, WidgetRef ref) {
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      title: Text(
        'Advanced',
        style: TextStyle(
          color: AppColors.textPrimary,
          fontSize: 14,
          fontWeight: FontWeight.bold,
        ),
      ),
      subtitle: Text(
        'Bridges and transport overrides',
        style: TextStyle(
          color: AppColors.textSecondary,
          fontSize: 12,
        ),
      ),
      children: [
        _buildTorBridgeControls(context, ref),
      ],
    );
  }

  Widget _buildTorBridgeControls(BuildContext context, WidgetRef ref) {
    if (!_isDesktop) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Bridge transports (Snowflake/obfs4) are desktop-only.',
            style: TextStyle(
              color: AppColors.textSecondary,
              fontSize: 12,
            ),
          ),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        PToggle(
          value: _useTorBridges,
          label: 'Use bridges immediately',
          onChanged: (value) => setState(() {
            _useTorBridges = value;
          }),
        ),
        const SizedBox(height: PirateSpacing.xs),
        PToggle(
          value: _fallbackToTorBridges,
          label: 'Fallback to bridges if direct fails',
          onChanged: (value) => setState(() {
            _fallbackToTorBridges = value;
          }),
        ),
        const SizedBox(height: PirateSpacing.sm),
        DropdownButtonFormField<String>(
          initialValue: _torBridgeTransport,
          items: const [
            DropdownMenuItem(value: 'snowflake', child: Text('Snowflake')),
            DropdownMenuItem(value: 'obfs4', child: Text('obfs4')),
          ],
          onChanged: (value) {
            if (value == null) return;
            setState(() {
              _torBridgeTransport = value;
            });
          },
          decoration: const InputDecoration(
            labelText: 'Fallback bridge transport',
            filled: true,
            helperText: 'Only used if direct Tor fails.',
          ),
        ),
        const SizedBox(height: PirateSpacing.sm),
        PInput(
          controller: _torBridgeLinesController,
          label: 'Bridge lines',
          hint: _torBridgeTransport == 'snowflake'
              ? 'Leave blank to use bundled Snowflake bridges'
              : 'Paste one bridge line per row',
          helperText: 'One bridge per line. Used only for bridges/fallback.',
          maxLines: 4,
          monospace: true,
        ),
        const SizedBox(height: PirateSpacing.sm),
        PInput(
          controller: _torTransportPathController,
          label: 'Transport binary path (optional)',
          hint: 'Leave blank to use PATH',
          monospace: true,
        ),
        if (_torBridgeError != null) ...[
          const SizedBox(height: PirateSpacing.xs),
          Text(
            _torBridgeError!,
            style: TextStyle(
              color: AppColors.error,
              fontSize: 12,
            ),
          ),
        ],
        const SizedBox(height: PirateSpacing.sm),
        Row(
          children: [
            Expanded(
              child: PButton(
                text: 'Apply & Restart Tor',
                variant: PButtonVariant.secondary,
                onPressed: () => _applyTorBridgeSettings(ref),
              ),
            ),
            const SizedBox(width: PirateSpacing.sm),
            Expanded(
              child: PTextButton(
                text: 'Use Snowflake',
                onPressed: () => _applyTorBridgePreset(ref, 'snowflake'),
              ),
            ),
          ],
        ),
        const SizedBox(height: PirateSpacing.xs),
        Row(
          children: [
            Expanded(
              child: PTextButton(
                text: 'Use obfs4',
                onPressed: () => _applyTorBridgePreset(ref, 'obfs4'),
              ),
            ),
            const SizedBox(width: PirateSpacing.sm),
            Expanded(
              child: PTextButton(
                text: 'Disable Bridges',
                onPressed: () => _disableTorBridges(ref),
              ),
            ),
          ],
        ),
      ],
    );
  }

  List<String> _splitBridgeLines(String raw) {
    return raw
        .split(RegExp(r'\r?\n'))
        .map((line) => line.trim())
        .where((line) => line.isNotEmpty)
        .toList();
  }

  String _torTransportLabel(String transport) {
    final normalized = transport.trim().toLowerCase();
    if (normalized.isEmpty || normalized == 'snowflake') {
      return 'Snowflake';
    }
    if (normalized == 'obfs4') {
      return 'obfs4';
    }
    return transport;
  }

  String _torRoutingSummary() {
    final transportLabel = _torTransportLabel(_torBridgeTransport);
    if (_useTorBridges) {
      return 'Attempting: $transportLabel (bridges)';
    }
    if (_fallbackToTorBridges) {
      return 'Attempting: Direct -> Fallback: $transportLabel';
    }
    return 'Attempting: Direct (no fallback bridges)';
  }

  Future<void> _applyTorBridgeSettings(WidgetRef ref) async {
    if (!_isDesktop) {
      setState(() {
        _torBridgeError = 'Bridge transports are desktop-only.';
      });
      return;
    }

    final lines = _splitBridgeLines(_torBridgeLinesController.text);
    if ((_useTorBridges || _fallbackToTorBridges) &&
        _torBridgeTransport == 'obfs4' &&
        lines.isEmpty) {
      setState(() {
        _torBridgeError = 'obfs4 requires bridge lines from a provider.';
      });
      return;
    }

    setState(() {
      _torBridgeError = null;
    });

    final path = _torTransportPathController.text.trim();
    final config = TorBridgeConfig(
      useBridges: _useTorBridges,
      fallbackToBridges: _fallbackToTorBridges,
      transport: _torBridgeTransport,
      bridgeLines: lines,
      transportPath: path.isEmpty ? null : path,
    );

    await ref.read(transportConfigProvider.notifier).setTorBridgeConfig(
          config,
          apply: true,
        );
  }

  Future<void> _applyTorBridgePreset(WidgetRef ref, String transport) async {
    setState(() {
      _useTorBridges = true;
      _fallbackToTorBridges = true;
      _torBridgeTransport = transport;
      _torBridgeError = null;
    });
    await _applyTorBridgeSettings(ref);
  }

  Future<void> _disableTorBridges(WidgetRef ref) async {
    setState(() {
      _useTorBridges = false;
      _fallbackToTorBridges = false;
      _torBridgeError = null;
    });
    await _applyTorBridgeSettings(ref);
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
          children: [
            Expanded(
              child: Text(
                label,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color:
                      isSelected ? AppColors.accentPrimary : AppColors.textPrimary,
                  fontSize: 14,
                ),
              ),
            ),
            if (isSelected)
              Icon(Icons.check, color: AppColors.accentPrimary, size: 20),
          ],
        ),
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
      final tlsPin = endpointConfig.tlsPin?.trim();
      final normalizedPin = tlsPin == null || tlsPin.isEmpty ? null : tlsPin;

      // Test the node connection
      final result = await FfiBridge.testNode(
        url: url,
        tlsPin: normalizedPin,
      );

      if (!context.mounted) return;

      // Show result dialog
      if (result.success) {
        _showSuccessDialog(context, result);
      } else {
        _showFailureDialog(context, result);
      }
    } catch (e) {
      if (!context.mounted) return;
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
    showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(Icons.check_circle, color: Colors.green, size: 28),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                'Connection Successful',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: AppColors.textPrimary),
              ),
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
    
    showDialog<void>(
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
    showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(Icons.error_outline, color: Colors.orange, size: 28),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                'Error',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: AppColors.textPrimary),
              ),
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
        children: [
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                color: AppColors.textSecondary,
                fontSize: 13,
              ),
            ),
          ),
          const SizedBox(width: PirateSpacing.sm),
          Expanded(
            child: Text(
              value,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.right,
              style: TextStyle(
                color: valueColor ?? AppColors.textPrimary,
                fontSize: 13,
                fontWeight: FontWeight.w500,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
