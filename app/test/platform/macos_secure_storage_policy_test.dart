import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('macOS builds do not require provisioned Keychain access groups', () {
    const entitlementFiles = [
      'macos/Runner/DebugProfile.entitlements',
      'macos/Runner/Release.entitlements',
      'macos/Runner/Distribution.entitlements',
    ];

    for (final path in entitlementFiles) {
      final contents = File(path).readAsStringSync();
      expect(
        contents,
        isNot(contains('<key>keychain-access-groups</key>')),
        reason: '$path must remain compatible with portable signing',
      );
    }
  });
}
