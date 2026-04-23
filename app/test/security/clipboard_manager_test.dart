import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/clipboard_manager.dart';

void main() {
  group('ClipboardManager lifecycle policy', () {
    test('does not clear on resumed state', () {
      expect(
        ClipboardManager.shouldClearOnLifecycleState(AppLifecycleState.resumed),
        isFalse,
      );
    });

    test('can keep transient inactive state in the foreground', () {
      expect(
        ClipboardManager.shouldClearOnLifecycleState(
          AppLifecycleState.inactive,
          inactiveIsBackground: false,
        ),
        isFalse,
      );
    });

    test('clears on inactive when the platform treats it as background', () {
      expect(
        ClipboardManager.shouldClearOnLifecycleState(
          AppLifecycleState.inactive,
        ),
        isTrue,
      );
    });

    test('clears on real background states', () {
      const states = <AppLifecycleState>[
        AppLifecycleState.paused,
        AppLifecycleState.hidden,
        AppLifecycleState.detached,
      ];

      for (final state in states) {
        expect(
          ClipboardManager.shouldClearOnLifecycleState(
            state,
            inactiveIsBackground: false,
          ),
          isTrue,
        );
      }
    });
  });
}
