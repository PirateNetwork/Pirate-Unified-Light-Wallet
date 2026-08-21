import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('macOS build entitlements permit secure preference storage', () {
    const entitlementFiles = [
      'macos/Runner/DebugProfile.entitlements',
      'macos/Runner/Release.entitlements',
      'macos/Runner/Distribution.entitlements',
    ];

    for (final path in entitlementFiles) {
      final contents = File(path).readAsStringSync();
      expect(
        contents,
        contains('<key>keychain-access-groups</key>'),
        reason: '$path must enable Keychain Sharing for flutter_secure_storage',
      );
    }
  });
}
