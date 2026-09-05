import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:pirate_wallet/core/desktop/desktop_update_prompt_host.dart';
import 'package:pirate_wallet/core/services/desktop_update_service.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';

import 'support/test_font_loader.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    await loadTestFont('Sora', 'assets/fonts/Sora/Sora.ttf');
    final icons = Platform.environment['PIRATE_MATERIAL_ICONS_FONT'];
    if (icons != null) await loadTestFont('MaterialIcons', icons);
  });
  testWidgets(
    'update prompt works above Navigator and Later does not skip release',
    (tester) async {
      FlutterSecureStorage.setMockInitialValues({});
      tester.view.physicalSize = const Size(1120, 760);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final navigator = GlobalKey<NavigatorState>();
      final boundary = GlobalKey();
      await tester.pumpWidget(
        ProviderScope(
          overrides: [allowDesktopUpdateApisProvider.overrideWithValue(true)],
          child: MaterialApp(
            navigatorKey: navigator,
            theme: PTheme.dark(),
            debugShowCheckedModeBanner: false,
            builder: (context, child) => RepaintBoundary(
              key: boundary,
              child: DesktopUpdatePromptHost(
                navigatorKey: navigator,
                child: child!,
                checkForUpdate: () async => DesktopUpdateCandidate(
                  currentVersion: '1.2.0',
                  release: DesktopReleaseInfo(
                    tagName: 'v1.2.1',
                    name: 'Stashi Wallet',
                    releaseUrl: '',
                    publishedAt: DateTime(2026),
                    isDraft: false,
                    isPrerelease: false,
                    assets: [],
                  ),
                  asset: const DesktopReleaseAsset(
                    name: 'installer.exe',
                    downloadUrl: '',
                  ),
                  assetKind: DesktopUpdateAssetKind.windowsInstaller,
                ),
              ),
            ),
            home: const Scaffold(body: Center(child: Text('Stashi Wallet'))),
          ),
        ),
      );
      await tester.pump(const Duration(seconds: 26));
      await tester.pumpAndSettle();
      expect(find.text('A new Stashi Wallet is ready'), findsOneWidget);
      expect(tester.takeException(), isNull);
      final dir = Platform.environment['PIRATE_UI_CAPTURE_DIR'];
      if (dir != null) {
        await tester.runAsync(() async {
          final image =
              await (boundary.currentContext!.findRenderObject()!
                      as RenderRepaintBoundary)
                  .toImage(pixelRatio: 2);
          final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
          await File('$dir/02-update.png')
              .writeAsBytes(bytes!.buffer.asUint8List());
          image.dispose();
        });
      }
      await tester.tap(find.text('Later'));
      await tester.pumpAndSettle();
      await tester.pump(const Duration(minutes: 30));
      await tester.pumpAndSettle();
      expect(find.text('A new Stashi Wallet is ready'), findsOneWidget);
      await tester.tap(find.text('Skip this version'));
      await tester.pumpAndSettle();
      await tester.pump(const Duration(minutes: 30));
      await tester.pumpAndSettle();
      expect(find.text('A new Stashi Wallet is ready'), findsNothing);
      await tester.pumpWidget(const SizedBox());
    },
  );
}
