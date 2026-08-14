import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/settings/screens/node_settings_screen.dart';
import 'package:pirate_wallet/ui/atoms/p_input.dart';

void main() {
  testWidgets('keeps the TLS pin control compact and vertically aligned', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async =>
                const LightdEndpointConfig(url: 'https://lightd.example:443'),
          ),
        ],
        child: const MaterialApp(home: NodeSettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    final pinFinder = find.byKey(const ValueKey('tls-pin-input'));
    await tester.scrollUntilVisible(
      pinFinder,
      240,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    final pinInput = tester.widget<PInput>(pinFinder);
    expect(pinInput.maxLines, 1);
    expect(pinInput.helperText, 'Leave empty to skip certificate pinning');
    expect(pinInput.hint, isNull);
    expect(pinInput.monospace, isTrue);
    expect(pinInput.autocorrect, isFalse);
    expect(pinInput.enableSuggestions, isFalse);
    expect(pinInput.keyboardType, TextInputType.visiblePassword);
    expect(pinInput.textInputAction, TextInputAction.done);

    final textFieldFinder = find.descendant(
      of: pinFinder,
      matching: find.byType(TextField),
    );
    final textField = tester.widget<TextField>(textFieldFinder);
    expect(textField.textAlignVertical, TextAlignVertical.center);
    expect(tester.getSize(textFieldFinder).height, lessThanOrEqualTo(64));

    final labelFinder = find.descendant(
      of: pinFinder,
      matching: find.text('SPKI Pin (base64)'),
    );
    final helperFinder = find.descendant(
      of: pinFinder,
      matching: find.text('Leave empty to skip certificate pinning'),
    );
    final leftEdge = tester.getTopLeft(textFieldFinder).dx;
    expect(tester.getTopLeft(labelFinder).dx, leftEdge);
    expect(tester.getTopLeft(helperFinder).dx, leftEdge);
  });
}
