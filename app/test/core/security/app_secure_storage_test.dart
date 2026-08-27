import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/app_secure_storage.dart';

void main() {
  test('portable macOS builds use the standard Keychain', () {
    expect(appMacOsSecureStorageOptions.usesDataProtectionKeychain, isFalse);
  });
}
