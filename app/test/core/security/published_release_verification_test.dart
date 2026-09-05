import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/release_verification_service.dart';

void main() {
  for (final tag in ['v1.1.9', 'v1.2.0']) {
    test('published $tag RSA release signatures authenticate', () async {
      final bundle = await File('test/fixtures/releases/signatures-$tag.zip')
          .readAsBytes();
      final archive = ZipDecoder().decodeBytes(bundle);
      final lines = utf8
          .decode(archive.findFile('build-payloads-$tag.txt')!.readBytes()!)
          .trim()
          .split('\n');
      for (final line in lines) {
        final hash = line.substring(0, 64);
        final name = line.substring(66).trim();
        for (final installedName in [name, 'renamed-installed-binary']) {
          final result = await ReleaseVerificationService(
            downloadBytes: (_) async => bundle,
            loadAsset: (_) =>
                File('assets/security/public_key.asc').readAsString(),
            loadLocalArtifacts: () async => [
              LocalReleaseArtifact(
                path: '/installed/$installedName',
                name: installedName,
                sha256: hash,
              ),
            ],
          ).verify(tag);
          expect(
            result.status,
            ReleaseVerificationStatus.match,
            reason: '$name: ${result.reason.name}',
          );
          expect(result.matchedChecksumName, name);
        }
        final changed = await ReleaseVerificationService(
          downloadBytes: (_) async => bundle,
          loadAsset: (_) =>
              File('assets/security/public_key.asc').readAsString(),
          loadLocalArtifacts: () async => [
            LocalReleaseArtifact(
              path: '/installed/$name',
              name: name,
              sha256: '0' * 64,
            ),
          ],
        ).verify(tag);
        expect(changed.reason, ReleaseVerificationReason.checksumMismatch);
      }
      final wrongClock = await ReleaseVerificationService(
        downloadBytes: (_) async => bundle,
        loadAsset: (_) => File('assets/security/public_key.asc').readAsString(),
        loadLocalArtifacts: () async => [],
        clock: () => DateTime.utc(2020),
      ).verify(tag);
      expect(wrongClock.reason, ReleaseVerificationReason.deviceClockIncorrect);
      expect(wrongClock.status, ReleaseVerificationStatus.unavailable);
    });
  }
}
