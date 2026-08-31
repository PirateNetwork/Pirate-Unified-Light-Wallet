import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('macOS uses the stable renderer on Intel hardware', () {
    final plist = File('macos/Runner/Info.plist').readAsStringSync();

    expect(plist, contains(RegExp(r'<key>FLTEnableImpeller</key>\s*<false/>')));
  });
}
