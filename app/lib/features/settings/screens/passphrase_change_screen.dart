// Passphrase change screen

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/biometric_auth.dart';
import '../../../core/security/duress_passphrase_store.dart';
import '../../../core/security/passphrase_cache.dart';
import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

enum _PassphraseStrength { weak, fair, good, strong, veryStrong }

enum _AuthMethod { passphrase, biometrics }

class PassphraseChangeScreen extends ConsumerStatefulWidget {
  const PassphraseChangeScreen({super.key});

  @override
  ConsumerState<PassphraseChangeScreen> createState() =>
      _PassphraseChangeScreenState();
}

class _PassphraseChangeScreenState
    extends ConsumerState<PassphraseChangeScreen> {
  final _currentController = TextEditingController();
  final _newController = TextEditingController();
  final _confirmController = TextEditingController();

  bool _authenticated = false;
  _AuthMethod? _authMethod;
  bool _isVerifying = false;
  bool _isSaving = false;
  bool _obscureCurrent = true;
  bool _obscureNew = true;
  bool _obscureConfirm = true;
  String? _error;
  _PassphraseStrength _strength = _PassphraseStrength.weak;
  bool _passwordsMatch = false;

  @override
  void initState() {
    super.initState();
    _newController.addListener(_onNewPassphraseChanged);
    _confirmController.addListener(_onConfirmChanged);
  }

  @override
  void dispose() {
    _currentController.dispose();
    _newController.dispose();
    _confirmController.dispose();
    super.dispose();
  }

  void _onNewPassphraseChanged() {
    setState(() {
      _strength = _calculateStrength(_newController.text);
      _passwordsMatch = _newController.text == _confirmController.text;
    });
  }

  void _onConfirmChanged() {
    setState(() {
      _passwordsMatch = _newController.text == _confirmController.text;
    });
  }

  _PassphraseStrength _calculateStrength(String password) {
    if (password.isEmpty) return _PassphraseStrength.weak;

    int score = 0;
    if (password.length >= 8) score++;
    if (password.length >= 12) score++;
    if (password.length >= 16) score++;

    if (RegExp('[a-z]').hasMatch(password)) score++;
    if (RegExp('[A-Z]').hasMatch(password)) score++;
    if (RegExp('[0-9]').hasMatch(password)) score++;
    if (RegExp(r'[!@#$%^&*(),.?":{}|<>]').hasMatch(password)) score++;

    if (score <= 2) return _PassphraseStrength.weak;
    if (score <= 4) return _PassphraseStrength.fair;
    if (score <= 5) return _PassphraseStrength.good;
    if (score <= 6) return _PassphraseStrength.strong;
    return _PassphraseStrength.veryStrong;
  }

  bool _meetsRequirements(String password) {
    if (password.length < 12) return false;
    if (_isPalindrome(password)) return false;
    if (!RegExp('[A-Z]').hasMatch(password)) return false;
    if (!RegExp('[a-z]').hasMatch(password)) return false;
    if (!RegExp('[0-9]').hasMatch(password)) return false;
    if (!RegExp(r'[!@#$%^&*(),.?":{}|<>]').hasMatch(password)) return false;
    return true;
  }

  bool _isPalindrome(String value) {
    if (value.isEmpty) return false;
    final reversed = value.split('').reversed.join();
    return value == reversed;
  }

  Color _strengthColor() {
    switch (_strength) {
      case _PassphraseStrength.weak:
        return AppColors.error;
      case _PassphraseStrength.fair:
        return AppColors.warning;
      case _PassphraseStrength.good:
        return AppColors.accentSecondary;
      case _PassphraseStrength.strong:
        return AppColors.success;
      case _PassphraseStrength.veryStrong:
        return AppColors.accentPrimary;
    }
  }

  String _strengthText() {
    switch (_strength) {
      case _PassphraseStrength.weak:
        return 'Weak';
      case _PassphraseStrength.fair:
        return 'Fair';
      case _PassphraseStrength.good:
        return 'Good';
      case _PassphraseStrength.strong:
        return 'Strong';
      case _PassphraseStrength.veryStrong:
        return 'Very strong';
    }
  }

  bool get _canSave {
    return _newController.text.isNotEmpty &&
        _confirmController.text.isNotEmpty &&
        _passwordsMatch &&
        _strength.index >= _PassphraseStrength.good.index &&
        _meetsRequirements(_newController.text);
  }

  Future<void> _verifyCurrentPassphrase() async {
    if (_isVerifying) return;
    if (_currentController.text.trim().isEmpty) {
      setState(() => _error = 'Enter your current passphrase.');
      return;
    }

    setState(() {
      _isVerifying = true;
      _error = null;
    });

    try {
      final isValid = await FfiBridge.verifyAppPassphrase(
        _currentController.text.trim(),
      );
      if (!isValid) {
        setState(() => _error = 'Current passphrase is incorrect.');
        return;
      }

      setState(() {
        _authenticated = true;
        _authMethod = _AuthMethod.passphrase;
      });
    } catch (e) {
      setState(() => _error = 'Unable to verify passphrase.');
    } finally {
      if (mounted) {
        setState(() => _isVerifying = false);
      }
    }
  }

  Future<void> _authenticateBiometric() async {
    if (_isVerifying) return;
    setState(() {
      _isVerifying = true;
      _error = null;
    });

    try {
      final authenticated = await BiometricAuth.authenticate(
        reason: 'Authorize passphrase change',
        biometricOnly: true,
      );
      if (!authenticated) {
        setState(() => _error = 'Biometric authentication was cancelled.');
        return;
      }

      setState(() {
        _authenticated = true;
        _authMethod = _AuthMethod.biometrics;
      });
    } on BiometricException catch (e) {
      setState(() => _error = e.message);
    } catch (e) {
      setState(() => _error = 'Biometric authentication failed: $e');
    } finally {
      if (mounted) {
        setState(() => _isVerifying = false);
      }
    }
  }

  Future<void> _savePassphrase() async {
    if (!_canSave || _isSaving) return;

    setState(() {
      _isSaving = true;
      _error = null;
    });

    try {
      if (_authMethod == _AuthMethod.biometrics) {
        await FfiBridge.changeAppPassphraseWithCached(
          newPassphrase: _newController.text,
        );
      } else {
        await FfiBridge.changeAppPassphrase(
          currentPassphrase: _currentController.text.trim(),
          newPassphrase: _newController.text,
        );
      }

      if (ref.read(biometricsEnabledProvider)) {
        try {
          await PassphraseCache.store(_newController.text);
        } catch (e) {
          debugPrint(
            'Failed to refresh wrapped passphrase cache after passphrase change: $e',
          );
          try {
            await ref
                .read(biometricsEnabledProvider.notifier)
                .setEnabled(enabled: false);
          } catch (disableError) {
            debugPrint(
              'Failed to disable biometrics after cache refresh failure: $disableError',
            );
          }
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: const Text(
                  'Passphrase updated. Biometrics were turned off because secure setup failed.',
                ),
                behavior: SnackBarBehavior.floating,
              ),
            );
          }
        }
      }

      try {
        final duressHash = await FfiBridge.getDuressPassphraseHash();
        if (duressHash == null || duressHash.isEmpty) {
          await DuressPassphraseStore.clear();
        } else {
          await DuressPassphraseStore.store(duressHash);
        }
      } catch (_) {
        // Ignore; duress passphrase can be reconfigured if needed.
      }

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Passphrase updated.'),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
        context.pop();
      }
    } catch (e) {
      setState(() => _error = 'Passphrase update failed.');
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final biometricsEnabled = ref.watch(biometricsEnabledProvider);
    final biometricAvailable = ref
        .watch(biometricAvailabilityProvider)
        .maybeWhen(data: (available) => available, orElse: () => false);
    final showBiometric = biometricsEnabled && biometricAvailable;
    final basePadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: AppSpacing.xl,
    );
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );

    return PScaffold(
      title: 'Change passphrase',
      appBar: const PAppBar(
        title: 'Change passphrase',
        subtitle: 'Update your app unlock passphrase',
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (!_authenticated) ...[
              Text(
                'Verify your identity',
                style: AppTypography.h2.copyWith(color: AppColors.textPrimary),
              ),
              const SizedBox(height: AppSpacing.sm),
              Text(
                'Confirm with biometrics or your current passphrase.',
                style: AppTypography.body.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
              const SizedBox(height: AppSpacing.xl),
              if (showBiometric) ...[
                PButton(
                  text: 'Use biometrics',
                  onPressed: _isVerifying ? null : _authenticateBiometric,
                  variant: PButtonVariant.primary,
                  size: PButtonSize.large,
                  isLoading: _isVerifying,
                ),
                const SizedBox(height: AppSpacing.lg),
                Text(
                  'Or verify with passphrase',
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                  ),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: AppSpacing.md),
              ],
              PInput(
                controller: _currentController,
                label: 'Current passphrase',
                hint: 'Enter your current passphrase',
                obscureText: _obscureCurrent,
                suffixIcon: IconButton(
                  icon: Icon(
                    _obscureCurrent
                        ? Icons.visibility_outlined
                        : Icons.visibility_off_outlined,
                    color: AppColors.textSecondary,
                  ),
                  onPressed: () {
                    setState(() => _obscureCurrent = !_obscureCurrent);
                  },
                ),
              ),
              const SizedBox(height: AppSpacing.md),
              PButton(
                text: 'Continue',
                onPressed: _isVerifying ? null : _verifyCurrentPassphrase,
                variant: PButtonVariant.primary,
                size: PButtonSize.large,
                isLoading: _isVerifying,
              ),
            ] else ...[
              Text(
                'Choose a new passphrase',
                style: AppTypography.h2.copyWith(color: AppColors.textPrimary),
              ),
              const SizedBox(height: AppSpacing.sm),
              Text(
                'Use at least 12 characters with letters, numbers, and symbols.',
                style: AppTypography.body.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
              const SizedBox(height: AppSpacing.xl),
              PInput(
                controller: _newController,
                label: 'New passphrase',
                hint: 'Enter a new passphrase',
                obscureText: _obscureNew,
                suffixIcon: IconButton(
                  icon: Icon(
                    _obscureNew
                        ? Icons.visibility_outlined
                        : Icons.visibility_off_outlined,
                    color: AppColors.textSecondary,
                  ),
                  onPressed: () {
                    setState(() => _obscureNew = !_obscureNew);
                  },
                ),
              ),
              const SizedBox(height: AppSpacing.sm),
              if (_newController.text.isNotEmpty) ...[
                Row(
                  children: [
                    Expanded(
                      child: ClipRRect(
                        borderRadius: BorderRadius.circular(4),
                        child: LinearProgressIndicator(
                          value: (_strength.index + 1) / 5,
                          backgroundColor: AppColors.surfaceElevated,
                          valueColor: AlwaysStoppedAnimation<Color>(
                            _strengthColor(),
                          ),
                          minHeight: 6,
                        ),
                      ),
                    ),
                    const SizedBox(width: AppSpacing.sm),
                    Text(
                      _strengthText(),
                      style: AppTypography.caption.copyWith(
                        color: _strengthColor(),
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: AppSpacing.lg),
              ],
              PInput(
                controller: _confirmController,
                label: 'Confirm new passphrase',
                hint: 'Re-enter to confirm',
                obscureText: _obscureConfirm,
                errorText:
                    _confirmController.text.isNotEmpty && !_passwordsMatch
                    ? 'Passphrases do not match'
                    : null,
                suffixIcon: IconButton(
                  icon: Icon(
                    _obscureConfirm
                        ? Icons.visibility_outlined
                        : Icons.visibility_off_outlined,
                    color: AppColors.textSecondary,
                  ),
                  onPressed: () {
                    setState(() => _obscureConfirm = !_obscureConfirm);
                  },
                ),
              ),
              const SizedBox(height: AppSpacing.lg),
              PButton(
                text: 'Update passphrase',
                onPressed: _canSave && !_isSaving ? _savePassphrase : null,
                variant: PButtonVariant.primary,
                size: PButtonSize.large,
                isLoading: _isSaving,
              ),
            ],
            if (_error != null) ...[
              const SizedBox(height: AppSpacing.lg),
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
          ],
        ),
      ),
    );
  }
}
