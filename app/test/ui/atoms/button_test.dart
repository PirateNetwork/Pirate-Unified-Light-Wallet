import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:golden_toolkit/golden_toolkit.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

void main() {
  group('PButton Golden Tests', () {
    testGoldens('PButton variants', (tester) async {
      await loadAppFonts();

      final builder = GoldenBuilder.column()
        ..addScenario(
          'Primary Button',
          PButton(
            onPressed: () {},
            child: const Text('Primary'),
          ),
        )
        ..addScenario(
          'Secondary Button',
          PButton(
            onPressed: () {},
            variant: PButtonVariant.secondary,
            child: const Text('Secondary'),
          ),
        )
        ..addScenario(
          'Outline Button',
          PButton(
            onPressed: () {},
            variant: PButtonVariant.outline,
            child: const Text('Outline'),
          ),
        )
        ..addScenario(
          'Disabled Button',
          PButton(
            onPressed: null,
            child: const Text('Disabled'),
          ),
        )
        ..addScenario(
          'Loading Button',
          PButton(
            onPressed: () {},
            loading: true,
            child: const Text('Loading'),
          ),
        )
        ..addScenario(
          'Button with Icon',
          PButton(
            onPressed: () {},
            icon: const Icon(Icons.send),
            child: const Text('Send'),
          ),
        );

      await tester.pumpWidgetBuilder(
        builder.build(),
        wrapper: materialAppWrapper(theme: PTheme.dark()),
      );

      await screenMatchesGolden(tester, 'button_variants');
    });

    testGoldens('PButton sizes', (tester) async {
      await loadAppFonts();

      final builder = GoldenBuilder.column()
        ..addScenario(
          'Small',
          PButton(
            onPressed: () {},
            size: PButtonSize.small,
            child: const Text('Small'),
          ),
        )
        ..addScenario(
          'Medium',
          PButton(
            onPressed: () {},
            size: PButtonSize.medium,
            child: const Text('Medium'),
          ),
        )
        ..addScenario(
          'Large',
          PButton(
            onPressed: () {},
            size: PButtonSize.large,
            child: const Text('Large'),
          ),
        );

      await tester.pumpWidgetBuilder(
        builder.build(),
        wrapper: materialAppWrapper(theme: PTheme.dark()),
      );

      await screenMatchesGolden(tester, 'button_sizes');
    });
  });
}

