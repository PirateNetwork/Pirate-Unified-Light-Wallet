import 'dart:convert';
import 'dart:typed_data';

import 'package:archive/archive.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/security/release_verification_service.dart';

const _tag = 'v1.1.9';
const _fixtureName = 'fixture.bin';
const _fixtureHash =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const _manifest = '$_fixtureHash  $_fixtureName\n';
const _fixtureSigningKeyId = '4e928affb92f2622';
const _fixturePublicKey = '''
-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEapH9CxYJKwYBBAHaRw8BAQdAI1w/rAJyGgwZ21if0sZWbl8TE8O2YBG4g0Jd
B5tQpK20OFJlbGVhc2UgdmVyaWZpY2F0aW9uIHRlc3QgZml4dHVyZSA8dGVzdEBl
eGFtcGxlLmludmFsaWQ+iK8EExYKAFcWIQTGF4xLrtC5G9r/vIdOkor/uS8mIgUC
apH9CxsUgAAAAAAEAA5tYW51MiwyLjUrMS4xMSwyLDECGwMFCwkIBwICIgIGFQoJ
CAsCBBYCAwECHgcCF4AACgkQTpKK/7kvJiJj0QEA10P8GfaeZ8b0rwKOpngabfhV
uFxcQka1uM0Nh2r8ez4BAP8+/Bu5ZPHFByj6KgcCqtxH7PABAEi+sWoF5iicoToF
=z7k3
-----END PGP PUBLIC KEY BLOCK-----
''';
const _signatureBase64 =
    'iJEEABYKADkWIQTGF4xLrtC5G9r/vIdOkor/uS8mIgUCapH9CxsUgAAAAAAEAA5tYW51MiwyLjUrMS4xMSwyLDEACgkQTpKK/7kvJiKJJAEAukxL7ghZFZpAOPEfaF+Gcr+wUpBDxrQRhhucigvjBRABAKvvb30Q8xSRM57VcVGLCTtWemo4UI3GUOcMIuzJHGQM';

Uint8List _bundle({String manifest = _manifest}) {
  final manifestBytes = utf8.encode(manifest);
  final signature = base64.decode(_signatureBase64);
  final archive = Archive()
    ..addFile(
      ArchiveFile('sha256sum-$_tag.txt', manifestBytes.length, manifestBytes),
    )
    ..addFile(
      ArchiveFile('sha256sum-$_tag.txt.sig', signature.length, signature),
    );
  return Uint8List.fromList(ZipEncoder().encode(archive));
}

Uint8List _nestedBundle() {
  final manifestBytes = utf8.encode(_manifest);
  final signature = base64.decode(_signatureBase64);
  final archive = Archive()
    ..addFile(
      ArchiveFile(
        'nested/sha256sum-$_tag.txt',
        manifestBytes.length,
        manifestBytes,
      ),
    )
    ..addFile(
      ArchiveFile(
        'nested/sha256sum-$_tag.txt.sig',
        signature.length,
        signature,
      ),
    );
  return Uint8List.fromList(ZipEncoder().encode(archive));
}

ReleaseVerificationService _fixtureService({
  String manifest = _manifest,
  String expectedSigningKeyId = _fixtureSigningKeyId,
}) {
  return ReleaseVerificationService(
    downloadBytes: (_) async => _bundle(manifest: manifest),
    loadAsset: (_) async => _fixturePublicKey,
    loadLocalArtifacts: () async => const [
      LocalReleaseArtifact(
        path: '/download/fixture.bin',
        name: _fixtureName,
        sha256: _fixtureHash,
      ),
    ],
    expectedSigningKeyId: expectedSigningKeyId,
  );
}

void main() {
  test('accepts a valid detached signature from the pinned signer', () async {
    final result = await _fixtureService().verify(_tag);

    expect(result.status, ReleaseVerificationStatus.match);
    expect(result.checksumAssetName, 'sha256sum-$_tag.txt');
    expect(result.signatureAssetName, 'signatures-$_tag.zip');
  });

  test('rejects a checksum manifest changed after signing', () async {
    final result = await _fixtureService(
      manifest: _manifest.replaceFirst('a', 'b'),
    ).verify(_tag);

    expect(result.status, ReleaseVerificationStatus.mismatch);
    expect(result.reason, ReleaseVerificationReason.signatureInvalid);
  });

  test('rejects a valid signature made by a different signer', () async {
    final result = await _fixtureService(
      expectedSigningKeyId: '0000000000000000',
    ).verify(_tag);

    expect(result.status, ReleaseVerificationStatus.mismatch);
    expect(result.reason, ReleaseVerificationReason.signatureInvalid);
  });

  test('rejects archive entries outside the flat signature layout', () async {
    final service = ReleaseVerificationService(
      downloadBytes: (_) async => _nestedBundle(),
      loadAsset: (_) async => _fixturePublicKey,
      loadLocalArtifacts: () async => const [],
      expectedSigningKeyId: _fixtureSigningKeyId,
    );

    final result = await service.verify(_tag);

    expect(result.status, ReleaseVerificationStatus.error);
    expect(result.reason, ReleaseVerificationReason.invalidVerificationFiles);
  });

  test('retries transient release download failures', () async {
    var attempts = 0;
    final delays = <Duration>[];
    final service = ReleaseVerificationService(
      downloadBytes: (_) async {
        attempts++;
        if (attempts < 3) {
          throw Exception('temporary transport error');
        }
        return _bundle();
      },
      loadAsset: (_) async => _fixturePublicKey,
      loadLocalArtifacts: () async => const [
        LocalReleaseArtifact(
          path: '/download/fixture.bin',
          name: _fixtureName,
          sha256: _fixtureHash,
        ),
      ],
      retryDelay: (duration) async => delays.add(duration),
      expectedSigningKeyId: _fixtureSigningKeyId,
    );

    final result = await service.verify(_tag);

    expect(result.status, ReleaseVerificationStatus.match);
    expect(attempts, 3);
    expect(delays, const [Duration(milliseconds: 500), Duration(seconds: 2)]);
  });

  test(
    'keeps the local hash when the release download is unavailable',
    () async {
      final service = ReleaseVerificationService(
        downloadBytes: (_) async => throw Exception('connection timed out'),
        loadAsset: (_) async => _fixturePublicKey,
        loadLocalArtifacts: () async => const [
          LocalReleaseArtifact(
            path: '/download/fixture.bin',
            name: _fixtureName,
            sha256: _fixtureHash,
          ),
        ],
        retryDelay: (_) async {},
        expectedSigningKeyId: _fixtureSigningKeyId,
      );

      final result = await service.verify(_tag);

      expect(result.status, ReleaseVerificationStatus.unavailable);
      expect(result.reason, ReleaseVerificationReason.downloadFailed);
      expect(result.localArtifactName, _fixtureName);
      expect(result.localHash, _fixtureHash);
    },
  );

  test('explains when the current network mode cannot reach GitHub', () async {
    final service = ReleaseVerificationService(
      downloadBytes: (_) async => throw Exception(
        "I2P transport refuses non-I2P URL destination 'github.com'",
      ),
      loadAsset: (_) async => _fixturePublicKey,
      loadLocalArtifacts: () async => const [],
      retryDelay: (_) async {},
      expectedSigningKeyId: _fixtureSigningKeyId,
    );

    final result = await service.verify(_tag);

    expect(result.status, ReleaseVerificationStatus.unavailable);
    expect(result.reason, ReleaseVerificationReason.networkModeUnsupported);
  });

  test('normalizes build metadata out of release tags', () {
    expect(
      ReleaseVerificationService.normalizeReleaseTag('1.1.9+10109'),
      'v1.1.9',
    );
    expect(
      ReleaseVerificationService.normalizeReleaseTag('v1.1.9-rc.1'),
      'v1.1.9-rc.1',
    );
  });
}
