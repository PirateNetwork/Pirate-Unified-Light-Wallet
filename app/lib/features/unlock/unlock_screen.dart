// Unlock Screen - Enter passphrase to unlock wallet

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/providers/wallet_providers.dart';
import '../../features/settings/providers/preferences_providers.dart';
import '../../core/security/biometric_auth.dart';
import '../../core/security/passphrase_cache.dart';

/// Unlock screen for entering passphrase
class UnlockScreen extends ConsumerStatefulWidget {
  const UnlockScreen({super.key, this.redirectTo});

  final String? redirectTo;

  @override
  ConsumerState<UnlockScreen> createState() => _UnlockScreenState();
}

class _UnlockScreenState extends ConsumerState<UnlockScreen> {
  final _passphraseController = TextEditingController();
  final _formKey = GlobalKey<FormState>();
  bool _obscurePassphrase = true;
  bool _isUnlocking = false;
  bool _isBiometricUnlocking = false;
  String? _error;
  bool _biometricAttempted = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _tryAutoBiometricUnlock();
    });
  }

  @override
  void dispose() {
    _passphraseController.dispose();
    super.dispose();
  }

  Future<void> _unlock() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }

    setState(() {
      _isUnlocking = true;
      _error = null;
    });

    try {
      final unlockApp = ref.read(unlockAppProvider);
      await unlockApp(_passphraseController.text);
      final isDecoy = await FfiBridge.isDecoyMode();
      if (isDecoy) {
        try {
          await PassphraseCache.clear();
        } catch (_) {
          // Ignore; best-effort clear.
        }
      } else {
        final biometricsEnabled = await _resolveBiometricsEnabled();
        if (biometricsEnabled) {
          final hasWrapped = await PassphraseCache.exists();
          if (!hasWrapped) {
            try {
              await PassphraseCache.store(_passphraseController.text);
            } catch (_) {
              // Ignore; biometric wrap can be retried later.
            }
          }
        }
      }
      
      await _navigateAfterUnlock();
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = 'Invalid passphrase. Please try again.';
          _isUnlocking = false;
        });
      }
    }
  }

  Future<void> _tryAutoBiometricUnlock() async {
    if (_biometricAttempted) return;

    if (!await _resolveBiometricsEnabled()) {
      return;
    }

    final hasWrapped = await PassphraseCache.exists();
    if (!hasWrapped) {
      return;
    }

    _biometricAttempted = true;
    await _unlockWithBiometrics(silentFailure: true);
  }

  Future<bool> _resolveBiometricsEnabled() async {
    if (ref.read(biometricsEnabledProvider)) {
      return true;
    }
    try {
      return await ref
          .read(biometricsEnabledProvider.notifier)
          .readPersistedValue();
    } catch (_) {
      return false;
    }
  }

  Future<void> _unlockWithBiometrics({
    bool silentFailure = false,
  }) async {
    if (_isBiometricUnlocking) return;
    setState(() {
      _isBiometricUnlocking = true;
      if (!silentFailure) {
        _error = null;
      }
    });

    try {
      if (Platform.isWindows || Platform.isMacOS || Platform.isLinux) {
        final authenticated = await BiometricAuth.authenticate(
          reason: 'Unlock your wallet',
          biometricOnly: true,
        );
        if (!authenticated) {
          return;
        }
      }

      String? cached;
      try {
        cached = await PassphraseCache.read();
      } catch (_) {
        return;
      }

      if (cached == null || cached.isEmpty) {
        return;
      }

      final unlockApp = ref.read(unlockAppProvider);
      await unlockApp(cached);

      await _navigateAfterUnlock();
    } catch (e) {
      if (mounted && !silentFailure) {
        setState(() {
          _error = 'Biometric unlock failed. Use your passphrase instead.';
        });
      }
    } finally {
      if (mounted) {
        setState(() => _isBiometricUnlocking = false);
      }
    }
  }

  Future<void> _navigateAfterUnlock() async {
    final redirect = widget.redirectTo;
    if (redirect != null && redirect.isNotEmpty) {
      if (mounted) {
        context.go(redirect);
      }
      return;
    }

    ref.invalidate(walletsExistProvider);
    final walletsExist = await ref.read(walletsExistProvider.future);
    if (!mounted) return;
    context.go(walletsExist ? '/home' : '/onboarding/welcome');
  }

  @override
  Widget build(BuildContext context) {
    // Listen for biometrics changes
    ref.listen<bool>(biometricsEnabledProvider, (previous, next) {
      if (next && !_biometricAttempted) {
        _tryAutoBiometricUnlock();
      }
    });
    
    return PScaffold(
      title: 'Unlock Wallet',
      appBar: const PAppBar(
        title: 'Unlock Wallet',
        subtitle: 'Enter your passphrase to access your wallets',
        showBackButton: false,
        centerTitle: true,
      ),
      body: LayoutBuilder(
        builder: (context, constraints) {
          final size = MediaQuery.of(context).size;
          final isCompactHeight = constraints.maxHeight < 760;
          final verticalPadding =
              isCompactHeight ? AppSpacing.lg : AppSpacing.xl;
          final basePadding = AppSpacing.screenPadding(
            size.width,
            vertical: verticalPadding,
          );
          final contentPadding = basePadding.copyWith(
            bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
          );
          final iconSize = isCompactHeight ? 52.0 : 64.0;
          final iconSpacing =
              isCompactHeight ? AppSpacing.lg : AppSpacing.xl;
          final titleSpacing =
              isCompactHeight ? AppSpacing.xs : AppSpacing.sm;
          final sectionSpacing =
              isCompactHeight ? AppSpacing.lg : AppSpacing.xl;
          return SingleChildScrollView(
            padding: contentPadding,
            keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: constraints.maxHeight),
              child: IntrinsicHeight(
                child: Form(
                  key: _formKey,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      if (isCompactHeight)
                        SizedBox(height: AppSpacing.lg)
                      else
                        const Spacer(),
                      
                      // Icon
                      Icon(
                        Icons.lock_outline,
                        size: iconSize,
                        color: AppColors.accentPrimary,
                      ),
                      
                      SizedBox(height: iconSpacing),
                      
                      // Title
                      Text(
                        'Enter Passphrase',
                        style: AppTypography.h2.copyWith(
                          color: AppColors.textPrimary,
                        ),
                        textAlign: TextAlign.center,
                      ),
                      
                      SizedBox(height: titleSpacing),
                      
                      // Subtitle
                      Text(
                        'Enter your passphrase to unlock and access your wallets',
                        style: AppTypography.body.copyWith(
                          color: AppColors.textSecondary,
                        ),
                        textAlign: TextAlign.center,
                      ),
                      
                      SizedBox(height: sectionSpacing),

                      // Passphrase input
                      PInput(
                        controller: _passphraseController,
                        label: 'Passphrase',
                        hint: 'Enter your passphrase',
                        obscureText: _obscurePassphrase,
                        textInputAction: TextInputAction.done,
                        onSubmitted: (_) => _unlock(),
                        suffixIcon: IconButton(
                          icon: Icon(
                            _obscurePassphrase
                                ? Icons.visibility_outlined
                                : Icons.visibility_off_outlined,
                            color: AppColors.textSecondary,
                          ),
                          onPressed: () {
                            setState(() {
                              _obscurePassphrase = !_obscurePassphrase;
                            });
                          },
                        ),
                        validator: (value) {
                          if (value == null || value.isEmpty) {
                            return 'Please enter your passphrase';
                          }
                          return null;
                        },
                      ),
                      
                      const SizedBox(height: AppSpacing.md),
                      
                      // Error message
                      if (_error != null)
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
                              Icon(
                                Icons.error_outline,
                                color: AppColors.error,
                                size: 20,
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
                      
                      if (isCompactHeight)
                        SizedBox(height: AppSpacing.lg)
                      else
                        const Spacer(),
                      
                      // Unlock button
                      PButton(
                        text: 'Unlock',
                        onPressed: _isUnlocking ? null : _unlock,
                        variant: PButtonVariant.primary,
                        size: PButtonSize.large,
                        isLoading: _isUnlocking,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
