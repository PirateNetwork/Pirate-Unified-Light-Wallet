import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/atoms/p_input.dart';

void main() {
  testWidgets('visible label is attached to text field semantics', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();

    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: PInput(label: 'Enter your passphrase', obscureText: true),
        ),
      ),
    );

    expect(find.bySemanticsLabel('Enter your passphrase'), findsOneWidget);
    semantics.dispose();
  });
}
