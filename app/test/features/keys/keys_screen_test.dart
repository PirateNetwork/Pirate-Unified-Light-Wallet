import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/keys/keys_screen.dart';

class _ActiveWallet extends ActiveWalletNotifier {
  @override
  String? build() => 'key-management-test-wallet';
}

class _DecoyMode extends DecoyModeNotifier {
  @override
  bool build() => true;
}

Widget _testApp() {
  return ProviderScope(
    overrides: [
      activeWalletProvider.overrideWith(_ActiveWallet.new),
      decoyModeProvider.overrideWith(_DecoyMode.new),
    ],
    child: const MaterialApp(home: KeyManagementScreen()),
  );
}

void main() {
  testWidgets('seed account actions stack on mobile and expose simple help', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(390, 844));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final semantics = tester.ensureSemantics();

    await tester.pumpWidget(_testApp());
    await tester.pumpAndSettle();

    expect(find.text('Seed accounts'), findsOneWidget);
    expect(find.text('Next #1'), findsOneWidget);
    expect(find.text('Add next account'), findsOneWidget);
    expect(find.text('Add 5 accounts'), findsOneWidget);
    expect(
      find.bySemanticsLabel(RegExp('Seed account management')),
      findsOneWidget,
    );

    final addOne = tester.getRect(find.text('Add next account'));
    final addFive = tester.getRect(find.text('Add 5 accounts'));
    expect(addFive.top, greaterThan(addOne.bottom));

    await tester.longPress(find.text('Add 5 accounts'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(
      find.textContaining('It does not stop at empty accounts'),
      findsOneWidget,
    );
    semantics.dispose();
  });

  testWidgets('seed account actions share a row on desktop', (tester) async {
    await tester.binding.setSurfaceSize(const Size(1100, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp());
    await tester.pumpAndSettle();

    final addOne = tester.getRect(find.text('Add next account'));
    final addFive = tester.getRect(find.text('Add 5 accounts'));
    expect((addOne.center.dy - addFive.center.dy).abs(), lessThan(1));
    expect(addFive.left, greaterThan(addOne.right));
  });
}
