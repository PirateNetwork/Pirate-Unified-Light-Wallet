import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/atoms/p_icon_button.dart';
import 'package:pirate_wallet/ui/atoms/theme_toggle_button.dart';
import 'package:pirate_wallet/ui/organisms/p_app_bar.dart';

void main() {
  testWidgets('uses a circular back control that fits a narrow app bar', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: Scaffold(
            appBar: PAppBar(
              title: 'Keys and addresses with a long wallet label',
              subtitle: 'Manage imported keys and addresses',
              showBackButton: true,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final backButtonFinder = find.byWidgetPredicate(
      (widget) => widget is PIconButton && widget.tooltip == 'Back',
    );
    final backButton = tester.widget<PIconButton>(backButtonFinder);
    expect(backButton.size, PIconButtonSize.medium);
    expect(backButton.shape, PIconButtonShape.circle);

    final surfaceFinder = find.descendant(
      of: backButtonFinder,
      matching: find.byType(AnimatedContainer),
    );
    final backRect = tester.getRect(surfaceFinder);
    final appBarRect = tester.getRect(find.byType(PAppBar));
    expect(backRect.size, const Size.square(48));
    expect(backRect.top, greaterThanOrEqualTo(appBarRect.top));
    expect(backRect.bottom, lessThanOrEqualTo(appBarRect.bottom));
  });

  testWidgets('can defer the theme control to a persistent shell', (
    tester,
  ) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: Scaffold(
            appBar: PAppBar(
              title: 'Pay',
              showBackButton: false,
              showThemeToggle: false,
            ),
          ),
        ),
      ),
    );

    expect(find.byType(ThemeToggleButton), findsNothing);
  });
}
