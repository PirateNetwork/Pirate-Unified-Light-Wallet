/// Biometrics settings screen
library;

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_auth/local_auth.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/biometric_auth.dart';
import '../../../core/security/passphrase_cache.dart';
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
        await ref
            .read(biometricsEnabledProvider.notifier)
            .setEnabled(enabled: false);
        ref.invalidate(resolvedBiometricsEnabledProvider);
        return;
      }

      final available = await BiometricAuth.isAvailable();
      if (!available) {
        setState(() => _error = 'Biometrics are not available on this device.');
        return;
      }

      // macOS unlock cache reads are guarded by Keychain biometry ACL, so we
      // avoid a redundant local_auth prompt during setup to keep UX single-step.
      if (!Platform.isMacOS) {
        final authenticated = await BiometricAuth.authenticate(
          reason: 'Enable biometric unlock for Pirate Wallet',
          biometricOnly: true,
        );
        if (!authenticated) {
          setState(() => _error = 'Biometric authentication was cancelled.');
          return;
        }
      }

      final ready = await _ensurePassphraseCacheReady();
      if (!ready) {
        setState(() {
          _error =
              'Biometrics need your passphrase once to finish setup. Try enabling again.';
        });
        return;
      }

      await ref
          .read(biometricsEnabledProvider.notifier)
          .setEnabled(enabled: true);
      ref.invalidate(resolvedBiometricsEnabledProvider);
    } on BiometricException catch (e) {
      setState(() => _error = e.message);
    } catch (e) {
      setState(() => _error = _formatBiometricToggleError(e));
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  String _formatBiometricToggleError(Object error) {
    final raw = error.toString();
    final lowered = raw.toLowerCase();
    if (lowered.contains('-34018') ||
        lowered.contains('required entitlement') ||
        lowered.contains('keychain')) {
      return 'Secure storage is unavailable for this macOS build. '
          'Install a build with Keychain entitlements and try again.';
    }
    return 'Unable to update biometrics settings: $raw';
  }

  Future<bool> _ensurePassphraseCacheReady() async {
    if (await PassphraseCache.exists()) {
      return true;
    }

    final passphrase = await _promptPassphraseForBiometrics();
    if (passphrase == null || passphrase.isEmpty) {
      return false;
    }

    final verified = await FfiBridge.verifyAppPassphrase(passphrase);
    if (!verified) {
      throw Exception('Passphrase verification failed.');
    }

    await PassphraseCache.store(passphrase);
    return true;
  }

  Future<String?> _promptPassphraseForBiometrics() async {
    final controller = TextEditingController();
    String? errorText;
    String? result;

    await showDialog<void>(
      context: context,
      barrierDismissible: true,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setStateDialog) {
            return AlertDialog(
              backgroundColor: AppColors.backgroundElevated,
              title: Text(
                'Confirm passphrase',
                style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
              ),
              content: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Enter your passphrase once to finish biometric setup.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.md),
                  TextField(
                    controller: controller,
                    obscureText: true,
                    autofocus: true,
                    style: AppTypography.body.copyWith(
                      color: AppColors.textPrimary,
                    ),
                    decoration: InputDecoration(
                      hintText: 'Passphrase',
                      hintStyle: AppTypography.body.copyWith(
                        color: AppColors.textSecondary,
                      ),
                      errorText: errorText,
                    ),
                    onSubmitted: (_) {
                      final value = controller.text.trim();
                      if (value.isEmpty) {
                        setStateDialog(
                          () => errorText = 'Passphrase is required.',
                        );
                        return;
                      }
                      result = value;
                      Navigator.of(context).pop();
                    },
                  ),
                ],
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: Text(
                    'Cancel',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                ),
                TextButton(
                  onPressed: () {
                    final value = controller.text.trim();
                    if (value.isEmpty) {
                      setStateDialog(
                        () => errorText = 'Passphrase is required.',
                      );
                      return;
                    }
                    result = value;
                    Navigator.of(context).pop();
                  },
                  child: Text(
                    'Confirm',
                    style: AppTypography.bodyBold.copyWith(
                      color: AppColors.accentPrimary,
                    ),
                  ),
                ),
              ],
            );
          },
        );
      },
    );

    controller.dispose();
    return result;
  }

  @override
  Widget build(BuildContext context) {
    final biometricsEnabled = ref.watch(biometricsEnabledProvider);
    final resolvedBiometricsEnabled = ref.watch(
      resolvedBiometricsEnabledProvider,
    );
    final typeLabels = _types
        .map(BiometricAuth.getBiometricName)
        .toList(growable: false);
    final typeSummary = typeLabels.isEmpty
        ? 'None detected'
        : typeLabels.join(', ');

    return PScaffold(
      title: 'Biometrics',
      appBar: const PAppBar(
        title: 'Biometrics',
        subtitle: 'Use your device biometrics to unlock',
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.md),
                child: Row(
                  children: [
                    Icon(Icons.fingerprint, color: AppColors.accentPrimary),
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
                            _isAvailable
                                ? 'Access your wallet faster'
                                : typeSummary,
                            style: AppTypography.caption.copyWith(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ],
                      ),
                    ),
                    resolvedBiometricsEnabled.when(
                      data: (_) => Switch.adaptive(
                        value: biometricsEnabled,
                        onChanged: !_isAvailable || _isLoading
                            ? null
                            : _toggleBiometrics,
                        activeTrackColor: AppColors.accentPrimary.withValues(
                          alpha: 0.4,
                        ),
                        activeThumbColor: AppColors.accentPrimary,
                      ),
                      loading: () => const SizedBox(
                        width: 20,
                        height: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                      error: (_, _) => Switch.adaptive(
                        value: biometricsEnabled,
                        onChanged: !_isAvailable || _isLoading
                            ? null
                            : _toggleBiometrics,
                        activeTrackColor: AppColors.accentPrimary.withValues(
                          alpha: 0.4,
                        ),
                        activeThumbColor: AppColors.accentPrimary,
                      ),
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
