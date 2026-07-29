import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/legal/privacy_policy_dialog.dart';

void main() {
  testWidgets('privacy policy link opens a scrollable, closable dialog', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 500);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: NoSplash.splashFactory),
        home: const Scaffold(body: Center(child: PrivacyPolicyAgreement())),
      ),
    );

    await tester.tap(find.byKey(privacyPolicyLinkKey));
    await tester.pumpAndSettle();

    final dialog = find.byKey(privacyPolicyDialogKey);
    expect(dialog, findsOneWidget);
    expect(
      find.descendant(
        of: dialog,
        matching: find.byKey(privacyPolicyScrollViewKey),
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: dialog,
        matching: find.textContaining('does not send seed phrases'),
      ),
      findsOneWidget,
    );

    final scrollable = tester.state<ScrollableState>(
      find.descendant(of: dialog, matching: find.byType(Scrollable)),
    );
    expect(scrollable.position.maxScrollExtent, greaterThan(0));

    await tester.tap(find.byKey(privacyPolicyCloseKey));
    await tester.pumpAndSettle();

    expect(find.byKey(privacyPolicyDialogKey), findsNothing);
    expect(find.byKey(privacyPolicyLinkKey), findsOneWidget);
  });
}
