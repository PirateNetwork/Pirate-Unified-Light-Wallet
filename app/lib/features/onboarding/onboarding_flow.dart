/// Onboarding flow state management
///
/// Manages the multi-step onboarding process with validation
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/providers/wallet_providers.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/services/birthday_update_service.dart';

/// Onboarding steps
enum OnboardingStep {
  welcome,
  createOrImport,
  setupPassphrase,
  biometrics,
  backupWarning,
  seedDisplay,
  seedConfirm,
  birthdayPicker,
  complete,
}

/// Onboarding mode
enum OnboardingMode {
  create,
  import,
  watchOnly, // viewing key import
}

/// Onboarding state
class OnboardingState {
  final OnboardingStep currentStep;
  final OnboardingMode? mode;
  final String? mnemonic;
  final MnemonicLanguage? mnemonicLanguage;
  final String? passphrase;
  final bool biometricsEnabled;
  final int? birthdayHeight;
  final bool seedBackedUp;

  const OnboardingState({
    this.currentStep = OnboardingStep.welcome,
    this.mode,
    this.mnemonic,
    this.mnemonicLanguage,
    this.passphrase,
    this.biometricsEnabled = false,
    this.birthdayHeight,
    this.seedBackedUp = false,
  });

  OnboardingState copyWith({
    OnboardingStep? currentStep,
    OnboardingMode? mode,
    String? mnemonic,
    MnemonicLanguage? mnemonicLanguage,
    String? passphrase,
    bool? biometricsEnabled,
    int? birthdayHeight,
    bool? seedBackedUp,
  }) {
    return OnboardingState(
      currentStep: currentStep ?? this.currentStep,
      mode: mode ?? this.mode,
      mnemonic: mnemonic ?? this.mnemonic,
      mnemonicLanguage: mnemonicLanguage ?? this.mnemonicLanguage,
      passphrase: passphrase ?? this.passphrase,
      biometricsEnabled: biometricsEnabled ?? this.biometricsEnabled,
      birthdayHeight: birthdayHeight ?? this.birthdayHeight,
      seedBackedUp: seedBackedUp ?? this.seedBackedUp,
    );
  }

  /// Check if can proceed to next step
  bool canProceed() {
    switch (currentStep) {
      case OnboardingStep.welcome:
        return true;
      case OnboardingStep.createOrImport:
        return mode != null;
      case OnboardingStep.setupPassphrase:
        return passphrase != null && passphrase!.isNotEmpty;
      case OnboardingStep.biometrics:
        return true; // Biometrics is optional
      case OnboardingStep.backupWarning:
        return true;
      case OnboardingStep.seedDisplay:
        return true;
      case OnboardingStep.seedConfirm:
        return seedBackedUp;
      case OnboardingStep.birthdayPicker:
        return birthdayHeight != null;
      case OnboardingStep.complete:
        return false; // Final step
    }
  }

  /// Get next step based on current state
  OnboardingStep? getNextStep() {
    switch (currentStep) {
      case OnboardingStep.welcome:
        return OnboardingStep.createOrImport;
      case OnboardingStep.createOrImport:
        return OnboardingStep.setupPassphrase;
      case OnboardingStep.setupPassphrase:
        return OnboardingStep.biometrics;
      case OnboardingStep.biometrics:
        if (mode == OnboardingMode.create) {
          return OnboardingStep.backupWarning;
        } else {
          return OnboardingStep.birthdayPicker;
        }
      case OnboardingStep.backupWarning:
        return OnboardingStep.seedDisplay;
      case OnboardingStep.seedDisplay:
        return OnboardingStep.seedConfirm;
      case OnboardingStep.seedConfirm:
        // For new wallets, skip birthday picker and auto-use latest block height
        // For import/restore, show birthday picker
        if (mode == OnboardingMode.create) {
          return OnboardingStep.complete;
        } else {
          return OnboardingStep.birthdayPicker;
        }
      case OnboardingStep.birthdayPicker:
        return OnboardingStep.complete;
      case OnboardingStep.complete:
        return null;
    }
  }
}

/// Onboarding flow controller
class OnboardingController extends Notifier<OnboardingState> {
  WalletId? _provisionedWalletId;
  int? _pendingBirthdayHeight;
  bool _needsBirthdayRefresh = false;

  @override
  OnboardingState build() {
    return const OnboardingState();
  }

  void setMode(OnboardingMode mode) {
    state = state.copyWith(mode: mode);
  }

  void setMnemonic(String mnemonic, {MnemonicLanguage? mnemonicLanguage}) {
    state = state.copyWith(
      mnemonic: mnemonic,
      mnemonicLanguage: mnemonicLanguage ?? state.mnemonicLanguage,
    );
  }

  void setPassphrase(String passphrase) {
    state = state.copyWith(passphrase: passphrase);
  }

  void setBiometrics({required bool enabled}) {
    state = state.copyWith(biometricsEnabled: enabled);
  }

  void setBirthdayHeight(int height) {
    state = state.copyWith(birthdayHeight: height);
  }

  void markSeedBackedUp() {
    state = state.copyWith(seedBackedUp: true);
  }

  void nextStep() {
    final next = state.getNextStep();
    if (next != null) {
      state = state.copyWith(currentStep: next);
    }
  }

  void previousStep() {
    // Navigate backwards (simplified for now)
    const steps = OnboardingStep.values;
    final currentIndex = steps.indexOf(state.currentStep);
    if (currentIndex > 0) {
      state = state.copyWith(currentStep: steps[currentIndex - 1]);
    }
  }

  void reset({OnboardingStep startAt = OnboardingStep.createOrImport}) {
    _clearProvisioningCheckpoint();
    state = OnboardingState(currentStep: startAt);
  }

  /// Complete onboarding and create/import wallet
  Future<void> complete(String walletName) async {
    final mode = state.mode;
    if (mode == null) {
      throw StateError('Onboarding mode not selected');
    }

    var walletId = _provisionedWalletId;
    if (walletId == null) {
      walletId = await _provisionWallet(mode, walletName);
      // Keep the returned ID until setup is fully finalized. If activation or
      // provider refresh fails, retrying resumes here instead of creating a
      // duplicate wallet from the same recovery phrase.
      _provisionedWalletId = walletId;
    }

    await ref.read(finalizeWalletProvisioningProvider)(walletId);

    final pendingBirthday = _pendingBirthdayHeight;
    if (_needsBirthdayRefresh && pendingBirthday != null) {
      await BirthdayUpdateService.markPending(walletId, pendingBirthday);
      unawaited(
        BirthdayUpdateService.updateWhenAvailable(
          walletId,
          pendingBirthday,
          onWalletsUpdated: ref.read(refreshWalletsProvider),
        ),
      );
      _needsBirthdayRefresh = false;
      _pendingBirthdayHeight = null;
    }

    // After wallet creation, unlock the app with the passphrase
    // The passphrase was set during onboarding, so we just need to mark as unlocked
    if (state.passphrase != null && state.passphrase!.isNotEmpty) {
      try {
        await FfiBridge.unlockApp(state.passphrase!);
        ref.read(appUnlockedProvider.notifier).unlocked = true;
      } catch (e) {
        // If unlock fails, it's okay - user will need to unlock on next launch
        debugPrint('Failed to unlock app after wallet creation: $e');
      }
    }

    state = state.copyWith(currentStep: OnboardingStep.complete);
    _clearProvisioningCheckpoint();
  }

  Future<WalletId> _provisionWallet(
    OnboardingMode mode,
    String walletName,
  ) async {
    final api = ref.read(walletProvisioningApiProvider);
    switch (mode) {
      case OnboardingMode.create:
        // New wallets start near the current tip. The generated mnemonic is
        // restored deliberately so the exact words shown to the user are used.
        int? birthday = state.birthdayHeight;
        _BirthdayResolution? resolution;
        if (birthday == null) {
          resolution = await _resolveBirthdayHeight();
          birthday = resolution.height;
          state = state.copyWith(birthdayHeight: birthday);
        }

        final mnemonic = state.mnemonic;
        final walletId = mnemonic != null && mnemonic.isNotEmpty
            ? await api.restoreWallet(
                name: walletName,
                mnemonic: mnemonic,
                birthday: birthday,
                mnemonicLanguage: state.mnemonicLanguage,
              )
            : await api.createWallet(
                name: walletName,
                entropyLen: 256,
                birthday: birthday,
                mnemonicLanguage: state.mnemonicLanguage,
              );
        if (resolution?.timedOut ?? false) {
          _needsBirthdayRefresh = true;
          _pendingBirthdayHeight = birthday;
        }
        return walletId;
      case OnboardingMode.import:
        final mnemonic = state.mnemonic;
        if (mnemonic == null || mnemonic.isEmpty) {
          throw StateError('Mnemonic not provided for restore');
        }
        return api.restoreWallet(
          name: walletName,
          mnemonic: mnemonic,
          birthday: state.birthdayHeight,
          mnemonicLanguage: state.mnemonicLanguage,
        );
      case OnboardingMode.watchOnly:
        throw StateError(
          'Watch-only onboarding must use viewing key import flow',
        );
    }
  }

  void _clearProvisioningCheckpoint() {
    _provisionedWalletId = null;
    _pendingBirthdayHeight = null;
    _needsBirthdayRefresh = false;
  }

  Future<_BirthdayResolution> _resolveBirthdayHeight() async {
    const maxWait = Duration(seconds: 25);
    const fetchTimeout = Duration(seconds: 8);
    final fallbackHeight = await _resolveBirthdayFallbackHeight();
    var attempt = 0;
    final start = DateTime.now();

    while (DateTime.now().difference(start) < maxWait) {
      int? height;
      try {
        height = await BirthdayUpdateService.fetchLatestBirthdayHeight()
            .timeout(fetchTimeout);
      } catch (_) {
        height = null;
      }
      if (height != null) {
        return _BirthdayResolution(height: height, timedOut: false);
      }
      attempt += 1;
      final elapsed = DateTime.now().difference(start);
      if (elapsed >= maxWait) {
        break;
      }
      final delaySeconds = attempt < 3 ? 2 : (attempt < 6 ? 4 : 6);
      await Future<void>.delayed(Duration(seconds: delaySeconds));
    }

    return _BirthdayResolution(height: fallbackHeight, timedOut: true);
  }

  Future<int> _resolveBirthdayFallbackHeight() async {
    try {
      final fallback = await FfiBridge.getDefaultBirthdayHeight().timeout(
        const Duration(seconds: 3),
      );
      return fallback > 0 ? fallback : 1;
    } catch (_) {
      return 1;
    }
  }
}

class _BirthdayResolution {
  final int height;
  final bool timedOut;

  const _BirthdayResolution({required this.height, required this.timedOut});
}

/// Provider for onboarding controller
final onboardingControllerProvider =
    NotifierProvider<OnboardingController, OnboardingState>(
      OnboardingController.new,
    );
