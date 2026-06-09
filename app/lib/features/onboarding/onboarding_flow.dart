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

/// Pirate network types
enum PirateNetwork {
  mainnet,
  testnet,
  regtest,
}

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
  final PirateNetwork network;
  final String? customEndpoint;

  const OnboardingState({
    this.currentStep = OnboardingStep.welcome,
    this.mode,
    this.mnemonic,
    this.mnemonicLanguage,
    this.passphrase,
    this.biometricsEnabled = false,
    this.birthdayHeight,
    this.seedBackedUp = false,
    this.network = PirateNetwork.mainnet,
    this.customEndpoint,
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
    PirateNetwork? network,
    String? customEndpoint,
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
      network: network ?? this.network,
      customEndpoint: customEndpoint ?? this.customEndpoint,
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

  void setNetwork(PirateNetwork network) {
    state = state.copyWith(network: network);
  }

  void setCustomEndpoint(String? endpoint) {
    state = state.copyWith(customEndpoint: endpoint);
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

    String finalWalletName = walletName;
    if (state.network == PirateNetwork.regtest) {
      finalWalletName = '$walletName [REGTEST]';
    } else if (state.network == PirateNetwork.testnet) {
      finalWalletName = '$walletName [TESTNET]';
    }

    switch (mode) {
      case OnboardingMode.create:
        // For new wallets, wait for a lightwalletd tip and set birthday to tip-10
        int? birthday = state.birthdayHeight;
        _BirthdayResolution? resolution;
        if (birthday == null) {
          resolution = await _resolveBirthdayHeight();
          birthday = resolution.height;
          state = state.copyWith(birthdayHeight: birthday);
        }

        // If we have a mnemonic in state (from seed display), use restore_wallet
        // to create wallet with that specific mnemonic. Otherwise, use create_wallet
        // which generates a new mnemonic.
        final WalletId walletId;
        if (state.mnemonic != null && state.mnemonic!.isNotEmpty) {
          walletId = await ref.read(restoreWalletProvider)(
            name: finalWalletName,
            mnemonic: state.mnemonic!,
            birthday: birthday,
            mnemonicLanguage: state.mnemonicLanguage,
            networkType: state.network.name,
            endpoint: state.customEndpoint,
          );
        } else {
          walletId = await ref.read(createWalletProvider)(
            name: finalWalletName,
            birthday: birthday,
            mnemonicLanguage: state.mnemonicLanguage,
            networkType: state.network.name,
            endpoint: state.customEndpoint,
          );
        }
        if (resolution?.timedOut ?? false) {
          await BirthdayUpdateService.markPending(walletId, birthday);
          unawaited(
            BirthdayUpdateService.updateWhenAvailable(
              walletId,
              birthday,
              onWalletsUpdated: ref.read(refreshWalletsProvider),
            ),
          );
        }
        break;
      case OnboardingMode.import:
        final mnemonic = state.mnemonic;
        if (mnemonic == null || mnemonic.isEmpty) {
          throw StateError('Mnemonic not provided for restore');
        }
        await ref.read(restoreWalletProvider)(
          name: finalWalletName,
          mnemonic: mnemonic,
          birthday: state.birthdayHeight,
          mnemonicLanguage: state.mnemonicLanguage,
          networkType: state.network.name,
          endpoint: state.customEndpoint,
        );
        break;
      case OnboardingMode.watchOnly:
        throw StateError(
          'Watch-only onboarding must use viewing key import flow',
        );
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

  Future<_BirthdayResolution> _resolveBirthdayHeight() async {
    const maxWait = Duration(seconds: 25);
    const fetchTimeout = Duration(seconds: 8);
    final fallbackHeight = await _resolveBirthdayFallbackHeight();
    var attempt = 0;
    final start = DateTime.now();

    final networkType = state.network.name;

    while (DateTime.now().difference(start) < maxWait) {
      int? height;
      try {
        height = await BirthdayUpdateService.fetchLatestBirthdayHeight(
          networkType: networkType,
        ).timeout(fetchTimeout);
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
    if (state.network == PirateNetwork.regtest) {
      return 1; // Default for Regtest
    }
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
