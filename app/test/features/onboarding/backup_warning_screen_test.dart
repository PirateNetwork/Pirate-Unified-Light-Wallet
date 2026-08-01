import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/onboarding/screens/backup_warning_screen.dart';

void main() {
  testWidgets('backup acknowledgment is centered with warning content', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1180, 760);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          theme: ThemeData(splashFactory: NoSplash.splashFactory),
          home: const BackupWarningScreen(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final acknowledgment = tester.getRect(
      find.byKey(const Key('seed-backup-acknowledgment')),
    );
    expect(acknowledgment.width, 520);
    expect(acknowledgment.center.dx, 590);
  });
}
