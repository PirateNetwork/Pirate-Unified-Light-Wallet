/// Node settings screen - Lightwalletd endpoint configuration
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../config/endpoints.dart' as endpoints;
import '../../../core/ffi/ffi_bridge.dart' as ffi;
import '../../../core/providers/wallet_providers.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_icon_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/p_snack.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

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
  String? _originalEndpoint;
  String? _originalTlsPin;

  @override
  void initState() {
    super.initState();
    _loadCurrentEndpoint();
  }

  @override
  void dispose() {
    _endpointController.dispose();
    _tlsPinController.dispose();
    super.dispose();
  }

  Future<void> _loadCurrentEndpoint() async {
    final configAsync = ref.read(lightdEndpointConfigProvider);
    
    configAsync.whenData((config) {
      _endpointController.text = config.displayString;
      _tlsPinController.text = config.tlsPin ?? '';
      _useTls = config.useTls;
      _originalEndpoint = config.displayString;
      _originalTlsPin = config.tlsPin ?? '';
      if (mounted) setState(() {});
    });
  }

  void _onEndpointChanged(String value) {
    setState(() {
      _hasChanges = value != _originalEndpoint || 
          _tlsPinController.text != (_originalTlsPin ?? '');
    });
  }

  void _onTlsPinChanged(String value) {
    setState(() {
      _hasChanges = _endpointController.text != _originalEndpoint || 
          value != (_originalTlsPin ?? '');
    });
  }

  String? _validateEndpoint(String? value) {
    if (value == null || value.trim().isEmpty) {
      return 'Endpoint is required';
    }
    
    final parsed = endpoints.LightdEndpoint.tryParse(value);
    if (parsed == null) {
      return 'Invalid endpoint format (use host:port)';
    }
    
    return null;
  }

  String? _validateTlsPin(String? value) {
    if (value == null || value.trim().isEmpty) {
      return null; // TLS pin is optional
    }
    
    if (!endpoints.LightdEndpoint.isValidTlsPin(value.trim())) {
      return 'Invalid TLS pin format (base64-encoded SPKI hash)';
    }
    
    return null;
  }

  Future<void> _saveEndpoint() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }
    
    setState(() => _isLoading = true);
    
    try {
      final endpoint = _endpointController.text.trim();
      final tlsPin = _tlsPinController.text.trim();
      
      // Build URL with scheme
      final parsed = endpoints.LightdEndpoint.tryParse(endpoint);
      if (parsed == null) {
        throw Exception('Invalid endpoint');
      }
      
      final fullUrl = _useTls 
          ? 'https://${parsed.host}:${parsed.port}'
          : 'http://${parsed.host}:${parsed.port}';
      
      final setEndpoint = ref.read(setLightdEndpointProvider);
      await setEndpoint(
        url: fullUrl,
        tlsPin: tlsPin.isEmpty ? null : tlsPin,
      );
      
      _originalEndpoint = parsed.displayString;
      _originalTlsPin = tlsPin.isEmpty ? '' : tlsPin;
      
      if (mounted) {
        setState(() {
          _hasChanges = false;
          _isLoading = false;
        });
        
        PSnack.show(
          context: context,
          message: 'Node endpoint saved',
          variant: PSnackVariant.success,
        );
      }
    } catch (e) {
      if (mounted) {
        setState(() => _isLoading = false);
        
        PSnack.show(
          context: context,
          message: 'Failed to save endpoint: $e',
          variant: PSnackVariant.error,
        );
      }
    }
  }

  void _resetToDefault() {
    setState(() {
      _endpointController.text = endpoints.kDefaultLightd;
      _tlsPinController.text = '';
      _useTls = endpoints.kDefaultUseTls;
      _hasChanges = endpoints.kDefaultLightd != _originalEndpoint || (_originalTlsPin?.isNotEmpty ?? false);
    });
  }

  void _applySuggested(endpoints.LightdEndpoint endpoint) {
    setState(() {
      _endpointController.text = endpoint.displayString;
      _tlsPinController.text = endpoint.tlsPin ?? '';
      _useTls = endpoint.useTls;
      _hasChanges = _endpointController.text != _originalEndpoint ||
          _tlsPinController.text != (_originalTlsPin ?? '');
    });
  }

  @override
  Widget build(BuildContext context) {
    final endpointConfigAsync = ref.watch(lightdEndpointConfigProvider);
    final isNarrow = MediaQuery.of(context).size.width < 360;
    final basePadding = AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    
    return PScaffold(
      title: 'Node Configuration',
      appBar: PAppBar(
        title: 'Node Configuration',
        subtitle: 'Choose your lightwalletd endpoint',
        centerTitle: true,
        actions: [
          if (isNarrow)
            PIconButton(
              icon: Icon(Icons.refresh, color: AppColors.textSecondary),
              onPressed: _resetToDefault,
              tooltip: 'Reset',
            )
          else
            PTextButton(
              label: 'Reset',
              onPressed: _resetToDefault,
              variant: PTextButtonVariant.subtle,
            ),
        ],
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Form(
          key: _formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Current status card
              endpointConfigAsync.when(
                data: (config) => _buildStatusCard(config),
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (e, _) => _buildErrorCard(e.toString()),
              ),
              
              const SizedBox(height: AppSpacing.xl),

              // Suggested endpoints (Orchard-capable presets)
              Text(
                'SUGGESTED ENDPOINTS',
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                  fontWeight: FontWeight.w600,
                  letterSpacing: 1.2,
                ),
              ),
              const SizedBox(height: AppSpacing.md),
              Wrap(
                spacing: AppSpacing.md,
                runSpacing: AppSpacing.sm,
                children: [
                  for (final endpoint in endpoints.LightdEndpoint.suggested)
                    PButton(
                      text: endpoint.label ?? endpoint.displayString,
                      variant: PButtonVariant.ghost,
                      size: PButtonSize.small,
                      onPressed: () => _applySuggested(endpoint),
                    ),
                ],
              ),

              const SizedBox(height: AppSpacing.xl),
              
              // Endpoint input section
              Text(
                'LIGHTWALLETD ENDPOINT',
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
                      label: 'Endpoint (host:port)',
                      hint: 'lightd1.piratechain.com:9067',
                      keyboardType: TextInputType.url,
                      validator: _validateEndpoint,
                      onChanged: _onEndpointChanged,
                      prefixIcon: const Icon(Icons.dns_outlined),
                    ),
                    
                    const SizedBox(height: AppSpacing.lg),
                    
                    // TLS toggle
                    SwitchListTile(
                      title: const Text('Use TLS'),
                      subtitle: Text(
                        _useTls 
                            ? 'Encrypted connection (recommended)' 
                            : 'Unencrypted connection (not recommended)',
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
                          _hasChanges = true;
                        });
                      },
                      activeTrackColor: AppColors.accentPrimary.withValues(alpha: 0.4),
                      activeThumbColor: AppColors.accentPrimary,
                      contentPadding: EdgeInsets.zero,
                    ),
                  ],
                ),
              ),
              
              const SizedBox(height: AppSpacing.xl),
              
              // TLS pin section
              Text(
                'TLS CERTIFICATE PIN (OPTIONAL)',
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
                      controller: _tlsPinController,
                      label: 'SPKI Pin (base64)',
                      hint: 'Leave empty to skip certificate pinning',
                      validator: _validateTlsPin,
                      onChanged: _onTlsPinChanged,
                      prefixIcon: const Icon(Icons.lock_outline),
                      maxLines: 2,
                    ),
                    
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
                              'TLS pinning adds extra security by verifying the server\'s certificate. '
                              'This feature will be auto-configured in a future update.',
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
                  onPressed: _hasChanges && !_isLoading ? _saveEndpoint : null,
                  isLoading: _isLoading,
                  child: const Text('Save Changes'),
                ),
              ),
              
              const SizedBox(height: AppSpacing.lg),
              
              // Help text
              Text(
                'Changes will take effect immediately. The wallet will reconnect to the new endpoint.',
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
    );
  }

  Widget _buildStatusCard(ffi.LightdEndpointConfig config) {
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
                  color: AppColors.success,
                      boxShadow: [
                        BoxShadow(
                          color: AppColors.success.withValues(alpha: 0.4),
                          blurRadius: 8,
                          spreadRadius: 2,
                        ),
                      ],
                ),
              ),
              const SizedBox(width: AppSpacing.sm),
              Text(
                'Current Node',
                style: AppTypography.labelLarge.copyWith(
                  color: AppColors.textPrimary,
                ),
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
                  config.displayString,
                  style: AppTypography.bodyMedium.copyWith(
                    color: AppColors.textPrimary,
                    fontFamily: 'monospace',
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: config.url));
                  PSnack.show(
                    context: context,
                    message: 'Endpoint copied',
                    variant: PSnackVariant.info,
                  );
                },
                tooltip: 'Copy endpoint',
                visualDensity: VisualDensity.compact,
              ),
            ],
          ),
          
          const SizedBox(height: AppSpacing.sm),
          
          Wrap(
            spacing: AppSpacing.xs,
            runSpacing: AppSpacing.xs,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Icon(
                config.useTls ? Icons.lock : Icons.lock_open,
                color: config.useTls ? AppColors.success : AppColors.warning,
                size: 16,
              ),
              Text(
                config.useTls ? 'TLS Enabled' : 'TLS Disabled',
                style: AppTypography.bodySmall.copyWith(
                  color: config.useTls ? AppColors.success : AppColors.warning,
                ),
              ),
              if (config.tlsPin != null) ...[
                const SizedBox(width: AppSpacing.md),
                Icon(
                  Icons.verified_user,
                  color: AppColors.accentPrimary,
                  size: 16,
                ),
                Text(
                  'Certificate Pinned',
                  style: AppTypography.bodySmall.copyWith(
                    color: AppColors.accentPrimary,
                  ),
                ),
              ],
            ],
          ),
        ],
      ),
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
              'Failed to load endpoint: $error',
              style: AppTypography.bodySmall.copyWith(color: AppColors.error),
            ),
          ),
        ],
      ),
    );
  }
}

