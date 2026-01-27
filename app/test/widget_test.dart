// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

void main() {
  testWidgets('App theme + button smoke test', (WidgetTester tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: PTheme.dark(),
        home: Scaffold(
          body: Center(
            child: PButton(
              onPressed: () {},
              child: const Text('Continue'),
            ),
          ),
        ),
      ),
    );

    expect(find.text('Continue'), findsOneWidget);
  });
}
