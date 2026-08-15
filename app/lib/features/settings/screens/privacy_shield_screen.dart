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
import '../../../ui/molecules/connection_status_indicator.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';
import '../providers/transport_providers.dart';
import '../../../core/i18n/arb_text_localizer.dart';

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
  ConsumerState<PrivacyShieldScreen> createState() =>
      _PrivacyShieldScreenState();
}

class _PrivacyShieldScreenState extends ConsumerState<PrivacyShieldScreen> {
  final _torBridgeLinesController = TextEditingController();
  final _torTransportPathController = TextEditingController();
  final _i2pEndpointController = TextEditingController();
  final _storage = const FlutterSecureStorage();
  static const String _i2pWarningKey = 'i2p_first_use_ack';
  bool _isTestingConnection = false;
  bool _isChangingTransport = false;
  bool _torBridgeFieldsInitialized = false;
  bool _i2pFieldsInitialized = false;
  String? _loadedI2pEndpoint;
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
    final isMobile = PirateSpacing.isHandset(MediaQuery.sizeOf(context));
    final transportConfig = ref.watch(transportConfigProvider);
    final basePadding = PirateSpacing.screenPadding(
      MediaQuery.of(context).size.width,
    );
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
    final canRefreshI2pField =
        !_i2pFieldsInitialized ||
        (_loadedI2pEndpoint != transportConfig.i2pEndpoint &&
            _i2pEndpointController.text == (_loadedI2pEndpoint ?? ''));
    if (canRefreshI2pField) {
      _i2pEndpointController.text = transportConfig.i2pEndpoint;
      _loadedI2pEndpoint = transportConfig.i2pEndpoint;
      _i2pFieldsInitialized = true;
    }

    return PScaffold(
      title: 'Privacy Shield'.tr,
      appBar: PAppBar(
        title: 'Privacy Shield'.tr,
        subtitle: 'Network & tunneling'.tr,
        actions: [ConnectionStatusIndicator(full: !isMobile)],
      ),
      body: SingleChildScrollView(
        padding: padding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Warning if using Direct mode
            if (transportMode == 'direct')
              _buildWarningCard(
                'Privacy Warning'.tr,
                'Direct mode exposes your IP address to the selected server.'
                    .tr,
                Icons.warning,
                Colors.red,
              ),

            const SizedBox(height: 16),

            // Transport Mode
            _buildSectionTitle('Transport Mode'.tr),
            const SizedBox(height: 8),
            _buildTransportModeSelector(context, ref, transportMode),

            const SizedBox(height: 24),

            // SOCKS5 Settings (if mode is SOCKS5)
            if (transportMode == 'socks5') ...[
              _buildSectionTitle('SOCKS5 Proxy Configuration'.tr),
              const SizedBox(height: PirateSpacing.sm),
              _buildSocks5Settings(context, ref, socks5Config),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // Tor Settings (if mode is Tor)
            if (transportMode == 'tor') ...[
              _buildSectionTitle('Tor Settings'.tr),
              const SizedBox(height: PirateSpacing.sm),
              _buildTorSettings(context, ref),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // I2P Settings (desktop only)
            if (transportMode == 'i2p' && _isDesktop) ...[
              _buildSectionTitle('I2P Endpoint'.tr),
              const SizedBox(height: PirateSpacing.sm),
              _buildI2pEndpointSettings(context, ref),
              const SizedBox(height: PirateSpacing.lg),
            ],

            // DNS Resolver
            _buildSectionTitle('DNS Resolver'.tr),
            const SizedBox(height: 8),
            _buildDnsSelector(context, ref, dnsProvider),

            const SizedBox(height: PirateSpacing.xl),

            // Test Connection Button
            SizedBox(
              width: double.infinity,
              child: PButton(
                text: _isTestingConnection
                    ? 'Testing...'.tr
                    : 'Test Node Connection'.tr,
                onPressed: _isTestingConnection
                    ? null
                    : () => _testNodeConnection(context, ref),
                variant: PButtonVariant.secondary,
              ),
            ),

            const SizedBox(height: PirateSpacing.sm),

            Text(
              'Tests connection to lightwalletd using current transport and TLS settings'
                  .tr,
              style: TextStyle(color: AppColors.textSecondary, fontSize: 12),
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
    final choices = <_TransportChoice>[
      _TransportChoice('tor', 'Tor'.tr, Icons.security_outlined),
      if (_isDesktop) _TransportChoice('i2p', 'I2P'.tr, Icons.router_outlined),
      _TransportChoice('socks5', 'SOCKS5'.tr, Icons.vpn_lock_outlined),
      _TransportChoice('direct', 'Direct'.tr, Icons.public_outlined),
    ];

    return PCard(
      padding: EdgeInsets.zero,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth / choices.length < 116;
          return SizedBox(
            height: compact ? 68 : 54,
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (var index = 0; index < choices.length; index++) ...[
                  if (index > 0)
                    Container(width: 1, color: AppColors.borderDefault),
                  Expanded(
                    child: _buildTransportSegment(
                      context,
                      ref,
                      choices[index],
                      compact: compact,
                      selected: currentMode == choices[index].mode,
                    ),
                  ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }

  Widget _buildTransportSegment(
    BuildContext context,
    WidgetRef ref,
    _TransportChoice choice, {
    required bool compact,
    required bool selected,
  }) {
    final foreground = selected
        ? AppColors.accentPrimary
        : choice.mode == 'direct'
        ? AppColors.warning
        : AppColors.textSecondary;
    final content = compact
        ? Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(choice.icon, color: foreground, size: 19),
              const SizedBox(height: 4),
              FittedBox(
                fit: BoxFit.scaleDown,
                child: Text(
                  choice.label,
                  maxLines: 1,
                  style: TextStyle(
                    color: foreground,
                    fontSize: 12,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                  ),
                ),
              ),
            ],
          )
        : Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(choice.icon, color: foreground, size: 20),
              const SizedBox(width: PirateSpacing.xs),
              Flexible(
                child: Text(
                  choice.label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: foreground,
                    fontSize: 13,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                  ),
                ),
              ),
            ],
          );

    return Semantics(
      button: true,
      selected: selected,
      label: choice.label,
      child: Material(
        color: selected
            ? AppColors.accentPrimary.withValues(alpha: 0.12)
            : Colors.transparent,
        child: InkWell(
          onTap: selected || _isChangingTransport
              ? null
              : () => _handleTransportSelection(context, ref, choice.mode),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: PirateSpacing.xs),
            child: content,
          ),
        ),
      ),
    );
  }

  Future<void> _handleTransportSelection(
    BuildContext context,
    WidgetRef ref,
    String mode,
  ) async {
    if (_isChangingTransport) return;
    if (mode == 'i2p') {
      final proceed = await _confirmI2pFirstUse(context);
      if (!proceed) return;
    }
    setState(() => _isChangingTransport = true);
    try {
      await ref.read(transportConfigProvider.notifier).setMode(mode);
    } finally {
      if (mounted) setState(() => _isChangingTransport = false);
    }
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
          title: Text('I2P First Startup'.tr),
          content: Text(
            'The embedded I2P router uses a fresh, ephemeral identity each '
                    'run. The first startup can take a few minutes while it '
                    'bootstraps. Keep the app open until it connects.'
                .tr,
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: Text('Cancel'.tr),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: Text('Continue'.tr),
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
            label: 'Host'.tr,
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
            label: 'Port'.tr,
            value: config['port'] ?? '1080',
            keyboardType: TextInputType.number,
            maxLength: 5,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            onChanged: (value) {
              ref.read(transportConfigProvider.notifier).setSocks5Config({
                ...config,
                'port': value,
              });
            },
          ),
          const SizedBox(height: 16),
          PInput(
            label: 'Username (Optional)'.tr,
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
            label: 'Password (Optional)'.tr,
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
            'I2P endpoints use .i2p hostnames (often ending in .b32.i2p).'.tr,
            style: TextStyle(color: AppColors.textSecondary, fontSize: 13),
          ),
          const SizedBox(height: PirateSpacing.sm),
          PInput(
            controller: _i2pEndpointController,
            label: 'I2P Lightwalletd Endpoint'.tr,
            hint: 'http://<base32>.b32.i2p:9067',
            helperText: 'Example: {endpoint}'.trArgs({
              'endpoint': 'http://<hash>.b32.i2p:9067',
            }),
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
              'No I2P endpoint set. I2P mode will stay offline until you add one.'
                  .tr,
              style: TextStyle(color: AppColors.warning, fontSize: 12),
            ),
          ],
          const SizedBox(height: PirateSpacing.md),
          SizedBox(
            width: double.infinity,
            child: PButton(
              text: _isSavingI2pEndpoint
                  ? 'Saving...'.tr
                  : 'Save I2P Endpoint'.tr,
              onPressed: _isSavingI2pEndpoint
                  ? null
                  : () async {
                      final candidate = _i2pEndpointController.text.trim();
                      if (candidate.isEmpty) {
                        setState(() {
                          _i2pEndpointError = 'Enter an .i2p endpoint.'.tr;
                        });
                        return;
                      }
                      if (!_isValidI2pEndpoint(candidate)) {
                        setState(() {
                          _i2pEndpointError =
                              'Endpoint must use a .i2p hostname.'.tr;
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
                          SnackBar(content: Text('I2P endpoint saved.'.tr)),
                        );
                      } catch (e) {
                        if (!context.mounted) return;
                        ScaffoldMessenger.of(context).showSnackBar(
                          SnackBar(
                            content: Text(
                              'Failed to save I2P endpoint: {error}'.trArgs({
                                'error': e,
                              }),
                            ),
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
          LayoutBuilder(
            builder: (context, constraints) {
              final title = Text(
                'Tor Status'.tr,
                style: TextStyle(
                  color: AppColors.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.w600,
                ),
              );
              final description = Text(
                'Tor provides the strongest privacy by routing traffic through multiple relays, making it very difficult to trace.'
                    .tr,
                style: TextStyle(
                  color: AppColors.textSecondary,
                  fontSize: 14,
                  height: 1.45,
                ),
              );
              final isWide = constraints.maxWidth >= 760;
              final switchExitButton = PTextButton(
                text: 'Switch exit node'.tr,
                compact: true,
                onPressed: torStatus.isReady ? _switchTorExit : null,
              );
              final controls = isWide
                  ? Row(
                      children: [
                        _buildTorStatusIndicator(torStatus),
                        const SizedBox(width: PirateSpacing.sm),
                        Expanded(child: switchExitButton),
                      ],
                    )
                  : Wrap(
                      crossAxisAlignment: WrapCrossAlignment.center,
                      spacing: PirateSpacing.sm,
                      runSpacing: PirateSpacing.xs,
                      children: [
                        _buildTorStatusIndicator(torStatus),
                        switchExitButton,
                      ],
                    );
              final route = Text(
                routingSummary,
                textAlign: isWide ? TextAlign.right : TextAlign.left,
                style: TextStyle(
                  color: AppColors.textSecondary,
                  fontSize: 12,
                  height: 1.4,
                ),
              );
              if (!isWide) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    title,
                    const SizedBox(height: PirateSpacing.sm),
                    controls,
                    const SizedBox(height: PirateSpacing.md),
                    description,
                    const SizedBox(height: PirateSpacing.xs),
                    route,
                  ],
                );
              }
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 720),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          title,
                          const SizedBox(height: PirateSpacing.sm),
                          description,
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(width: PirateSpacing.xl),
                  SizedBox(
                    width: 320,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: [
                        controls,
                        const SizedBox(height: PirateSpacing.xs),
                        route,
                      ],
                    ),
                  ),
                ],
              );
            },
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
                  ? 'Bootstrapping...'.tr
                  : 'Bootstrapping... {progress}%'.trArgs({
                      'progress': progress,
                    }),
              style: TextStyle(color: AppColors.textSecondary, fontSize: 12),
            ),
            if (torStatus.blocked != null && torStatus.blocked!.isNotEmpty) ...[
              const SizedBox(height: PirateSpacing.xs),
              Text(
                'Blocked: {reason}'.trArgs({'reason': torStatus.blocked}),
                style: TextStyle(color: AppColors.warning, fontSize: 12),
              ),
            ],
          ],
          if (torStatus.status == 'error' && torStatus.error != null) ...[
            const SizedBox(height: PirateSpacing.xs),
            Text(
              torStatus.error!,
              style: TextStyle(color: AppColors.error, fontSize: 12),
            ),
          ],
          if (_isDesktop) ...[
            Divider(
              height: PirateSpacing.xl,
              color: AppColors.borderSubtle,
            ),
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
        label = 'Ready'.tr;
        break;
      case 'bootstrapping':
        color = Colors.orange;
        label = 'Bootstrapping...'.tr;
        break;
      case 'error':
        color = Colors.red;
        label = 'Error'.tr;
        break;
      default:
        color = Colors.grey;
        label = 'Not Started'.tr;
    }

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 12,
          height: 12,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
        const SizedBox(width: PirateSpacing.xs),
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
        SnackBar(content: Text('Switched Tor exit node. Reconnecting...'.tr)),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            'Failed to switch exit node: {error}'.trArgs({'error': e}),
          ),
          backgroundColor: AppColors.error,
        ),
      );
    }
  }

  Widget _buildTorAdvancedControls(BuildContext context, WidgetRef ref) {
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      title: Text(
        'Advanced'.tr,
        style: TextStyle(
          color: AppColors.textPrimary,
          fontSize: 14,
          fontWeight: FontWeight.bold,
        ),
      ),
      subtitle: Text(
        'Bridges and transport overrides'.tr,
        style: TextStyle(color: AppColors.textSecondary, fontSize: 12),
      ),
      children: [_buildTorBridgeControls(context, ref)],
    );
  }

  Widget _buildTorBridgeControls(BuildContext context, WidgetRef ref) {
    if (!_isDesktop) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Bridge transports (Snowflake/obfs4) are desktop-only.'.tr,
            style: TextStyle(color: AppColors.textSecondary, fontSize: 12),
          ),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        PToggle(
          value: _useTorBridges,
          label: 'Use bridges immediately'.tr,
          onChanged: (value) => setState(() {
            _useTorBridges = value;
          }),
        ),
        const SizedBox(height: PirateSpacing.xs),
        PToggle(
          value: _fallbackToTorBridges,
          label: 'Fallback to bridges if direct fails'.tr,
          onChanged: (value) => setState(() {
            _fallbackToTorBridges = value;
          }),
        ),
        const SizedBox(height: PirateSpacing.sm),
        DropdownMenuFormField<String>(
          initialSelection: _torBridgeTransport,
          label: Text('Fallback bridge transport'.tr),
          helperText: 'Only used if direct Tor fails.'.tr,
          dropdownMenuEntries: [
            const DropdownMenuEntry(value: 'snowflake', label: 'Snowflake'),
            const DropdownMenuEntry(value: 'obfs4', label: 'obfs4'),
          ],
          onSelected: (value) {
            if (value == null) return;
            setState(() {
              _torBridgeTransport = value;
            });
          },
        ),
        const SizedBox(height: PirateSpacing.sm),
        PInput(
          controller: _torBridgeLinesController,
          label: 'Bridge lines'.tr,
          hint: _torBridgeTransport == 'snowflake'
              ? 'Leave blank to use bundled Snowflake bridges'.tr
              : 'Paste one bridge line per row'.tr,
          helperText: 'One bridge per line. Used only for bridges/fallback.'.tr,
          maxLines: 4,
          monospace: true,
        ),
        const SizedBox(height: PirateSpacing.sm),
        PInput(
          controller: _torTransportPathController,
          label: 'Transport binary path (optional)'.tr,
          hint: 'Leave blank to use PATH'.tr,
          monospace: true,
        ),
        if (_torBridgeError != null) ...[
          const SizedBox(height: PirateSpacing.xs),
          Text(
            _torBridgeError!,
            style: TextStyle(color: AppColors.error, fontSize: 12),
          ),
        ],
        const SizedBox(height: PirateSpacing.sm),
        Row(
          children: [
            Expanded(
              child: PButton(
                text: 'Apply & Restart Tor'.tr,
                variant: PButtonVariant.secondary,
                onPressed: () => _applyTorBridgeSettings(ref),
              ),
            ),
            const SizedBox(width: PirateSpacing.sm),
            Expanded(
              child: PTextButton(
                text: 'Use Snowflake'.tr,
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
                text: 'Use obfs4'.tr,
                onPressed: () => _applyTorBridgePreset(ref, 'obfs4'),
              ),
            ),
            const SizedBox(width: PirateSpacing.sm),
            Expanded(
              child: PTextButton(
                text: 'Disable Bridges'.tr,
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
    if (!_isDesktop) {
      return 'Attempting: Direct (bridges disabled on mobile)'.tr;
    }
    final transportLabel = _torTransportLabel(_torBridgeTransport);
    if (_useTorBridges) {
      return 'Attempting: {transport} (bridges)'.trArgs({
        'transport': transportLabel,
      });
    }
    if (_fallbackToTorBridges) {
      return 'Attempting: Direct -> Fallback: {transport}'.trArgs({
        'transport': transportLabel,
      });
    }
    return 'Attempting: Direct (no fallback bridges)'.tr;
  }

  Future<void> _applyTorBridgeSettings(WidgetRef ref) async {
    if (!_isDesktop) {
      setState(() {
        _torBridgeError = 'Bridge transports are desktop-only.'.tr;
      });
      return;
    }

    final lines = _splitBridgeLines(_torBridgeLinesController.text);
    if ((_useTorBridges || _fallbackToTorBridges) &&
        _torBridgeTransport == 'obfs4' &&
        lines.isEmpty) {
      setState(() {
        _torBridgeError = 'obfs4 requires bridge lines from a provider.'.tr;
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

    await ref
        .read(transportConfigProvider.notifier)
        .setTorBridgeConfig(config, apply: true);
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
          _buildDnsOption(
            ref,
            'cloudflare_doh',
            'Cloudflare (1.1.1.1)',
            currentProvider,
          ),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(ref, 'quad9_doh', 'Quad9 (9.9.9.9)', currentProvider),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(
            ref,
            'google_doh',
            'Google (8.8.8.8)',
            currentProvider,
          ),
          Divider(height: 1, color: AppColors.borderDefault),
          _buildDnsOption(
            ref,
            'system',
            'System (Not Private)'.tr,
            currentProvider,
          ),
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
                  color: isSelected
                      ? AppColors.accentPrimary
                      : AppColors.textPrimary,
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
      final endpointConfig = await ref.read(
        lightdEndpointConfigProvider.future,
      );
      final url = endpointConfig.url;
      final tlsPin = endpointConfig.tlsPin?.trim();
      final normalizedPin = tlsPin == null || tlsPin.isEmpty ? null : tlsPin;

      // Test the node connection
      final result = await FfiBridge.testNode(url: url, tlsPin: normalizedPin);

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
                'Connection Successful'.tr,
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
            _buildResultRow(
              'Transport'.tr,
              '${result.transportIcon} ${result.transportMode.toUpperCase()}',
            ),
            _buildResultRow(
              'TLS',
              result.tlsEnabled ? 'Enabled ✓'.tr : 'Disabled'.tr,
            ),
            if (result.tlsPinMatched != null)
              _buildResultRow(
                'Pin Verified'.tr,
                result.tlsPinMatched! ? 'Yes ✓'.tr : 'MISMATCH ✗'.tr,
                valueColor: result.tlsPinMatched! ? Colors.green : Colors.red,
              ),
            if (result.latestBlockHeight != null)
              _buildResultRow('Latest Block'.tr, '#${result.latestBlockHeight}')
            else
              _buildResultRow(
                'Latest Block'.tr,
                'Unavailable (Connection Failed)'.tr,
                valueColor: AppColors.error,
              ),
            _buildResultRow('Response Time'.tr, '${result.responseTimeMs}ms'),
            if (result.serverVersion != null)
              _buildResultRow('Server'.tr, result.serverVersion!),
            if (result.chainName != null)
              _buildResultRow('Chain'.tr, result.chainName!),
          ],
        ),
        actions: [
          PTextButton(
            label: 'OK'.tr,
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
                isPinMismatch
                    ? 'Certificate Pin Mismatch'.tr
                    : 'Connection Failed'.tr,
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
              _buildResultRow(
                'Transport'.tr,
                '${result.transportIcon} ${result.transportMode.toUpperCase()}',
              ),
              _buildResultRow(
                'TLS',
                result.tlsEnabled ? 'Enabled'.tr : 'Disabled'.tr,
              ),
              _buildResultRow('Response Time'.tr, '${result.responseTimeMs}ms'),

              const SizedBox(height: 16),

              if (isPinMismatch) ...[
                Container(
                  padding: EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.red.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(
                      color: Colors.red.withValues(alpha: 0.3),
                    ),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '⚠️ Security Warning'.tr,
                        style: TextStyle(
                          color: Colors.red,
                          fontWeight: FontWeight.bold,
                          fontSize: 14,
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'The server certificate does not match the expected '
                                'pin. This could indicate:\n'
                                '• A man-in-the-middle attack\n'
                                '• The server certificate has been rotated\n'
                                '• An incorrect pin was entered'
                            .tr,
                        style: TextStyle(
                          color: AppColors.textSecondary,
                          fontSize: 12,
                        ),
                      ),
                      if (result.expectedPin != null) ...[
                        const SizedBox(height: 8),
                        Text(
                          'Expected:'.tr,
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
                          'Actual (from server):'.tr,
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
                          '⚠️ Latest block height not retrieved - connection failed before data could be fetched.'
                              .tr,
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
              label: 'Copy Actual Pin'.tr,
              leadingIcon: Icons.copy,
              variant: PTextButtonVariant.subtle,
              onPressed: () {
                Clipboard.setData(ClipboardData(text: result.actualPin!));
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(content: Text('Actual pin copied to clipboard'.tr)),
                );
              },
            ),
          PTextButton(
            label: 'OK'.tr,
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
                'Error'.tr,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: AppColors.textPrimary),
              ),
            ),
          ],
        ),
        content: Text(error, style: TextStyle(color: AppColors.textSecondary)),
        actions: [
          PTextButton(
            label: 'OK'.tr,
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
              style: TextStyle(color: AppColors.textSecondary, fontSize: 13),
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

class _TransportChoice {
  const _TransportChoice(this.mode, this.label, this.icon);

  final String mode;
  final String label;
  final IconData icon;
}
