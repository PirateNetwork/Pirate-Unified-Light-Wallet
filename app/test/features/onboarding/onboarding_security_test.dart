import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/onboarding/onboarding_security.dart';

void main() {
  group('resolveWalletSetupSecurity', () {
    test('requires a passphrase before first wallet provisioning', () {
      expect(
        resolveWalletSetupSecurity(hasAppPassphrase: false, appUnlocked: false),
        WalletSetupSecurityRequirement.createPassphrase,
      );
    });

    test('requires unlock when encrypted storage already exists', () {
      expect(
        resolveWalletSetupSecurity(hasAppPassphrase: true, appUnlocked: false),
        WalletSetupSecurityRequirement.unlock,
      );
    });

    test('accepts an unlocked or newly established security session', () {
      expect(
        resolveWalletSetupSecurity(hasAppPassphrase: true, appUnlocked: true),
        WalletSetupSecurityRequirement.ready,
      );
      expect(
        resolveWalletSetupSecurity(
          hasAppPassphrase: true,
          appUnlocked: false,
          passphraseEstablishedInFlow: true,
        ),
        WalletSetupSecurityRequirement.ready,
      );
    });
  });
}
