import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

void main() {
  testWidgets('loading indicator is spaced from its label', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: NoSplash.splashFactory),
        home: Scaffold(
          body: Center(
            child: PButton(text: 'Saving...', onPressed: () {}, loading: true),
          ),
        ),
      ),
    );
    await tester.pump(const Duration(milliseconds: 200));

    final indicator = tester.getRect(find.byType(CircularProgressIndicator));
    final label = tester.getRect(find.text('Saving...'));
    expect(label.left - indicator.right, PSpacing.iconTextGap);
  });

  testWidgets('circular icon button keeps a clipped square surface', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: PIconButton(
              icon: const Icon(Icons.arrow_back),
              onPressed: () {},
              shape: PIconButtonShape.circle,
            ),
          ),
        ),
      ),
    );

    final surfaceFinder = find.descendant(
      of: find.byType(PIconButton),
      matching: find.byType(AnimatedContainer),
    );
    final surface = tester.widget<AnimatedContainer>(surfaceFinder);
    final decoration = surface.decoration! as BoxDecoration;

    expect(tester.getSize(surfaceFinder), const Size.square(48));
    expect(surface.clipBehavior, Clip.antiAlias);
    expect(decoration.shape, BoxShape.circle);
    expect(decoration.borderRadius, isNull);
  });
}
