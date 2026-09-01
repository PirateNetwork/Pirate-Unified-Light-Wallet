import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/swaps/swap_availability.dart';
import 'package:pirate_wallet/design/tokens/colors.dart';
import 'package:pirate_wallet/features/pay/pay_screen.dart';

void main() {
  testWidgets('disables the swap action while swaps are unreleased', (
    tester,
  ) async {
    var sendTaps = 0;
    var swapTaps = 0;

    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: InkRipple.splashFactory),
        home: Scaffold(
          body: PaySheet(
            onSend: () => sendTaps++,
            onReceive: () {},
            onVerify: () {},
            onSwap: () => swapTaps++,
          ),
        ),
      ),
    );

    expect(find.text('Wallets'), findsOneWidget);

    expect(kAtomicSwapsEnabled, isFalse);

    final sendAction = find.ancestor(
      of: find.text('Send'),
      matching: find.byType(InkWell),
    );
    final swapAction = find.ancestor(
      of: find.text('Swap'),
      matching: find.byType(InkWell),
    );

    expect(sendAction, findsOneWidget);
    expect(swapAction, findsOneWidget);
    expect(tester.widget<InkWell>(sendAction).onTap, isNotNull);
    expect(tester.widget<InkWell>(swapAction).onTap, isNull);

    await tester.tap(find.text('Send'));
    await tester.tap(find.text('Swap'), warnIfMissed: false);

    expect(sendTaps, 1);
    expect(swapTaps, 0);
  });

  testWidgets('gives payment verification its own visual role', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PaySheet(
            onSend: () {},
            onReceive: () {},
            onVerify: () {},
            onSwap: () {},
          ),
        ),
      ),
    );

    final verifyTile = find.ancestor(
      of: find.text('Verify'),
      matching: find.byType(Ink),
    );
    final ink = tester.widget<Ink>(verifyTile);
    final decoration = ink.decoration! as BoxDecoration;
    final gradient = decoration.gradient! as LinearGradient;

    expect(gradient.colors, [AppColors.gradientCStart, AppColors.gradientCEnd]);
    expect(gradient.colors, isNot(contains(AppColors.gradientBEnd)));
  });

  testWidgets('keeps every payment action reachable in phone landscape', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(844, 390);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PaySheet(
            onSend: () {},
            onReceive: () {},
            onVerify: () {},
            onSwap: () {},
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Send'), findsOneWidget);
    expect(find.text('Receive'), findsOneWidget);
    expect(find.text('Verify'), findsOneWidget);
    expect(find.text('Swap'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('fits action cards into a scaled Ubuntu laptop viewport', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1097, 706);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(const MaterialApp(home: PayScreen()));
    await tester.pump();

    final sendTile = find.ancestor(
      of: find.text('Send'),
      matching: find.byType(Ink),
    );
    expect(tester.getSize(sendTile).height, lessThanOrEqualTo(230));
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });
}
