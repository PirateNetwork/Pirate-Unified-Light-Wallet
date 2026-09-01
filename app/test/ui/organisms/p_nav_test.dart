import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/tokens/colors.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';
import 'package:pirate_wallet/design/tokens/typography.dart';
import 'package:pirate_wallet/ui/organisms/p_nav.dart';

void main() {
  testWidgets('desktop rail selects the complete navigation item', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);
    var selectedIndex = -1;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PNav(
            currentIndex: 1,
            onDestinationSelected: (index) => selectedIndex = index,
            destinations: const [
              PNavDestination(icon: Icons.home_outlined, label: 'Home'),
              PNavDestination(
                icon: Icons.payments_outlined,
                selectedIcon: Icons.payments,
                label: 'Wallets',
              ),
              PNavDestination(icon: Icons.settings_outlined, label: 'Settings'),
            ],
          ),
        ),
      ),
    );

    final railFinder = find.byKey(const ValueKey('desktop-navigation-rail'));
    final selectedItemFinder = find.byKey(const ValueKey('desktop-nav-item-1'));
    final selectedSurfaceFinder = find.descendant(
      of: selectedItemFinder,
      matching: find.byType(AnimatedContainer),
    );
    final selectedSurface = tester.widget<AnimatedContainer>(
      selectedSurfaceFinder,
    );
    final decoration = selectedSurface.decoration! as BoxDecoration;
    final railRect = tester.getRect(railFinder);
    final selectedRect = tester.getRect(selectedSurfaceFinder);

    expect(railRect.width, PSpacing.desktopNavRailWidth);
    expect(selectedRect.height, 72);
    expect(selectedRect.left - railRect.left, PSpacing.sm);
    expect(railRect.right - selectedRect.right, PSpacing.sm);
    expect(selectedSurface.clipBehavior, Clip.antiAlias);
    expect(decoration.color, AppColors.selectedBackground);
    expect(decoration.borderRadius, BorderRadius.circular(PSpacing.radiusSM));

    final selectedLabel = tester.widget<Text>(
      find.descendant(of: selectedItemFinder, matching: find.text('Wallets')),
    );
    expect(selectedLabel.style!.fontSize, 12);
    expect(selectedLabel.style!.fontWeight, PTypography.regular);
    expect(find.byIcon(Icons.payments), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('desktop-nav-item-2')));
    expect(selectedIndex, 2);
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('desktop destinations expand for large text', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        home: MediaQuery(
          data: const MediaQueryData(textScaler: TextScaler.linear(2)),
          child: Scaffold(
            body: PNav(
              currentIndex: 0,
              onDestinationSelected: (_) {},
              destinations: const [
                PNavDestination(icon: Icons.home_outlined, label: 'Home'),
              ],
            ),
          ),
        ),
      ),
    );

    final selectedSurfaceFinder = find.descendant(
      of: find.byKey(const ValueKey('desktop-nav-item-0')),
      matching: find.byType(AnimatedContainer),
    );

    expect(tester.getSize(selectedSurfaceFinder).height, greaterThan(72));
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('desktop rail compacts on a scaled laptop viewport', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1097, 706);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PNav(
            currentIndex: 0,
            onDestinationSelected: (_) {},
            destinations: const [
              PNavDestination(icon: Icons.home_outlined, label: 'Home'),
              PNavDestination(icon: Icons.payments_outlined, label: 'Wallets'),
            ],
          ),
        ),
      ),
    );

    expect(
      tester
          .getSize(find.byKey(const ValueKey('desktop-navigation-rail')))
          .width,
      PSpacing.desktopCompactNavRailWidth,
    );
    expect(
      tester.getSize(find.byKey(const ValueKey('desktop-nav-item-0'))).height,
      60,
    );
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('keeps complete mobile labels in compact landscape', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(844, 390);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: const SizedBox.expand(),
          bottomNavigationBar: PNav(
            currentIndex: 0,
            onDestinationSelected: (_) {},
            onPayTap: () {},
            payIndex: 1,
            destinations: const [
              PNavDestination(icon: Icons.home_outlined, label: 'Home'),
              PNavDestination(
                icon: Icons.payments_outlined,
                label: 'Wallets',
                isPay: true,
              ),
              PNavDestination(
                icon: Icons.receipt_long_outlined,
                label: 'Recent activity',
              ),
              PNavDestination(icon: Icons.settings_outlined, label: 'Settings'),
            ],
          ),
        ),
      ),
    );
    await tester.pump();

    final navRect = tester.getRect(find.byKey(PNav.mobileNavigationKey));
    final recentLabel = tester.widget<Text>(find.text('Recent activity'));

    expect(navRect.height, lessThanOrEqualTo(64));
    expect(recentLabel.maxLines, 2);
    expect(recentLabel.overflow, isNull);
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });
}
