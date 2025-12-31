import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:golden_toolkit/golden_toolkit.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/ui/molecules/p_card.dart';

void main() {
  group('PCard Golden Tests', () {
    testGoldens('PCard variants', (tester) async {
      await loadAppFonts();

      final builder = GoldenBuilder.column()
        ..addScenario(
          'Basic Card',
          PCard(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('Card Title', style: TextStyle(fontSize: 18)),
                const SizedBox(height: 8),
                const Text('Card content goes here'),
              ],
            ),
          ),
        )
        ..addScenario(
          'Elevated Card',
          PCard(
            elevated: true,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('Elevated Card', style: TextStyle(fontSize: 18)),
                const SizedBox(height: 8),
                const Text('This card has elevated background'),
              ],
            ),
          ),
        )
        ..addScenario(
          'Clickable Card',
          PCard(
            onTap: () {},
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('Clickable Card', style: TextStyle(fontSize: 18)),
                const SizedBox(height: 8),
                const Text('This card is interactive'),
              ],
            ),
          ),
        );

      await tester.pumpWidgetBuilder(
        builder.build(),
        wrapper: materialAppWrapper(theme: PTheme.dark()),
      );

      await screenMatchesGolden(tester, 'card_variants');
    });
  });
}

