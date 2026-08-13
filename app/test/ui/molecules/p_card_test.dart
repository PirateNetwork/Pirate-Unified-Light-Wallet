import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/molecules/p_card.dart';

void main() {
  testWidgets('clips child content to its rounded corners', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Center(
          child: SizedBox(
            width: 120,
            height: 80,
            child: PCard(
              padding: EdgeInsets.zero,
              child: ColoredBox(color: Colors.green),
            ),
          ),
        ),
      ),
    );

    final cardContainer = tester.widget<AnimatedContainer>(
      find.descendant(
        of: find.byType(PCard),
        matching: find.byType(AnimatedContainer),
      ),
    );
    expect(cardContainer.clipBehavior, Clip.antiAlias);
  });
}
