import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/release_verification_service.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/verify_build_screen.dart';

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
  localArtifactPath: r'C:\Program Files\Pirate Wallet\app.exe',
  localArtifactName: 'app.exe',
  localHash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  expectedHash:
      'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  matchedChecksumName: 'app.exe',
);

Future<void> _pumpScreen(WidgetTester tester, Size size) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);

  await tester.pumpWidget(
    ProviderScope(
      overrides: [allowGithubApisProvider.overrideWithValue(true)],
      child: MaterialApp(
        theme: ThemeData.dark(),
        home: VerifyBuildScreen(
          buildInfoLoader: () async => _buildInfo,
          releaseVerifier: (_, _) async => _verifiedResult,
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
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
