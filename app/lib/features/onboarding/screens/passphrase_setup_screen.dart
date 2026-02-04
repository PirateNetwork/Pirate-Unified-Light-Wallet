// Passphrase setup screen

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/screenshot_protection.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../widgets/onboarding_progress_indicator.dart';

/// Passphrase strength
enum PassphraseStrength {
  weak,
  fair,
  good,
  strong,
  veryStrong,
}

/// Passphrase setup screen
class PassphraseSetupScreen extends ConsumerStatefulWidget {
  const PassphraseSetupScreen({super.key});

  @override
  ConsumerState<PassphraseSetupScreen> createState() =>
      _PassphraseSetupScreenState();
}

class _PassphraseSetupScreenState
    extends ConsumerState<PassphraseSetupScreen> {
  final _passphraseController = TextEditingController();
  final _confirmController = TextEditingController();
  bool _obscurePassphrase = true;
  bool _obscureConfirm = true;
  bool _isSaving = false;
  String? _error;
  ScreenProtection? _screenProtection;

  PassphraseStrength _strength = PassphraseStrength.weak;
  bool _passwordsMatch = false;

  @override
  void initState() {
    super.initState();
    _passphraseController.addListener(_onPassphraseChanged);
    _confirmController.addListener(_onConfirmChanged);
    _disableScreenshots();
  }

  @override
  void dispose() {
    _passphraseController.dispose();
    _confirmController.dispose();
    _enableScreenshots();
    super.dispose();
  }

  void _disableScreenshots() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _enableScreenshots() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  void _onPassphraseChanged() {
    setState(() {
      _strength = _calculateStrength(_passphraseController.text);
      _passwordsMatch = _passphraseController.text == _confirmController.text;
    });
  }

  void _onConfirmChanged() {
    setState(() {
      _passwordsMatch = _passphraseController.text == _confirmController.text;
    });
  }

  PassphraseStrength _calculateStrength(String password) {
    if (password.isEmpty) return PassphraseStrength.weak;

    int score = 0;

    // Length
    if (password.length >= 8) score++;
    if (password.length >= 12) score++;
    if (password.length >= 16) score++;

    // Complexity
    if (RegExp('[a-z]').hasMatch(password)) score++;
    if (RegExp('[A-Z]').hasMatch(password)) score++;
    if (RegExp('[0-9]').hasMatch(password)) score++;
    if (RegExp(r'[!@#$%^&*(),.?":{}|<>]').hasMatch(password)) score++;

    if (score <= 2) return PassphraseStrength.weak;
    if (score <= 4) return PassphraseStrength.fair;
    if (score <= 5) return PassphraseStrength.good;
    if (score <= 6) return PassphraseStrength.strong;
    return PassphraseStrength.veryStrong;
  }

  Color _getStrengthColor() {
    switch (_strength) {
      case PassphraseStrength.weak:
        return AppColors.error;
      case PassphraseStrength.fair:
        return AppColors.warning;
      case PassphraseStrength.good:
        return AppColors.accentSecondary;
      case PassphraseStrength.strong:
        return AppColors.success;
      case PassphraseStrength.veryStrong:
        return AppColors.accentPrimary;
    }
  }

  String _getStrengthText() {
    switch (_strength) {
      case PassphraseStrength.weak:
        return 'Weak';
      case PassphraseStrength.fair:
        return 'Fair';
      case PassphraseStrength.good:
        return 'Good';
      case PassphraseStrength.strong:
        return 'Strong';
      case PassphraseStrength.veryStrong:
        return 'Very strong';
    }
  }

  bool _canProceed() {
    return _passphraseController.text.isNotEmpty &&
           _confirmController.text.isNotEmpty &&
           _passwordsMatch &&
           _strength.index >= PassphraseStrength.good.index &&
           !_isPalindrome(_passphraseController.text);
  }

  bool _isPalindrome(String value) {
    if (value.isEmpty) return false;
    final reversed = value.split('').reversed.join();
    return value == reversed;
  }

  Future<void> _proceed() async {
    if (!_canProceed() || _isSaving) return;

    setState(() {
      _isSaving = true;
      _error = null;
    });

    try {
      await FfiBridge.setAppPassphrase(_passphraseController.text);
      ref.read(onboardingControllerProvider.notifier)
          .setPassphrase(_passphraseController.text);
      ref.read(onboardingControllerProvider.notifier).nextStep();
      if (mounted) {
        unawaited(context.push('/onboarding/biometrics'));
      }
    } catch (e) {
      setState(() {
        _error = e.toString().replaceFirst('Exception: ', '');
      });
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final onboardingState = ref.watch(onboardingControllerProvider);
    final isImport = onboardingState.mode == OnboardingMode.import;
    final totalSteps = isImport ? 5 : 6;
    final currentStep = isImport ? 3 : 2;

    final basePadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: AppSpacing.xl,
    );
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return PScaffold(
      title: 'Set a passphrase',
      appBar: const PAppBar(
        title: 'Set a passphrase',
        subtitle: 'Unlocks this wallet on this device',
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
              'Choose a strong passphrase',
              style: AppTypography.h2.copyWith(
                color: AppColors.textPrimary,
              ),
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              'This encrypts your wallet on this device. You will need it each time you open the app.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
            const SizedBox(height: AppSpacing.xxl),

            // Passphrase input
            PInput(
              controller: _passphraseController,
              label: 'Passphrase',
              hint: 'Enter a passphrase',
              obscureText: _obscurePassphrase,
              autocorrect: false,
              enableSuggestions: false,
              suffixIcon: IconButton(
                icon: Icon(
                  _obscurePassphrase ? Icons.visibility : Icons.visibility_off,
                ),
                onPressed: () {
                  setState(() {
                    _obscurePassphrase = !_obscurePassphrase;
                  });
                },
                tooltip:
                    _obscurePassphrase ? 'Show passphrase' : 'Hide passphrase',
              ),
            ),

            const SizedBox(height: AppSpacing.sm),

            // Strength indicator
            if (_passphraseController.text.isNotEmpty) ...[
              Row(
                children: [
                  Expanded(
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: LinearProgressIndicator(
                        value: (_strength.index + 1) / 5,
                        backgroundColor: AppColors.surfaceElevated,
                        valueColor: AlwaysStoppedAnimation<Color>(
                          _getStrengthColor(),
                        ),
                        minHeight: 6,
                        semanticsLabel: 'Passphrase strength',
                        semanticsValue: _getStrengthText(),
                      ),
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Text(
                    _getStrengthText(),
                    style: AppTypography.caption.copyWith(
                      color: _getStrengthColor(),
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppSpacing.lg),
            ],

            const SizedBox(height: AppSpacing.md),

            // Confirm passphrase input
            PInput(
              controller: _confirmController,
              label: 'Confirm passphrase',
              hint: 'Re-enter to confirm',
              obscureText: _obscureConfirm,
              autocorrect: false,
              enableSuggestions: false,
              errorText: _confirmController.text.isNotEmpty && !_passwordsMatch
                  ? 'Passphrases do not match'
                  : null,
              suffixIcon: IconButton(
                icon: Icon(
                  _obscureConfirm ? Icons.visibility : Icons.visibility_off,
                ),
                onPressed: () {
                  setState(() {
                    _obscureConfirm = !_obscureConfirm;
                  });
                },
                tooltip:
                    _obscureConfirm ? 'Show passphrase' : 'Hide passphrase',
              ),
            ),

            const SizedBox(height: AppSpacing.xl),

            // Requirements checklist
            _RequirementsList(
              passphrase: _passphraseController.text,
            ),

            const SizedBox(height: AppSpacing.xxl),

            // Security notice
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
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(
                    Icons.warning_amber_rounded,
                    color: AppColors.error,
                    size: 24,
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Text(
                      'Never share your passphrase. We cannot recover it.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.textPrimary,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ],
              ),
            ),

            const SizedBox(height: AppSpacing.xl),

            // Continue button
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
            const SizedBox(height: AppSpacing.xl),
            PButton(
              text: _isSaving ? 'Saving...' : 'Continue',
              onPressed: _canProceed() && !_isSaving ? _proceed : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isSaving,
            ),
            ],
          ),
        ),
      );
  }
}

/// Requirements list widget
class _RequirementsList extends StatelessWidget {
  final String passphrase;

  const _RequirementsList({required this.passphrase});

  @override
  Widget build(BuildContext context) {
    final reversed = passphrase.split('').reversed.join();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Requirements',
          style: AppTypography.bodyBold.copyWith(
            color: AppColors.textPrimary,
          ),
        ),
        const SizedBox(height: AppSpacing.sm),
        _RequirementItem(
          text: 'At least 12 characters',
          met: passphrase.length >= 12,
        ),
        _RequirementItem(
          text: 'Uppercase letter',
          met: RegExp('[A-Z]').hasMatch(passphrase),
        ),
        _RequirementItem(
          text: 'Lowercase letter',
          met: RegExp('[a-z]').hasMatch(passphrase),
        ),
        _RequirementItem(
          text: 'Number',
          met: RegExp('[0-9]').hasMatch(passphrase),
        ),
        _RequirementItem(
          text: 'Symbol',
          met: RegExp(r'[!@#$%^&*(),.?":{}|<>]').hasMatch(passphrase),
        ),
        _RequirementItem(
          text: 'Not the same backwards',
          met: passphrase.isNotEmpty && passphrase != reversed,
        ),
      ],
    );
  }
}

/// Requirement item widget
class _RequirementItem extends StatelessWidget {
  final String text;
  final bool met;

  const _RequirementItem({
    required this.text,
    required this.met,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSpacing.xs),
      child: Row(
        children: [
          Icon(
            met ? Icons.check_circle : Icons.circle_outlined,
            size: 20,
            color: met ? AppColors.success : AppColors.textTertiary,
            semanticLabel: met ? 'Requirement met' : 'Requirement not met',
          ),
          const SizedBox(width: AppSpacing.sm),
          Text(
            text,
            style: AppTypography.body.copyWith(
              color: met ? AppColors.textPrimary : AppColors.textSecondary,
            ),
          ),
        ],
      ),
    );
  }
}
