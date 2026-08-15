import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/home/widgets/home_sync_indicator.dart';

Widget _testApp({
  double width = 328,
  double textScale = 1,
  double blocksPerSecond = 0,
}) {
  return MaterialApp(
    theme: PTheme.dark(),
    home: MediaQuery(
      data: MediaQueryData(
        size: Size(width, 800),
        textScaler: TextScaler.linear(textScale),
        disableAnimations: true,
      ),
      child: Scaffold(
        body: Align(
          alignment: Alignment.topCenter,
          child: SizedBox(
            width: width,
            child: HomeSyncIndicator(
              progress: 0,
              currentHeight: 2426013,
              targetHeight: 4081234,
              stage: 'Fetching headers',
              eta: 'Calculating...',
              blocksPerSecond: blocksPerSecond,
              isSyncing: true,
              isComplete: false,
              reduceMotion: true,
            ),
          ),
        ),
      ),
    ),
  );
}

void main() {
  testWidgets('shows complete sync metrics within a narrow mobile card', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    await tester.binding.setSurfaceSize(const Size(360, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp());
    await tester.pump();

    expect(find.text('Fetching headers'), findsOneWidget);
    expect(find.text('2,426,013'), findsOneWidget);
    expect(find.text('4,081,234'), findsOneWidget);
    expect(find.text('Calculating...'), findsOneWidget);

    final currentRect = tester.getRect(
      find.byKey(HomeSyncIndicator.currentHeightKey),
    );
    final targetRect = tester.getRect(
      find.byKey(HomeSyncIndicator.targetHeightKey),
    );
    final etaRect = tester.getRect(find.byKey(HomeSyncIndicator.etaKey));

    expect(currentRect.center.dy, closeTo(targetRect.center.dy, 0.5));
    expect(currentRect.right, lessThan(targetRect.left));
    expect(etaRect.top, greaterThan(currentRect.bottom));
    expect(currentRect.left, greaterThanOrEqualTo(16));
    expect(targetRect.right, lessThanOrEqualTo(344));
    expect(
      tester.getSemantics(find.bySemanticsLabel('Wallet sync status')).value,
      'Block 2426013 / 4081234',
    );
    semantics.dispose();
    expect(tester.takeException(), isNull);
  });

  testWidgets('reflows metrics for enlarged mobile text', (tester) async {
    await tester.binding.setSurfaceSize(const Size(320, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp(width: 288, textScale: 2));
    await tester.pump();

    final currentRect = tester.getRect(
      find.byKey(HomeSyncIndicator.currentHeightKey),
    );
    final targetRect = tester.getRect(
      find.byKey(HomeSyncIndicator.targetHeightKey),
    );
    final etaRect = tester.getRect(find.byKey(HomeSyncIndicator.etaKey));

    expect(targetRect.top, greaterThan(currentRect.bottom));
    expect(etaRect.top, greaterThan(targetRect.bottom));
    expect(tester.takeException(), isNull);
  });

  testWidgets('moves live speed into the responsive metrics grid', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(360, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp(blocksPerSecond: 21543.7));
    await tester.pump();

    expect(find.text('blk/s'), findsOneWidget);
    expect(find.text('21543.7'), findsOneWidget);
    expect(
      tester.getRect(find.byKey(HomeSyncIndicator.speedKey)).top,
      greaterThan(
        tester.getRect(find.byKey(HomeSyncIndicator.currentHeightKey)).bottom,
      ),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('explains sync metrics through concise tooltips', (tester) async {
    await tester.binding.setSurfaceSize(const Size(1200, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(
      _testApp(width: 1000, blocksPerSecond: 21543.7),
    );
    await tester.pump();

    expect(find.byTooltip('Current wallet block height'), findsOneWidget);
    expect(find.byTooltip('Current chain tip'), findsOneWidget);
    expect(find.byTooltip('Estimated time remaining'), findsOneWidget);
    expect(find.byTooltip('Current sync speed'), findsOneWidget);
  });
}
