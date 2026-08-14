import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/main.dart';

void main() {
  testWidgets('desktop vertical scrolling uses a trackless scrollbar', (
    tester,
  ) async {
    final controller = ScrollController();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: PTheme.dark().copyWith(platform: TargetPlatform.windows),
        scrollBehavior: const PirateScrollBehavior(),
        home: Scaffold(
          body: ListView.builder(
            controller: controller,
            itemCount: 100,
            itemBuilder: (_, index) =>
                SizedBox(height: 40, child: Text('Item $index')),
          ),
        ),
      ),
    );

    final scrollbar = tester.widget<Scrollbar>(find.byType(Scrollbar));
    expect(scrollbar.thumbVisibility, isTrue);
    expect(scrollbar.trackVisibility, isFalse);
    expect(scrollbar.interactive, isTrue);
    expect(scrollbar.controller, same(controller));
  });
}
