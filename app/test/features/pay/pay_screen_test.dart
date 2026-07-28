import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/swaps/swap_availability.dart';
import 'package:pirate_wallet/features/pay/pay_screen.dart';

void main() {
  testWidgets('disables the swap action while swaps are unreleased', (
    tester,
  ) async {
    var sendTaps = 0;
    var swapTaps = 0;

    await tester.pumpWidget(
      MaterialApp(
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
}
