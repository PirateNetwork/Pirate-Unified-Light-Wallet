import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/send/widgets/send_error_banner.dart';

Widget _testApp({required double width, required double textScale}) {
  return MaterialApp(
    theme: PTheme.dark(),
    home: MediaQuery(
      data: MediaQueryData(
        size: Size(width, 800),
        textScaler: TextScaler.linear(textScale),
      ),
      child: Scaffold(
        body: Align(
          alignment: Alignment.topCenter,
          child: SizedBox(
            width: width,
            child: const Padding(
              padding: EdgeInsets.all(16),
              child: SendErrorBanner(
                message: 'The selected notes cannot cover this amount and its network fee. Choose another source and try again.',
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

void main() {
  testWidgets('wraps safely on a narrow phone with large text', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 800);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp(width: 320, textScale: 2));

    final banner = tester.getRect(find.byType(SendErrorBanner));
    final message = tester.getRect(find.byKey(SendErrorBanner.messageKey));
    expect(message.left, greaterThanOrEqualTo(banner.left));
    expect(message.right, lessThanOrEqualTo(banner.right));
    expect(tester.takeException(), isNull);
  });

  testWidgets('keeps a bounded readable line length on desktop', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1024, 800);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp(width: 720, textScale: 1));

    final banner = tester.getRect(find.byType(SendErrorBanner));
    final message = tester.getRect(find.byKey(SendErrorBanner.messageKey));
    expect(message.right, lessThanOrEqualTo(banner.right));
    expect(tester.takeException(), isNull);
  });

  testWidgets('announces the failure once as a live status', (tester) async {
    await tester.pumpWidget(_testApp(width: 320, textScale: 1));

    final semantics = tester.widget<Semantics>(
      find.byKey(SendErrorBanner.semanticsKey),
    );
    expect(semantics.container, isTrue);
    expect(semantics.properties.liveRegion, isTrue);
    expect(semantics.properties.label, contains('selected notes'));
  });
}
