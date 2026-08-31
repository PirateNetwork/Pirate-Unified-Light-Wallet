import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/home/widgets/home_header_controls.dart';

const _horizontalGutter = 16.0;

Widget _testApp({
  required double width,
  double textScale = 1.0,
  bool showConnectionStatus = true,
}) {
  return ProviderScope(
    overrides: [
      activeWalletMetaProvider.overrideWithValue(
        WalletMeta(
          id: 'wallet-1',
          name: 'My Stashi Wallet with a deliberately long name',
          createdAt: 0,
          watchOnly: true,
          birthdayHeight: 1,
          networkType: 'mainnet',
        ),
      ),
      connectionStatusLevelProvider.overrideWithValue(
        ConnectionStatusLevel.secure,
      ),
    ],
    child: MaterialApp(
      theme: PTheme.dark(),
      home: MediaQuery(
        data: MediaQueryData(
          size: Size(width, 800),
          textScaler: TextScaler.linear(textScale),
        ),
        child: Scaffold(
          body: Padding(
            padding: const EdgeInsets.symmetric(horizontal: _horizontalGutter),
            child: HomeHeaderControls(
              onConnectionTap: _doNothing,
              showConnectionStatus: showConnectionStatus,
            ),
          ),
        ),
      ),
    ),
  );
}

void _doNothing() {}

void main() {
  testWidgets('stacks header controls within narrow mobile bounds', (
    tester,
  ) async {
    const width = 320.0;
    await tester.binding.setSurfaceSize(const Size(width, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp(width: width, textScale: 1.6));
    await tester.pump();

    final walletRect = tester.getRect(
      find.byKey(HomeHeaderControls.walletControlKey),
    );
    final connectionRect = tester.getRect(
      find.byKey(HomeHeaderControls.connectionControlKey),
    );

    expect(walletRect.left, _horizontalGutter);
    expect(walletRect.right, width - _horizontalGutter);
    expect(connectionRect.top, greaterThan(walletRect.bottom));
    expect(connectionRect.left, _horizontalGutter);
    expect(connectionRect.right, lessThanOrEqualTo(width - _horizontalGutter));
    expect(tester.takeException(), isNull);
  });

  testWidgets('keeps header controls on one row when space permits', (
    tester,
  ) async {
    const width = 720.0;
    await tester.binding.setSurfaceSize(const Size(width, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(_testApp(width: width));
    await tester.pump();

    final walletRect = tester.getRect(
      find.byKey(HomeHeaderControls.walletControlKey),
    );
    final connectionRect = tester.getRect(
      find.byKey(HomeHeaderControls.connectionControlKey),
    );

    expect(walletRect.right, lessThan(connectionRect.left));
    expect(walletRect.center.dy, closeTo(connectionRect.center.dy, 0.5));
    expect(connectionRect.right, width - _horizontalGutter);
    expect(tester.takeException(), isNull);
  });

  testWidgets('defers connection state to the desktop status bar', (
    tester,
  ) async {
    const width = 720.0;
    await tester.binding.setSurfaceSize(const Size(width, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(
      _testApp(width: width, showConnectionStatus: false),
    );
    await tester.pump();

    final walletRect = tester.getRect(
      find.byKey(HomeHeaderControls.walletControlKey),
    );
    expect(find.byKey(HomeHeaderControls.connectionControlKey), findsNothing);
    expect(walletRect.right, width - _horizontalGutter);
    expect(tester.takeException(), isNull);
  });
}
