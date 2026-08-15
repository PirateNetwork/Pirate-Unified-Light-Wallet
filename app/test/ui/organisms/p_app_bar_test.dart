import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/atoms/p_icon_button.dart';
import 'package:pirate_wallet/ui/atoms/theme_toggle_button.dart';
import 'package:pirate_wallet/ui/organisms/p_app_bar.dart';
import 'package:pirate_wallet/ui/organisms/p_scaffold.dart';

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
    expect(backRect.left, lessThan(80));
  });

  testWidgets('keeps the back control at the leading edge on desktop', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: Scaffold(
            appBar: PAppBar(
              title: 'Send',
              showBackButton: true,
              actions: [Icon(Icons.add_circle_outline)],
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final backButtonFinder = find.byWidgetPredicate(
      (widget) => widget is PIconButton && widget.tooltip == 'Back',
    );
    final backRect = tester.getRect(
      find.descendant(
        of: backButtonFinder,
        matching: find.byType(AnimatedContainer),
      ),
    );
    final titleRect = tester.getRect(find.text('Send'));

    expect(backRect.left, lessThan(80));
    expect(backRect.right, lessThan(titleRect.left));
  });

  testWidgets('uses a circular surface for the app-bar theme action', (
    tester,
  ) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: Scaffold(
            appBar: PAppBar(title: 'Send', showBackButton: false),
          ),
        ),
      ),
    );

    final themeButton = tester.widget<PIconButton>(
      find.descendant(
        of: find.byType(ThemeToggleButton),
        matching: find.byType(PIconButton),
      ),
    );
    expect(themeButton.shape, PIconButtonShape.circle);
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

  testWidgets('compacts short landscape viewports without crowding titles', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(844, 390);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: PScaffold(
            appBar: PAppBar(
              title: 'Node Configuration',
              subtitle: 'Choose your lightwalletd endpoint',
              showBackButton: true,
              showThemeToggle: false,
              actions: [
                IconButton(onPressed: null, icon: Icon(Icons.wifi)),
                IconButton(onPressed: null, icon: Icon(Icons.refresh)),
              ],
            ),
            body: SizedBox.expand(),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(tester.getSize(find.byType(PAppBar)).height, 64);
    expect(find.text('Choose your lightwalletd endpoint'), findsNothing);
    expect(find.text('Node Configuration'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
