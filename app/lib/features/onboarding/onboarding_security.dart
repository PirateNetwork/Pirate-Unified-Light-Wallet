import '../../core/ffi/ffi_bridge.dart';

enum WalletSetupSecurityRequirement { ready, createPassphrase, unlock }

WalletSetupSecurityRequirement resolveWalletSetupSecurity({
  required bool hasAppPassphrase,
  required bool appUnlocked,
  bool passphraseEstablishedInFlow = false,
}) {
  if (appUnlocked || passphraseEstablishedInFlow) {
    return WalletSetupSecurityRequirement.ready;
  }
  if (!hasAppPassphrase) {
    return WalletSetupSecurityRequirement.createPassphrase;
  }
  return WalletSetupSecurityRequirement.unlock;
}

class OnboardingSecurityServices {
  const OnboardingSecurityServices();

  Future<bool> hasAppPassphrase() => FfiBridge.hasAppPassphrase();

  Future<void> unlockApp(String passphrase) => FfiBridge.unlockApp(passphrase);
}
