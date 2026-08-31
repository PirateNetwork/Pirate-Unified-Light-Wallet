import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/release_verification_service.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/verify_build_screen.dart';

import '../../support/test_font_loader.dart';

const _captureBoundaryKey = ValueKey('verify-build-capture');

class _ThemeModeTestNotifier extends ThemeModeNotifier {
  @override
  AppThemeMode build() => AppThemeMode.dark;
}

const _buildInfo = <String, String>{
  'version': '1.1.9',
  'gitCommit': '0123456789abcdef0123456789abcdef01234567',
  'buildDate': '2026-08-29T12:00:00Z',
  'rustVersion': '1.91.0',
  'targetTriple': 'x86_64-pc-windows-msvc',
};

const _verifiedResult = ReleaseVerificationResult(
  status: ReleaseVerificationStatus.match,
  reason: ReleaseVerificationReason.none,
  releaseTag: 'v1.1.9',
  releaseUrl: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/releases/tag/v1.1.9',
  signatureAssetName: 'signatures-v1.1.9.zip',
  checksumAssetName: 'build-payloads-v1.1.9.txt',
  localArtifactPath: r'C:\Program Files\Stashi Wallet\Stashi Wallet.exe',
  localArtifactName: 'Stashi Wallet.exe',
  localHash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  expectedHash:
      'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  matchedChecksumName: 'Stashi Wallet.exe',
);

const _unavailableResult = ReleaseVerificationResult(
  status: ReleaseVerificationStatus.unavailable,
  reason: ReleaseVerificationReason.downloadFailed,
  releaseTag: 'v1.1.9',
  releaseUrl: 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/releases/tag/v1.1.9',
  signatureAssetName: 'signatures-v1.1.9.zip',
  localArtifactPath:
      '/Applications/Stashi Wallet.app/Contents/MacOS/Stashi Wallet',
  localArtifactName: 'Stashi Wallet',
  localHash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
);

Future<void> _pumpScreen(
  WidgetTester tester,
  Size size, {
  ReleaseVerificationResult result = _verifiedResult,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);

  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appThemeModeProvider.overrideWith(_ThemeModeTestNotifier.new),
        allowGithubApisProvider.overrideWithValue(true),
      ],
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: PTheme.dark(),
        home: RepaintBoundary(
          key: _captureBoundaryKey,
          child: VerifyBuildScreen(
            buildInfoLoader: () async => _buildInfo,
            releaseVerifier: (_, _) async => result,
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
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
    await loadTestFont(
      'monospace',
      'assets/fonts/JetBrainsMono/JetBrainsMono.ttf',
    );

    final materialIconsPath =
        Platform.environment['PIRATE_MATERIAL_ICONS_FONT'];
    if (materialIconsPath != null && File(materialIconsPath).existsSync()) {
      await loadTestFont('MaterialIcons', materialIconsPath);
    }
  });

  testWidgets('stacks release and build details at phone width', (
    tester,
  ) async {
    await _pumpScreen(tester, const Size(360, 900));

    final verification = tester.getTopLeft(
      find.text('Official Release Verification'),
    );
    final buildInfo = tester.getTopLeft(find.text('Build Information'));
    expect(buildInfo.dy, greaterThan(verification.dy));
    expect(find.text('Verify now'), findsOneWidget);
    expect(find.text('Technical details'), findsOneWidget);
    expect(find.text('Local SHA256'), findsNothing);
    expect(tester.takeException(), isNull);
    await _captureIfRequested(tester, 'verify-build-phone.png');
  });

  testWidgets('uses a balanced two-column desktop layout', (tester) async {
    await _pumpScreen(tester, const Size(1280, 900));

    final verification = tester.getTopLeft(
      find.text('Official Release Verification'),
    );
    final buildInfo = tester.getTopLeft(find.text('Build Information'));
    expect((buildInfo.dy - verification.dy).abs(), lessThan(2));
    expect(buildInfo.dx, greaterThan(verification.dx));
    expect(tester.takeException(), isNull);
    await _captureIfRequested(tester, 'verify-build-desktop.png');
  });

  testWidgets('does not present a network failure as a failed build', (
    tester,
  ) async {
    await _pumpScreen(
      tester,
      const Size(1280, 900),
      result: _unavailableResult,
    );

    expect(find.text('Check unavailable'), findsOneWidget);
    expect(find.text('Error'), findsNothing);
    expect(find.text('Stashi Wallet'), findsOneWidget);
    expect(find.textContaining('does not mean the app failed'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await _captureIfRequested(tester, 'verify-build-unavailable-desktop.png');
  });

  testWidgets('reveals signed manifest details on demand', (tester) async {
    await _pumpScreen(tester, const Size(390, 1100));

    await tester.scrollUntilVisible(
      find.text('Technical details'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.tap(find.text('Technical details'));
    await tester.pumpAndSettle();

    expect(find.text('Local SHA256'), findsOneWidget);
    expect(find.text('Signature Asset'), findsOneWidget);
    expect(find.text('signatures-v1.1.9.zip'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
