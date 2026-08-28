/// Node settings screen - Lightwalletd endpoint configuration
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../config/endpoints.dart' as endpoints;
import '../../../core/ffi/ffi_bridge.dart' as ffi;
import '../../../core/providers/wallet_providers.dart';
import '../providers/endpoint_health_provider.dart';
import '../providers/transport_providers.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/connection_status_indicator.dart';
import '../../../ui/molecules/p_snack.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Node settings screen for configuring lightwalletd endpoint
class NodeSettingsScreen extends ConsumerStatefulWidget {
  const NodeSettingsScreen({super.key});

  @override
  ConsumerState<NodeSettingsScreen> createState() => _NodeSettingsScreenState();
}

class _NodeSettingsScreenState extends ConsumerState<NodeSettingsScreen> {
  final _formKey = GlobalKey<FormState>();
  final _endpointController = TextEditingController();
  final _tlsPinController = TextEditingController();

  bool _useTls = endpoints.kDefaultUseTls;
  bool _isLoading = false;
  bool _hasChanges = false;
  bool _isFetchingSpkiPin = false;
  bool _automaticFailover = false;
  String? _applyingPresetId;
  String? _spkiPinMessage;
  bool _spkiPinMessageIsError = false;
  String? _originalEndpoint;
  String? _originalTlsPin;
  bool _originalUseTls = endpoints.kDefaultUseTls;
  bool _originalAutomaticFailover = false;
  ProviderSubscription<AsyncValue<ffi.LightdEndpointConfig>>?
  _endpointSubscription;

  @override
  void initState() {
    super.initState();
    _endpointSubscription = ref
        .listenManual<AsyncValue<ffi.LightdEndpointConfig>>(
          lightdEndpointConfigProvider,
          (_, next) => next.whenData(_applyCurrentEndpoint),
          fireImmediately: true,
        );
  }

  @override
  void dispose() {
    _endpointSubscription?.close();
    _endpointController.dispose();
    _tlsPinController.dispose();
    super.dispose();
  }

  void _applyCurrentEndpoint(ffi.LightdEndpointConfig config) {
    if (_hasChanges || !mounted) return;
    final displayString = config.displayString;
    final tlsPin = config.tlsPin ?? '';
    setState(() {
      _endpointController.text = displayString;
      _tlsPinController.text = tlsPin;
      _useTls = config.useTls;
      _automaticFailover = config.automaticFailover;
      _originalEndpoint = displayString;
      _originalTlsPin = tlsPin;
      _originalUseTls = config.useTls;
      _originalAutomaticFailover = config.automaticFailover;
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });
  }

  void _onEndpointChanged(String value) {
    setState(() {
      if (value.trim() != (_originalEndpoint ?? '') &&
          _tlsPinController.text.trim() == (_originalTlsPin ?? '')) {
        _tlsPinController.clear();
      }
      _automaticFailover = false;
      _hasChanges = _selectionHasChanges();
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });
  }

  void _onTlsPinChanged(String value) {
    setState(() {
      _automaticFailover = false;
      _hasChanges = _selectionHasChanges();
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });
  }

  String? _validateEndpoint(String? value) {
    if (value == null || value.trim().isEmpty) {
      return 'Endpoint is required'.tr;
    }

    final parsed = endpoints.LightdEndpoint.tryParse(value);
    if (parsed == null) {
      return 'Invalid endpoint format (use host:port)'.tr;
    }

    return null;
  }

  String? _validateTlsPin(String? value) {
    if (value == null || value.trim().isEmpty) {
      return null; // TLS pin is optional
    }

    final normalized = _normalizeSpkiPin(value.trim());
    if (!_isValidSpkiPin(normalized)) {
      return 'Invalid TLS pin format (base64-encoded SPKI hash)'.tr;
    }

    return null;
  }

  String _normalizeSpkiPin(String value) {
    final trimmed = value.trim();
    if (trimmed.startsWith('sha256/')) {
      return trimmed.substring(7);
    }
    return trimmed;
  }

  bool _isValidSpkiPin(String value) {
    if (value.isEmpty) {
      return false;
    }
    return endpoints.LightdEndpoint.isValidTlsPin(value);
  }

  Future<void> _fetchSpkiPin() async {
    if (!_useTls) {
      setState(() {
        _spkiPinMessage = 'Enable TLS to fetch a pin.'.tr;
        _spkiPinMessageIsError = true;
      });
      return;
    }

    final endpointInput = _endpointController.text.trim();
    final parsed = endpoints.LightdEndpoint.tryParse(endpointInput);
    if (parsed == null) {
      setState(() {
        _spkiPinMessage = 'Invalid endpoint format.'.tr;
        _spkiPinMessageIsError = true;
      });
      return;
    }

    setState(() {
      _isFetchingSpkiPin = true;
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });

    final url = _useTls
        ? 'https://${parsed.host}:${parsed.port}'
        : 'http://${parsed.host}:${parsed.port}';

    try {
      final result = await ffi.FfiBridge.testNode(url: url, tlsPin: null);
      final actualPin = result.actualPin?.trim();
      if (result.success && actualPin != null && actualPin.isNotEmpty) {
        final normalizedPin = _normalizeSpkiPin(actualPin);
        if (!_isValidSpkiPin(normalizedPin)) {
          setState(() {
            _spkiPinMessage = 'SPKI pin returned by server is invalid.'.tr;
            _spkiPinMessageIsError = true;
          });
          return;
        }

        _tlsPinController.text = normalizedPin;
        _automaticFailover = false;
        await ref.read(setLightdEndpointProvider)(
          url: url,
          tlsPin: normalizedPin,
        );

        _originalEndpoint = parsed.displayString;
        _originalTlsPin = normalizedPin;
        _originalUseTls = _useTls;
        _originalAutomaticFailover = false;
        if (mounted) {
          setState(() {
            _hasChanges = false;
            _spkiPinMessage = 'SPKI pin retrieved and saved.'.tr;
            _spkiPinMessageIsError = false;
          });
        }
      } else {
        final errorMessage = result.errorMessage?.trim();
        final normalizedError = errorMessage?.toLowerCase() ?? '';
        final tlsLikelyUnsupported =
            _useTls &&
            (normalizedError.contains('connection failed') ||
                normalizedError.contains('transport error') ||
                normalizedError.contains('tls') ||
                normalizedError.contains('certificate') ||
                normalizedError.contains('dns'));
        setState(() {
          if (tlsLikelyUnsupported) {
            _spkiPinMessage = errorMessage?.isNotEmpty ?? false
                ? 'This endpoint likely does not support TLS. Disable TLS or use a TLS-enabled endpoint. {error}'
                      .trArgs({'error': errorMessage})
                : 'This endpoint likely does not support TLS. Disable TLS or use a TLS-enabled endpoint.'
                      .tr;
          } else {
            _spkiPinMessage = errorMessage?.isNotEmpty ?? false
                ? 'SPKI pin not available: {error}'.trArgs({
                    'error': errorMessage,
                  })
                : 'SPKI pin not available for this endpoint.'.tr;
          }
          _spkiPinMessageIsError = true;
        });
      }
    } catch (e) {
      setState(() {
        _spkiPinMessage = 'Failed to fetch SPKI pin: {error}'.trArgs({
          'error': e,
        });
        _spkiPinMessageIsError = true;
      });
    } finally {
      if (mounted) {
        setState(() => _isFetchingSpkiPin = false);
      }
    }
  }

  Future<void> _saveEndpoint() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }

    setState(() => _isLoading = true);

    try {
      final endpoint = _endpointController.text.trim();
      final tlsPin = _normalizeSpkiPin(_tlsPinController.text.trim());

      // Build URL with scheme
      final parsed = endpoints.LightdEndpoint.tryParse(
        endpoint,
        automaticFailover: _automaticFailover,
      );
      if (parsed == null) {
        throw Exception('Invalid endpoint'.tr);
      }

      final selection = endpoints.LightdEndpoint(
        id: parsed.id,
        host: parsed.host,
        port: parsed.port,
        useTls: _useTls,
        tlsPin: _automaticFailover || tlsPin.isEmpty ? null : tlsPin,
        label: parsed.label,
        network: parsed.network,
        route: parsed.route,
        automaticFailover: _automaticFailover,
      );

      await ref.read(setLightdEndpointSelectionProvider)(selection);

      _originalEndpoint = parsed.displayString;
      _originalTlsPin = tlsPin.isEmpty ? '' : tlsPin;
      _originalUseTls = _useTls;
      _originalAutomaticFailover = _automaticFailover;

      if (mounted) {
        setState(() {
          _hasChanges = false;
          _isLoading = false;
        });

        PSnack.show(
          context: context,
          message: 'Node endpoint saved'.tr,
          variant: PSnackVariant.success,
        );
      }
    } catch (e) {
      if (mounted) {
        setState(() => _isLoading = false);

        PSnack.show(
          context: context,
          message: 'Failed to save endpoint: {error}'.trArgs({'error': e}),
          variant: PSnackVariant.error,
        );
      }
    }
  }

  void _resetToDefault() {
    final defaultEndpoint = endpoints.LightdEndpoint.defaultEndpoint;
    setState(() {
      _endpointController.text = defaultEndpoint.displayString;
      _tlsPinController.text = '';
      _useTls = defaultEndpoint.useTls;
      _automaticFailover = true;
      _hasChanges = _selectionHasChanges();
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });
  }

  void _applySuggested(endpoints.LightdEndpoint endpoint) {
    setState(() {
      _endpointController.text = endpoint.displayString;
      _tlsPinController.text = endpoint.tlsPin ?? '';
      _useTls = endpoint.useTls;
      _automaticFailover = endpoint.automaticFailover;
      _hasChanges = _selectionHasChanges();
      _spkiPinMessage = null;
      _spkiPinMessageIsError = false;
    });
  }

  bool _selectionHasChanges() =>
      _endpointController.text != _originalEndpoint ||
      _tlsPinController.text != (_originalTlsPin ?? '') ||
      _useTls != _originalUseTls ||
      _automaticFailover != _originalAutomaticFailover;

  Future<void> _applySuggestedEndpoint(
    endpoints.LightdEndpoint endpoint,
  ) async {
    if (_isLoading || _isFetchingSpkiPin || _originalEndpoint == null) return;

    _applySuggested(endpoint);
    if (!_hasChanges) return;

    setState(() {
      _isLoading = true;
      _applyingPresetId = endpoint.id;
    });

    try {
      await ref.read(setLightdEndpointSelectionProvider)(endpoint);
      if (!mounted) return;

      setState(() {
        _originalEndpoint = endpoint.displayString;
        _originalTlsPin = endpoint.tlsPin ?? '';
        _originalUseTls = endpoint.useTls;
        _originalAutomaticFailover = endpoint.automaticFailover;
        _hasChanges = false;
        _isLoading = false;
        _applyingPresetId = null;
      });
      PSnack.show(
        context: context,
        message: 'Node endpoint saved'.tr,
        variant: PSnackVariant.success,
      );
    } catch (error) {
      if (!mounted) return;

      setState(() {
        _endpointController.text = _originalEndpoint ?? '';
        _tlsPinController.text = _originalTlsPin ?? '';
        _useTls = _originalUseTls;
        _automaticFailover = _originalAutomaticFailover;
        _hasChanges = false;
        _isLoading = false;
        _applyingPresetId = null;
      });
      PSnack.show(
        context: context,
        message: 'Failed to save endpoint: {error}'.trArgs({'error': error}),
        variant: PSnackVariant.error,
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final endpointConfigAsync = ref.watch(lightdEndpointConfigProvider);
    final endpointHealth = ref.watch(endpointHealthProvider);
    final transportMode = ref.watch(transportConfigProvider).mode;
    final suggestedEndpoints = endpoints.LightdEndpoint.presetsForTransport(
      transportMode,
    );
    final isMobile = AppSpacing.isHandset(MediaQuery.sizeOf(context));
    final basePadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
    );
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );

    return PScaffold(
      title: 'Node Configuration'.tr,
      appBar: PAppBar(
        title: 'Node Configuration'.tr,
        subtitle: 'Choose your lightwalletd endpoint'.tr,
        actions: [
          ConnectionStatusIndicator(full: !isMobile),
          if (isMobile)
            PIconButton(
              icon: Icon(Icons.refresh, color: AppColors.textSecondary),
              onPressed: () => _formKey.currentState?.reset(),
              tooltip: 'Reset'.tr,
            )
          else
            PTextButton(
              label: 'Reset'.tr,
              onPressed: () => _formKey.currentState?.reset(),
              variant: PTextButtonVariant.subtle,
            ),
        ],
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Form(
          key: _formKey,
          child: FormField<void>(
            onReset: _resetToDefault,
            builder: (_) => Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // Current status card
                endpointConfigAsync.when(
                  data: (config) =>
                      _buildStatusCard(config, endpointHealth, transportMode),
                  loading: () =>
                      const Center(child: CircularProgressIndicator()),
                  error: (e, _) => _buildErrorCard(e.toString()),
                ),

                const SizedBox(height: AppSpacing.xl),

                // Suggested network endpoints
                Text(
                  'SUGGESTED ENDPOINTS'.tr,
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 1.2,
                  ),
                ),
                const SizedBox(height: AppSpacing.md),
                _buildPresetGrid(suggestedEndpoints, transportMode),

                const SizedBox(height: AppSpacing.xl),

                // Endpoint input section
                Text(
                  'LIGHTWALLETD ENDPOINT'.tr,
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 1.2,
                  ),
                ),
                const SizedBox(height: AppSpacing.md),

                PCard(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      PInput(
                        controller: _endpointController,
                        label: 'Endpoint (host:port)'.tr,
                        hint: 'lightd1.pirate.black:443',
                        keyboardType: TextInputType.url,
                        validator: _validateEndpoint,
                        onChanged: _onEndpointChanged,
                        prefixIcon: const Icon(Icons.dns_outlined),
                      ),

                      const SizedBox(height: AppSpacing.lg),

                      // TLS toggle
                      SwitchListTile(
                        title: Text('Use TLS'.tr),
                        subtitle: Text(
                          _useTls
                              ? 'Encrypted connection (recommended)'.tr
                              : 'Unencrypted connection (not recommended)'.tr,
                          style: AppTypography.bodySmall.copyWith(
                            color: _useTls
                                ? AppColors.success
                                : AppColors.warning,
                          ),
                        ),
                        value: _useTls,
                        onChanged: (value) {
                          setState(() {
                            _useTls = value;
                            _automaticFailover = false;
                            _hasChanges = _selectionHasChanges();
                          });
                        },
                        activeTrackColor: AppColors.accentPrimary.withValues(
                          alpha: 0.4,
                        ),
                        activeThumbColor: AppColors.accentPrimary,
                        contentPadding: EdgeInsets.zero,
                      ),
                    ],
                  ),
                ),

                const SizedBox(height: AppSpacing.xl),

                // TLS pin section
                Text(
                  'TLS CERTIFICATE PIN (OPTIONAL)'.tr,
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 1.2,
                  ),
                ),
                const SizedBox(height: AppSpacing.md),

                PCard(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      PInput(
                        key: const ValueKey('tls-pin-input'),
                        controller: _tlsPinController,
                        label: 'SPKI Pin (base64)'.tr,
                        helperText:
                            'Leave empty to skip certificate pinning'.tr,
                        validator: _validateTlsPin,
                        onChanged: _onTlsPinChanged,
                        prefixIcon: const Icon(Icons.lock_outline),
                        keyboardType: TextInputType.visiblePassword,
                        textInputAction: TextInputAction.done,
                        autocorrect: false,
                        enableSuggestions: false,
                        monospace: true,
                      ),

                      const SizedBox(height: AppSpacing.md),

                      PButton(
                        text: _isFetchingSpkiPin
                            ? 'Fetching...'.tr
                            : 'Fetch SPKI'.tr,
                        onPressed: _useTls && !_isFetchingSpkiPin && !_isLoading
                            ? _fetchSpkiPin
                            : null,
                        variant: PButtonVariant.secondary,
                        fullWidth: true,
                      ),

                      if (_spkiPinMessage != null) ...[
                        const SizedBox(height: AppSpacing.sm),
                        Text(
                          _spkiPinMessage!,
                          style: AppTypography.bodySmall.copyWith(
                            color: _spkiPinMessageIsError
                                ? AppColors.error
                                : AppColors.success,
                          ),
                        ),
                      ],

                      const SizedBox(height: AppSpacing.md),

                      Container(
                        padding: const EdgeInsets.all(AppSpacing.md),
                        decoration: BoxDecoration(
                          color: AppColors.warning.withValues(alpha: 0.1),
                          borderRadius: BorderRadius.circular(8),
                          border: Border.all(
                            color: AppColors.warning.withValues(alpha: 0.3),
                          ),
                        ),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Icon(
                              Icons.info_outline,
                              color: AppColors.warning,
                              size: 20,
                            ),
                            const SizedBox(width: AppSpacing.sm),
                            Expanded(
                              child: Text(
                                "TLS pinning adds extra security by verifying the server's certificate. "
                                        'Use Fetch SPKI to grab the pin from the current endpoint.'
                                    .tr,
                                style: AppTypography.bodySmall.copyWith(
                                  color: AppColors.warning,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),

                const SizedBox(height: AppSpacing.xxl),

                // Save button
                SizedBox(
                  width: double.infinity,
                  child: PButton(
                    onPressed: _hasChanges && !_isLoading && !_isFetchingSpkiPin
                        ? _saveEndpoint
                        : null,
                    isLoading: _isLoading,
                    child: Text('Save Changes'.tr),
                  ),
                ),

                const SizedBox(height: AppSpacing.lg),

                // Help text
                Text(
                  'Changes will take effect immediately. The wallet will reconnect to the new endpoint.'
                      .tr,
                  style: AppTypography.bodySmall.copyWith(
                    color: AppColors.textSecondary,
                  ),
                  textAlign: TextAlign.center,
                ),

                const SizedBox(height: AppSpacing.xxl),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildStatusCard(
    ffi.LightdEndpointConfig config,
    EndpointHealthState health,
    String transportMode,
  ) {
    final configuredEndpoint = endpoints.LightdEndpoint.tryParse(
      config.url,
      automaticFailover: config.automaticFailover,
    );
    final route = configuredEndpoint?.route ?? endpoints.LightdRoute.clearnet;
    final (securityIcon, securityLabel, securityColor) = switch (transportMode
        .toLowerCase()) {
      'tor' => (Icons.security_outlined, 'Tor'.tr, AppColors.accentPrimary),
      'i2p' => (Icons.router_outlined, 'I2P'.tr, AppColors.accentPrimary),
      _ => switch (route) {
        endpoints.LightdRoute.tor => (
          Icons.security_outlined,
          'Tor'.tr,
          AppColors.accentPrimary,
        ),
        endpoints.LightdRoute.i2p => (
          Icons.router_outlined,
          'I2P'.tr,
          AppColors.accentPrimary,
        ),
        endpoints.LightdRoute.clearnet =>
          config.useTls
              ? (Icons.lock, 'TLS Enabled'.tr, AppColors.success)
              : (Icons.lock_open, 'TLS Disabled'.tr, AppColors.warning),
      },
    };
    final record = health.recordFor(
      config.automaticFailover ? (health.activeUrl ?? config.url) : config.url,
    );
    final isChecking =
        health.phase == EndpointHealthPhase.checking ||
        health.phase == EndpointHealthPhase.switching;
    final statusColor = record == null
        ? AppColors.textDisabled
        : record.healthy
        ? AppColors.success
        : AppColors.error;
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 12,
                height: 12,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: statusColor,
                  boxShadow: record?.healthy == true
                      ? [
                          BoxShadow(
                            color: statusColor.withValues(alpha: 0.4),
                            blurRadius: 8,
                            spreadRadius: 2,
                          ),
                        ]
                      : null,
                ),
              ),
              const SizedBox(width: AppSpacing.sm),
              Expanded(
                child: Text(
                  config.automaticFailover
                      ? 'Server selection'.tr
                      : 'Current Node'.tr,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppTypography.labelLarge.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
              ),
              const SizedBox(width: AppSpacing.xs),
              if (isChecking)
                const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              else
                IconButton(
                  icon: const Icon(Icons.refresh, size: 19),
                  onPressed: () => ref
                      .read(endpointHealthProvider.notifier)
                      .checkNow(probePool: true),
                  tooltip: 'Refresh'.tr,
                  visualDensity: VisualDensity.compact,
                ),
            ],
          ),
          const SizedBox(height: AppSpacing.md),

          Row(
            children: [
              Icon(
                Icons.dns_outlined,
                color: AppColors.textSecondary,
                size: 20,
              ),
              const SizedBox(width: AppSpacing.sm),
              Expanded(
                child: Text(
                  config.automaticFailover
                      ? 'Automatic server selection'.tr
                      : config.displayString,
                  style: AppTypography.bodyMedium.copyWith(
                    color: AppColors.textPrimary,
                    fontFamily: 'monospace',
                  ),
                ),
              ),
              if (!config.automaticFailover)
                IconButton(
                  icon: const Icon(Icons.copy, size: 18),
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: config.url));
                    PSnack.show(
                      context: context,
                      message: 'Endpoint copied'.tr,
                      variant: PSnackVariant.info,
                    );
                  },
                  tooltip: 'Copy endpoint'.tr,
                  visualDensity: VisualDensity.compact,
                ),
            ],
          ),

          const SizedBox(height: AppSpacing.sm),

          if (config.automaticFailover) ...[
            Text(
              'Uses multiple verified servers and keeps results in chain order'
                  .tr,
              style: AppTypography.bodySmall.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
            const SizedBox(height: AppSpacing.sm),
          ],

          Wrap(
            spacing: AppSpacing.xs,
            runSpacing: AppSpacing.xs,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Icon(securityIcon, color: securityColor, size: 16),
              Text(
                securityLabel,
                style: AppTypography.bodySmall.copyWith(color: securityColor),
              ),
              if (config.tlsPin != null) ...[
                const SizedBox(width: AppSpacing.md),
                Icon(
                  Icons.verified_user,
                  color: AppColors.accentPrimary,
                  size: 16,
                ),
                Text(
                  'Certificate Pinned'.tr,
                  style: AppTypography.bodySmall.copyWith(
                    color: AppColors.accentPrimary,
                  ),
                ),
              ],
            ],
          ),
          const SizedBox(height: AppSpacing.sm),
          if (record == null || isChecking)
            Text(
              'Checking...'.tr,
              style: AppTypography.bodySmall.copyWith(
                color: AppColors.textSecondary,
              ),
            )
          else if (!record.healthy)
            Text(
              'Connection failed'.tr,
              style: AppTypography.bodySmall.copyWith(color: AppColors.error),
            )
          else
            Wrap(
              spacing: AppSpacing.md,
              runSpacing: AppSpacing.xs,
              children: [
                if (record.height != null)
                  Text(
                    'Block #{height}'.trArgs({'height': record.height}),
                    style: AppTypography.bodySmall.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                if (record.responseTimeMs != null)
                  Text(
                    '${record.responseTimeMs}ms',
                    style: AppTypography.bodySmall.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _buildPresetGrid(
    List<endpoints.LightdEndpoint> presets,
    String transportMode,
  ) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final columns = constraints.maxWidth >= 900
            ? 3
            : constraints.maxWidth >= 560
            ? 2
            : 1;
        const gap = AppSpacing.sm;
        final tileWidth =
            (constraints.maxWidth - gap * (columns - 1)) / columns;
        return Wrap(
          spacing: gap,
          runSpacing: gap,
          children: [
            for (final endpoint in presets)
              SizedBox(
                width: tileWidth,
                child: _buildPresetTile(endpoint, transportMode),
              ),
          ],
        );
      },
    );
  }

  Widget _buildPresetTile(
    endpoints.LightdEndpoint endpoint,
    String transportMode,
  ) {
    final selected =
        _endpointController.text.trim() == endpoint.displayString &&
        _useTls == endpoint.useTls &&
        _automaticFailover == endpoint.automaticFailover;
    final routedThroughTor =
        transportMode.toLowerCase() == 'tor' &&
        endpoint.route == endpoints.LightdRoute.clearnet;
    final routeIcon = endpoint.automaticFailover
        ? Icons.hub_outlined
        : routedThroughTor
        ? Icons.security_outlined
        : switch (endpoint.route) {
            endpoints.LightdRoute.tor => Icons.security_outlined,
            endpoints.LightdRoute.i2p => Icons.router_outlined,
            endpoints.LightdRoute.clearnet =>
              endpoint.useTls ? Icons.lock_outline : Icons.dns_outlined,
          };
    final applying = _applyingPresetId == endpoint.id;
    final canSelect = !_isLoading && !_isFetchingSpkiPin;
    return Semantics(
      button: true,
      selected: selected,
      enabled: canSelect,
      child: PCard(
        key: ValueKey('endpoint-preset-${endpoint.id}'),
        onTap: canSelect
            ? () => unawaited(_applySuggestedEndpoint(endpoint))
            : null,
        padding: const EdgeInsets.all(AppSpacing.md),
        backgroundColor: selected
            ? AppColors.accentPrimary.withValues(alpha: 0.1)
            : null,
        child: _buildAutomaticPresetTooltip(
          endpoint,
          ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 58),
            child: Row(
              children: [
                Container(
                  width: 34,
                  height: 34,
                  decoration: BoxDecoration(
                    color: selected
                        ? AppColors.accentPrimary.withValues(alpha: 0.14)
                        : AppColors.backgroundElevated,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: Icon(
                    routeIcon,
                    size: 18,
                    color: selected
                        ? AppColors.accentPrimary
                        : AppColors.textSecondary,
                  ),
                ),
                const SizedBox(width: AppSpacing.sm),
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        endpoint.displayLabel,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppTypography.labelMedium.copyWith(
                          color: selected
                              ? AppColors.accentPrimary
                              : AppColors.textPrimary,
                        ),
                      ),
                      const SizedBox(height: 3),
                      Text(
                        routedThroughTor
                            ? '${endpoint.displayString} • ${'Tor'.tr}'
                            : endpoint.displaySubtitle,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppTypography.bodySmall.copyWith(
                          color: AppColors.textSecondary,
                          fontFamily: 'monospace',
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: AppSpacing.sm),
                _buildPresetState(selected: selected, applying: applying),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildAutomaticPresetTooltip(
    endpoints.LightdEndpoint endpoint,
    Widget child,
  ) {
    if (!endpoint.automaticFailover) return child;
    return Tooltip(
      message: 'Auto can sync faster by downloading different parts of the blockchain from several trusted servers at the same time.'
          .tr,
      child: child,
    );
  }

  Widget _buildPresetState({required bool selected, required bool applying}) {
    if (applying) {
      return const SizedBox.square(
        dimension: 18,
        child: CircularProgressIndicator(strokeWidth: 2),
      );
    }
    return Icon(
      selected ? Icons.radio_button_checked : Icons.radio_button_unchecked,
      color: selected ? AppColors.accentPrimary : AppColors.textDisabled,
      size: 19,
    );
  }

  Widget _buildErrorCard(String error) {
    return PCard(
      child: Row(
        children: [
          Icon(Icons.error_outline, color: AppColors.error),
          const SizedBox(width: AppSpacing.md),
          Expanded(
            child: Text(
              'Failed to load endpoint: {error}'.trArgs({'error': error}),
              style: AppTypography.bodySmall.copyWith(color: AppColors.error),
            ),
          ),
        ],
      ),
    );
  }
}
