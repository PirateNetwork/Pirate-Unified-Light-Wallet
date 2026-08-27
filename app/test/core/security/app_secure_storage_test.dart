import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/app_secure_storage.dart';

void main() {
  test('portable macOS builds use the standard Keychain', () {
    expect(appMacOsSecureStorageOptions.usesDataProtectionKeychain, isFalse);
  });

  test('application secure storage construction stays centralized', () {
    final directConstructors = Directory('lib')
        .listSync(recursive: true)
        .whereType<File>()
        .where((file) => file.path.endsWith('.dart'))
        .where(
          (file) => !file.path
              .replaceAll(r'\', '/')
              .endsWith('core/security/app_secure_storage.dart'),
        )
        .where(
          (file) =>
              RegExp(r'FlutterSecureStorage\s*\(')
                  .hasMatch(file.readAsStringSync()),
        )
        .map((file) => file.path)
        .toList();

    expect(
      directConstructors,
      isEmpty,
      reason: 'Use appSecureStorage so every platform follows one policy.',
    );
  });
}
