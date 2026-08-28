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

Widget _testApp({double textScale = 1}) {
  return ProviderScope(
    overrides: [
      activeWalletProvider.overrideWith(_ActiveWallet.new),
      decoyModeProvider.overrideWith(_DecoyMode.new),
    ],
    child: MaterialApp(
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(textScale)),
        child: child!,
      ),
      home: const KeyManagementScreen(),
    ),
  );
}

void main() {
  testWidgets('phone layout stacks actions and opens account help', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(390, 844));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final semantics = tester.ensureSemantics();

    await tester.pumpWidget(_testApp());
    await tester.pumpAndSettle();

    expect(find.text('Seed accounts'), findsOneWidget);
    expect(find.text('Next account #1'), findsOneWidget);
    expect(find.text('Add next account'), findsOneWidget);
    expect(find.text('Add 5 accounts'), findsOneWidget);
    expect(find.text('Import keys'), findsOneWidget);
    expect(find.text('Keys'), findsOneWidget);
    expect(
      find.bySemanticsLabel(RegExp('Seed account management')),
      findsOneWidget,
    );

    final addOne = tester.getRect(find.text('Add next account'));
    final addFive = tester.getRect(find.text('Add 5 accounts'));
    expect(addFive.top, greaterThan(addOne.bottom));

    await tester.tap(find.text('How seed accounts work'));
    await tester.pumpAndSettle();
    expect(find.text('How seed accounts work'), findsNWidgets(2));
    expect(
      find.textContaining('Each account has its own Sapling and Ironwood'),
      findsOneWidget,
    );
    semantics.dispose();
  });

  testWidgets(
    'tablet layout keeps panels stacked and import choices in a row',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(820, 1180));
      addTearDown(() => tester.binding.setSurfaceSize(null));

      await tester.pumpWidget(_testApp());
      await tester.pumpAndSettle();

      final seed = tester.getRect(
        find.byKey(KeyManagementScreen.seedAccountsCardKey),
      );
      final imports = tester.getRect(
        find.byKey(KeyManagementScreen.importKeysCardKey),
      );
      expect(imports.top, greaterThan(seed.bottom));

      final spending = tester.getRect(find.text('Spending Key'));
      final viewing = tester.getRect(find.text('Viewing Key'));
      expect((spending.center.dy - viewing.center.dy).abs(), lessThan(1));
    },
  );

  testWidgets(
    'desktop layout pairs overview panels and keeps actions aligned',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1200, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));

      await tester.pumpWidget(_testApp());
      await tester.pumpAndSettle();

      final seed = tester.getRect(
        find.byKey(KeyManagementScreen.seedAccountsCardKey),
      );
      final imports = tester.getRect(
        find.byKey(KeyManagementScreen.importKeysCardKey),
      );
      expect((seed.top - imports.top).abs(), lessThan(1));
      expect(imports.left, greaterThan(seed.right));

      final addOne = tester.getRect(find.text('Add next account'));
      final addFive = tester.getRect(find.text('Add 5 accounts'));
      expect((addOne.center.dy - addFive.center.dy).abs(), lessThan(1));
      expect(addFive.left, greaterThan(addOne.right));
    },
  );

  testWidgets('compact landscape remains scrollable without overflow', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(844, 390));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp());
    await tester.pumpAndSettle();
    await tester.drag(find.byType(ListView), const Offset(0, -500));
    await tester.pumpAndSettle();

    expect(find.text('Keys'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('phone layout supports large text without overflow', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(390, 844));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp(textScale: 1.8));
    await tester.pumpAndSettle();

    expect(find.text('Seed accounts'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
