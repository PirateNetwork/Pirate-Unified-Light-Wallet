/// Biometrics settings screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_auth/local_auth.dart';

import '../../../core/security/biometric_auth.dart';
import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

class BiometricsScreen extends ConsumerStatefulWidget {
  const BiometricsScreen({super.key});

  @override
  ConsumerState<BiometricsScreen> createState() => _BiometricsScreenState();
}

class _BiometricsScreenState extends ConsumerState<BiometricsScreen> {
  bool _isLoading = false;
  bool _isAvailable = false;
  List<BiometricType> _types = [];
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadAvailability();
  }

  Future<void> _loadAvailability() async {
    final available = await BiometricAuth.isAvailable();
    final types = await BiometricAuth.getAvailableBiometrics();
    if (mounted) {
      setState(() {
        _isAvailable = available;
        _types = types;
      });
    }
  }

  Future<void> _toggleBiometrics(bool enable) async {
    if (_isLoading) return;

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      if (!enable) {
        await ref.read(biometricsEnabledProvider.notifier).setEnabled(enabled: false);
        return;
      }

      final available = await BiometricAuth.isAvailable();
      if (!available) {
        setState(() => _error = 'Biometrics are not available on this device.');
        return;
      }

      final authenticated = await BiometricAuth.authenticate(
        reason: 'Enable biometric unlock for Pirate Wallet',
        biometricOnly: true,
      );
      if (!authenticated) {
        setState(() => _error = 'Biometric authentication failed.');
        return;
      }

      await ref.read(biometricsEnabledProvider.notifier).setEnabled(enabled: true);
    } catch (e) {
      setState(() => _error = 'Unable to update biometrics settings.');
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final biometricsEnabled = ref.watch(biometricsEnabledProvider);
    final typeLabels = _types
        .map(BiometricAuth.getBiometricName)
        .toList(growable: false);
    final typeSummary =
        typeLabels.isEmpty ? 'None detected' : typeLabels.join(', ');

    return PScaffold(
      title: 'Biometrics',
      appBar: const PAppBar(
        title: 'Biometrics',
        subtitle: 'Use your device biometrics to unlock',
        showBackButton: true,
        centerTitle: true,
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(AppSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
              PCard(
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.md),
                child: Row(
                  children: [
                    Icon(
                      Icons.fingerprint,
                      color: AppColors.accentPrimary,
                    ),
                    const SizedBox(width: AppSpacing.sm),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            _isAvailable ? 'Available' : 'Unavailable',
                            style: AppTypography.bodyBold.copyWith(
                              color: AppColors.textPrimary,
                            ),
                          ),
                          const SizedBox(height: AppSpacing.xxs),
                          Text(
                            _isAvailable ? 'Access your wallet faster' : typeSummary,
                            style: AppTypography.caption.copyWith(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ],
                      ),
                    ),
                    Switch.adaptive(
                      value: biometricsEnabled,
                      onChanged: !_isAvailable || _isLoading
                          ? null
                          : _toggleBiometrics,
                      activeTrackColor: AppColors.accentPrimary.withValues(alpha: 0.4),
                      activeThumbColor: AppColors.accentPrimary,
                    ),
                  ],
                ),
              ),
            ),
            if (_error != null) ...[
              const SizedBox(height: AppSpacing.md),
              Container(
                padding: const EdgeInsets.all(AppSpacing.md),
                decoration: BoxDecoration(
                  color: AppColors.error.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(
                    color: AppColors.error.withValues(alpha: 0.3),
                  ),
                ),
                child: Row(
                  children: [
                    Icon(Icons.error_outline, color: AppColors.error, size: 18),
                    const SizedBox(width: AppSpacing.sm),
                    Expanded(
                      child: Text(
                        _error!,
                        style: AppTypography.body.copyWith(
                          color: AppColors.error,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
            const SizedBox(height: AppSpacing.lg),
            Text(
              'Biometric data never leaves your device.',
              style: AppTypography.caption.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
