import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';
import 'package:pirate_wallet/features/settings/panic_pin_screen.dart';
import '../test_flags.dart';

final bool _skipFfiTests = shouldSkipFfiTests();

void main() {
  group('Duress Passphrase Screen', () {
    testWidgets('Shows setup view by default', (WidgetTester tester) async {
      await tester.pumpWidget(
        const ProviderScope(child: MaterialApp(home: PanicPinScreen())),
      );

      await tester.pumpAndSettle();

      expect(find.text('Set duress passphrase'), findsOneWidget);
      expect(find.byIcon(Icons.emergency), findsOneWidget);
      expect(find.text('How it works'), findsOneWidget);
      expect(find.byIcon(Icons.circle), findsNWidgets(4));
      expect(find.text('-'), findsNothing);

      final bullet = tester.getRect(find.byIcon(Icons.circle).first);
      final firstItem = tester.getRect(
        find.text('Opens a decoy wallet with empty data.'),
      );
      expect(firstItem.left - bullet.right, PSpacing.sm);
    }, skip: _skipFfiTests);

    testWidgets('Shows custom passphrase fields when toggled', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(
        const ProviderScope(child: MaterialApp(home: PanicPinScreen())),
      );

      await tester.pumpAndSettle();

      final customToggle = find.text('Use a custom duress passphrase');
      await tester.ensureVisible(customToggle);
      await tester.pumpAndSettle();
      await tester.tap(customToggle);
      await tester.pumpAndSettle();

      expect(find.text('Custom duress passphrase'), findsOneWidget);
      expect(find.text('Confirm duress passphrase'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Explains default reverse behavior', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(
        const ProviderScope(child: MaterialApp(home: PanicPinScreen())),
      );

      await tester.pumpAndSettle();

      expect(find.text('Default is your passphrase reversed.'), findsOneWidget);
    }, skip: _skipFfiTests);
  });
}
