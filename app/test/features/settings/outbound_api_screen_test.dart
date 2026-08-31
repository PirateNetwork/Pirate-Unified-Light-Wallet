import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/screens/outbound_api_screen.dart';

import '../../support/test_font_loader.dart';

const _captureBoundaryKey = ValueKey('outbound-api-capture');

class _ThemeModeTestNotifier extends ThemeModeNotifier {
  @override
  AppThemeMode build() => AppThemeMode.dark;
}

class _ExternalApiMasterTestNotifier extends ExternalApiMasterNotifier {
  @override
  bool build() => true;
}

class _ExternalPriceApiTestNotifier extends ExternalPriceApiNotifier {
  @override
  bool build() => true;
}

class _ExternalGithubApiTestNotifier extends ExternalGithubApiNotifier {
  @override
  bool build() => true;
}

class _ExternalDesktopUpdateApiTestNotifier
    extends ExternalDesktopUpdateApiNotifier {
  @override
  bool build() => true;
}

class _ExternalKomodoSwapApiTestNotifier extends ExternalKomodoSwapApiNotifier {
  @override
  bool build() => true;
}

Widget _testApp() {
  return ProviderScope(
    overrides: [
      appThemeModeProvider.overrideWith(_ThemeModeTestNotifier.new),
      externalApiMasterProvider.overrideWith(
        _ExternalApiMasterTestNotifier.new,
      ),
      externalPriceApiProvider.overrideWith(_ExternalPriceApiTestNotifier.new),
      externalGithubApiProvider.overrideWith(
        _ExternalGithubApiTestNotifier.new,
      ),
      externalDesktopUpdateApiProvider.overrideWith(
        _ExternalDesktopUpdateApiTestNotifier.new,
      ),
      externalKomodoSwapApiProvider.overrideWith(
        _ExternalKomodoSwapApiTestNotifier.new,
      ),
    ],
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: PTheme.dark(),
      home: const RepaintBoundary(
        key: _captureBoundaryKey,
        child: OutboundApiScreen(),
      ),
    ),
  );
}

Future<void> _captureIfRequested(WidgetTester tester, String filename) async {
  final outputDirectory = Platform.environment['PIRATE_UI_CAPTURE_DIR'];
  if (outputDirectory == null || outputDirectory.isEmpty) return;

  final path = '$outputDirectory${Platform.pathSeparator}$filename';
  await expectLater(
    find.byKey(_captureBoundaryKey),
    matchesGoldenFile(Uri.file(path)),
  );
}

void main() {
  setUpAll(() async {
    await loadTestFont('Sora', 'assets/fonts/Sora/Sora.ttf');
  });

  testWidgets('phone layout discloses price providers without overflow', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(390, 844);
    addTearDown(() {
      debugDefaultTargetPlatformOverride = null;
      tester.view.reset();
    });

    await tester.pumpWidget(_testApp());
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.text('Live Price Feeds'), findsOneWidget);
    expect(find.textContaining('CoinPaprika'), findsOneWidget);
    expect(find.text('Desktop Update Checks'), findsNothing);
    expect(tester.takeException(), isNull);
    await _captureIfRequested(tester, 'outbound-api-phone.png');
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('desktop layout discloses price providers without overflow', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(() {
      debugDefaultTargetPlatformOverride = null;
      tester.view.reset();
    });

    await tester.pumpWidget(_testApp());
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.text('Live Price Feeds'), findsOneWidget);
    expect(find.textContaining('CoinPaprika'), findsOneWidget);
    expect(find.text('Desktop Update Checks'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await _captureIfRequested(tester, 'outbound-api-desktop.png');
    debugDefaultTargetPlatformOverride = null;
  });
}
