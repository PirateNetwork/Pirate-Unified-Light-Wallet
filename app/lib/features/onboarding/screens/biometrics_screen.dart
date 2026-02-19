/// Onboarding biometrics screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../core/security/biometric_auth.dart';
import '../../../core/security/passphrase_cache.dart';
import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../widgets/onboarding_progress_indicator.dart';
import '../../../core/i18n/arb_text_localizer.dart';

class OnboardingBiometricsScreen extends ConsumerStatefulWidget {
  const OnboardingBiometricsScreen({super.key});

  @override
  ConsumerState<OnboardingBiometricsScreen> createState() =>
      _OnboardingBiometricsScreenState();
}

class _OnboardingBiometricsScreenState
    extends ConsumerState<OnboardingBiometricsScreen> {
  bool _isAvailable = false;
  bool _isLoading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _checkAvailability();
  }

  Future<void> _checkAvailability() async {
    final available = await BiometricAuth.isAvailable();
    if (mounted) {
      setState(() => _isAvailable = available);
    }
  }

  Future<void> _enableBiometrics() async {
    if (_isLoading) return;
    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final authenticated = await BiometricAuth.authenticate(
        reason: 'Enable biometric unlock for Pirate Wallet',
        biometricOnly: true,
      );
      if (!authenticated) {
        setState(() => _error = 'Biometric authentication failed.');
        return;
      }

      final state = ref.read(onboardingControllerProvider);
      final passphrase = state.passphrase;
      if (passphrase == null || passphrase.isEmpty) {
        setState(() {
          _error =
              'Biometrics need your passphrase once to finish setup. You can enable it later in Settings.';
        });
        return;
      }

      await PassphraseCache.store(passphrase);
      await ref
          .read(biometricsEnabledProvider.notifier)
          .setEnabled(enabled: true);
      ref
          .read(onboardingControllerProvider.notifier)
          .setBiometrics(enabled: true);
      _proceed();
    } catch (e) {
      if (mounted) {
        setState(() => _error = _formatEnableError(e));
      }
      debugPrint('Failed to enable onboarding biometrics: $e');
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  String _formatEnableError(Object error) {
    final raw = error.toString();
    final lowered = raw.toLowerCase();
    if (lowered.contains('-34018') ||
        lowered.contains('required entitlement') ||
        lowered.contains('keychain')) {
      return 'Secure storage is unavailable for this macOS build. '
          'Install the latest build and try again.';
    }
    return 'Unable to enable biometrics.';
  }

  Future<void> _skip() async {
    if (_isLoading) return;

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      await ref
          .read(biometricsEnabledProvider.notifier)
          .setEnabled(enabled: false);

      ref
          .read(onboardingControllerProvider.notifier)
          .setBiometrics(enabled: false);

      if (mounted) {
        _proceed();
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error =
              'Unable to persist biometric preference right now. Please try again.';
        });
      }
      debugPrint('Failed to complete onboarding biometric skip: $e');
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  void _proceed() {
    final state = ref.read(onboardingControllerProvider);
    ref.read(onboardingControllerProvider.notifier).nextStep();

    // Route based on mode: create goes to backup warning, import shows birthday picker
    if (state.mode == OnboardingMode.create) {
      // For create mode, go to backup warning screen
      context.push('/onboarding/backup-warning');
    } else {
      // For import mode, show birthday picker
      context.push('/onboarding/birthday');
    }
  }

  @override
  Widget build(BuildContext context) {
    final onboardingState = ref.watch(onboardingControllerProvider);
    final isImport = onboardingState.mode == OnboardingMode.import;
    final totalSteps = isImport ? 5 : 6;
    final currentStep = isImport ? 4 : 3;
    final contentPadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: AppSpacing.xl,
    );

    return PScaffold(
      title: 'Biometrics'.tr,
      appBar: PAppBar(
        title: 'Enable biometrics'.tr,
        subtitle: 'Faster unlock with device security'.tr,
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            OnboardingProgressIndicator(
              currentStep: currentStep,
              totalSteps: totalSteps,
            ),
            const SizedBox(height: AppSpacing.xxl),
            Text(
              'Use biometrics to unlock'.tr,
              style: AppTypography.h2.copyWith(
                color: AppColors.textPrimary,
              ),
            ),
            const SizedBox(height: AppSpacing.sm),
            Text(
              'You can still unlock with your passphrase. '
                      'Biometrics never leave your device.'
                  .tr,
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
            const SizedBox(height: AppSpacing.xxl),
            if (_error != null)
              Container(
                padding: const EdgeInsets.all(AppSpacing.md),
                margin: const EdgeInsets.only(bottom: AppSpacing.lg),
                decoration: BoxDecoration(
                  color: AppColors.error.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(
                    color: AppColors.error.withValues(alpha: 0.3),
                  ),
                ),
                child: Row(
                  children: [
                    Icon(
                      Icons.error_outline,
                      color: AppColors.error,
                      size: 18,
                    ),
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
            PButton(
              text: _isAvailable ? 'Enable biometrics' : 'Biometrics unavailable',
              onPressed: !_isAvailable || _isLoading ? null : _enableBiometrics,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isLoading,
            ),
            const SizedBox(height: AppSpacing.md),
            PTextButton(
              label: 'Skip for now'.tr,
              onPressed: _isLoading ? null : _skip,
              variant: PTextButtonVariant.subtle,
            ),
          ],
        ),
      ),
    );
  }
}
