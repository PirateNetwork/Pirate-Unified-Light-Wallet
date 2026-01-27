// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

void main() {
  testWidgets('App button smoke test', (WidgetTester tester) async {
    // Disable animations for testing
    // Animate.restartOnHotReload = true;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: PButton(
              onPressed: () {},
              text: 'Continue',
            ),
          ),
        ),
      ),
    );

    expect(find.text('Continue'), findsOneWidget);
  });
}
