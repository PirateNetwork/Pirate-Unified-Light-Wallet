/// Onboarding flow state management
///
/// Manages the multi-step onboarding process with validation
library;

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../config/endpoints.dart' as endpoints;

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
  final String? passphrase;
  final bool biometricsEnabled;
  final int? birthdayHeight;
  final bool seedBackedUp;

  const OnboardingState({
    this.currentStep = OnboardingStep.welcome,
    this.mode,
    this.mnemonic,
    this.passphrase,
    this.biometricsEnabled = false,
    this.birthdayHeight,
    this.seedBackedUp = false,
  });

  OnboardingState copyWith({
    OnboardingStep? currentStep,
    OnboardingMode? mode,
    String? mnemonic,
    String? passphrase,
    bool? biometricsEnabled,
    int? birthdayHeight,
    bool? seedBackedUp,
  }) {
    return OnboardingState(
      currentStep: currentStep ?? this.currentStep,
      mode: mode ?? this.mode,
      mnemonic: mnemonic ?? this.mnemonic,
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
  @override
  OnboardingState build() {
    return const OnboardingState();
  }

  void setMode(OnboardingMode mode) {
    state = state.copyWith(mode: mode);
  }

  void setMnemonic(String mnemonic) {
    state = state.copyWith(mnemonic: mnemonic);
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
    state = OnboardingState(currentStep: startAt);
  }

  /// Complete onboarding and create/import wallet
  Future<void> complete(String walletName) async {
    final mode = state.mode;
    if (mode == null) {
      throw StateError('Onboarding mode not selected');
    }

    switch (mode) {
      case OnboardingMode.create:
        // For new wallets, auto-fetch latest block height if not set
        int? birthday = state.birthdayHeight;
        if (birthday == null) {
          // Fetch latest block height from network
          try {
            final result = await FfiBridge.testNode(url: endpoints.kDefaultLightd);
            if (result.success && result.latestBlockHeight != null) {
              birthday = result.latestBlockHeight;
              state = state.copyWith(birthdayHeight: birthday);
            } else {
              // Fallback to default if fetch fails
              birthday = FfiBridge.defaultBirthdayHeight;
            }
          } catch (_) {
            // Fallback to default if fetch fails
            birthday = FfiBridge.defaultBirthdayHeight;
          }
        }
        
        // If we have a mnemonic in state (from seed display), use restore_wallet
        // to create wallet with that specific mnemonic. Otherwise, use create_wallet
        // which generates a new mnemonic.
        if (state.mnemonic != null && state.mnemonic!.isNotEmpty) {
          await ref.read(restoreWalletProvider)(
            name: walletName,
            mnemonic: state.mnemonic!,
            passphrase: null, // BIP-39 passphrase, not app passphrase
            birthday: birthday,
          );
        } else {
          await ref.read(createWalletProvider)(
            name: walletName,
            birthday: birthday,
          );
        }
        break;
      case OnboardingMode.import:
        final mnemonic = state.mnemonic;
        if (mnemonic == null || mnemonic.isEmpty) {
          throw StateError('Mnemonic not provided for restore');
        }
        await ref.read(restoreWalletProvider)(
          name: walletName,
          mnemonic: mnemonic,
          passphrase: state.passphrase,
          birthday: state.birthdayHeight,
        );
        break;
      case OnboardingMode.watchOnly:
        throw StateError('Watch-only onboarding must use viewing key import flow');
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
  }
}

/// Provider for onboarding controller
final onboardingControllerProvider =
    NotifierProvider<OnboardingController, OnboardingState>(OnboardingController.new);
