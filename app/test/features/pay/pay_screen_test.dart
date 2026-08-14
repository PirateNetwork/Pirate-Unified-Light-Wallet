import 'package:flutter/material.dart';
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
}
