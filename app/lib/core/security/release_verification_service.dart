import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:crypto/crypto.dart';
import 'package:dart_pg/dart_pg.dart';
import 'package:flutter/services.dart';

import '../ffi/ffi_bridge.dart';

enum ReleaseVerificationStatus {
  idle,
  checking,
  match,
  mismatch,
  unavailable,
  noRelease,
  noLocalArtifact,
  noMatchingChecksum,
  error,
}

enum ReleaseVerificationReason {
  none,
  releaseFilesUnavailable,
  downloadFailed,
  networkModeUnsupported,
  localArtifactUnavailable,
  checksumNotPublished,
  checksumMismatch,
  signatureInvalid,
  invalidVerificationFiles,
}

final class ReleaseVerificationResult {
  const ReleaseVerificationResult({
    required this.status,
    required this.reason,
    required this.releaseTag,
    required this.releaseUrl,
    required this.signatureAssetName,
    this.checksumAssetName,
    this.localArtifactPath,
    this.localArtifactName,
    this.localHash,
    this.expectedHash,
    this.matchedChecksumName,
  });

  final ReleaseVerificationStatus status;
  final ReleaseVerificationReason reason;
  final String releaseTag;
  final String releaseUrl;
  final String signatureAssetName;
  final String? checksumAssetName;
  final String? localArtifactPath;
  final String? localArtifactName;
  final String? localHash;
  final String? expectedHash;
  final String? matchedChecksumName;
}

final class LocalReleaseArtifact {
  const LocalReleaseArtifact({
    required this.path,
    required this.name,
    required this.sha256,
  });

  final String path;
  final String name;
  final String sha256;
}

typedef ReleaseBytesDownloader = Future<Uint8List> Function(String url);
typedef ReleaseAssetLoader = Future<String> Function(String key);
typedef LocalArtifactLoader = Future<List<LocalReleaseArtifact>> Function();
typedef ReleaseRetryDelay = Future<void> Function(Duration duration);

final class ReleaseVerificationService {
  ReleaseVerificationService({
    ReleaseBytesDownloader? downloadBytes,
    ReleaseAssetLoader? loadAsset,
    LocalArtifactLoader? loadLocalArtifacts,
    ReleaseRetryDelay? retryDelay,
    String? expectedSigningKeyId,
  }) : _downloadBytes = downloadBytes ?? _defaultDownloadBytes,
       _loadAsset = loadAsset ?? rootBundle.loadString,
       _loadLocalArtifacts = loadLocalArtifacts ?? _defaultLocalArtifacts,
       _retryDelay = retryDelay ?? _defaultRetryDelay,
       _expectedSigningKeyId =
           expectedSigningKeyId ?? _unifiedWalletSigningKeyId;

  static const repositoryUrl =
      'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet';
  static const _publicKeyAsset = 'assets/security/public_key.asc';
  static const _unifiedWalletSigningKeyId = '2ce65343401553a6';
  static const _maxBundleBytes = 8 * 1024 * 1024;
  static const _maxBundleFiles = 512;
  static const _maxBundleEntryBytes = 2 * 1024 * 1024;
  static const _downloadRetryDelays = <Duration>[
    Duration(milliseconds: 500),
    Duration(seconds: 2),
  ];

  final ReleaseBytesDownloader _downloadBytes;
  final ReleaseAssetLoader _loadAsset;
  final LocalArtifactLoader _loadLocalArtifacts;
  final ReleaseRetryDelay _retryDelay;
  final String _expectedSigningKeyId;

  Future<ReleaseVerificationResult> verify(
    String appVersion, {
    String embeddedReleaseTag = '',
  }) async {
    final tag = embeddedReleaseTag.trim().isEmpty
        ? normalizeReleaseTag(appVersion)
        : normalizeReleaseTag(embeddedReleaseTag);
    final releaseUrl = '$repositoryUrl/releases/tag/$tag';
    final signatureAssetName = 'signatures-$tag.zip';
    final signatureUrl =
        '$repositoryUrl/releases/download/$tag/$signatureAssetName';

    List<LocalReleaseArtifact> artifacts;
    try {
      artifacts = await _loadLocalArtifacts();
    } catch (_) {
      artifacts = const [];
    }

    Uint8List? bundleBytes;
    Object? downloadError;
    for (var attempt = 0; attempt <= _downloadRetryDelays.length; attempt++) {
      try {
        bundleBytes = await _downloadBytes(signatureUrl);
        break;
      } catch (error) {
        downloadError = error;
        if (_isReleaseUnavailable(error) ||
            _isUnsupportedNetworkMode(error) ||
            attempt == _downloadRetryDelays.length) {
          break;
        }
        await _retryDelay(_downloadRetryDelays[attempt]);
      }
    }

    if (bundleBytes == null) {
      final unavailable = _isReleaseUnavailable(downloadError);
      final unsupportedNetwork = _isUnsupportedNetworkMode(downloadError);
      final localArtifact = artifacts.isEmpty ? null : artifacts.first;
      return ReleaseVerificationResult(
        status: unavailable
            ? ReleaseVerificationStatus.noRelease
            : ReleaseVerificationStatus.unavailable,
        reason: unsupportedNetwork
            ? ReleaseVerificationReason.networkModeUnsupported
            : unavailable
            ? ReleaseVerificationReason.releaseFilesUnavailable
            : ReleaseVerificationReason.downloadFailed,
        releaseTag: tag,
        releaseUrl: releaseUrl,
        signatureAssetName: signatureAssetName,
        localArtifactPath: localArtifact?.path,
        localArtifactName: localArtifact?.name,
        localHash: localArtifact?.sha256,
      );
    }

    try {
      final files = _readSignatureBundle(bundleBytes);
      final checksumName = 'sha256sum-$tag.txt';
      final checksumBytes = _requiredFile(files, checksumName);
      final checksumSignature = _requiredFile(files, '$checksumName.sig');
      final publicKey = await _loadAsset(_publicKeyAsset);

      if (!_verifyDetached(checksumBytes, checksumSignature, publicKey)) {
        return _signatureFailure(
          tag: tag,
          releaseUrl: releaseUrl,
          signatureAssetName: signatureAssetName,
          checksumAssetName: checksumName,
        );
      }

      final packageChecksums = _parseChecksums(checksumBytes);
      if (artifacts.isEmpty) {
        return ReleaseVerificationResult(
          status: ReleaseVerificationStatus.noLocalArtifact,
          reason: ReleaseVerificationReason.localArtifactUnavailable,
          releaseTag: tag,
          releaseUrl: releaseUrl,
          signatureAssetName: signatureAssetName,
          checksumAssetName: checksumName,
        );
      }

      var trustedChecksums = packageChecksums;
      var trustedChecksumName = checksumName;
      LocalReleaseArtifact? artifact = _findMatchingArtifact(
        artifacts,
        trustedChecksums,
      );

      if (artifact == null) {
        final payloadName = 'build-payloads-$tag.txt';
        final payloadBytes = files[payloadName];
        final payloadSignature = files['$payloadName.sig'];
        if (payloadBytes != null && payloadSignature != null) {
          if (!_verifyDetached(payloadBytes, payloadSignature, publicKey)) {
            return _signatureFailure(
              tag: tag,
              releaseUrl: releaseUrl,
              signatureAssetName: signatureAssetName,
              checksumAssetName: payloadName,
            );
          }
          trustedChecksums = _parseChecksums(payloadBytes);
          trustedChecksumName = payloadName;
          artifact = _findMatchingArtifact(artifacts, trustedChecksums);
        }
      }

      if (artifact == null) {
        return ReleaseVerificationResult(
          status: ReleaseVerificationStatus.noMatchingChecksum,
          reason: ReleaseVerificationReason.checksumNotPublished,
          releaseTag: tag,
          releaseUrl: releaseUrl,
          signatureAssetName: signatureAssetName,
          checksumAssetName: trustedChecksumName,
          localArtifactPath: artifacts.first.path,
          localArtifactName: artifacts.first.name,
          localHash: artifacts.first.sha256,
        );
      }

      final matchedName = _lookupName(trustedChecksums, artifact.name)!;
      final expectedHash = trustedChecksums[matchedName]!;
      final matches =
          _normalizeHash(expectedHash) == _normalizeHash(artifact.sha256);
      return ReleaseVerificationResult(
        status: matches
            ? ReleaseVerificationStatus.match
            : ReleaseVerificationStatus.mismatch,
        reason: matches
            ? ReleaseVerificationReason.none
            : ReleaseVerificationReason.checksumMismatch,
        releaseTag: tag,
        releaseUrl: releaseUrl,
        signatureAssetName: signatureAssetName,
        checksumAssetName: trustedChecksumName,
        localArtifactPath: artifact.path,
        localArtifactName: artifact.name,
        localHash: artifact.sha256,
        expectedHash: expectedHash,
        matchedChecksumName: matchedName,
      );
    } catch (_) {
      return ReleaseVerificationResult(
        status: ReleaseVerificationStatus.error,
        reason: ReleaseVerificationReason.invalidVerificationFiles,
        releaseTag: tag,
        releaseUrl: releaseUrl,
        signatureAssetName: signatureAssetName,
      );
    }
  }

  static String normalizeReleaseTag(String version) {
    var normalized = version.trim();
    final buildSeparator = normalized.indexOf('+');
    if (buildSeparator >= 0) {
      normalized = normalized.substring(0, buildSeparator);
    }
    if (!normalized.startsWith('v')) {
      normalized = 'v$normalized';
    }
    return normalized;
  }

  static bool _isReleaseUnavailable(Object? error) {
    final detail = error.toString().toLowerCase();
    return detail.contains('404') || detail.contains('not found');
  }

  static bool _isUnsupportedNetworkMode(Object? error) {
    final detail = error.toString().toLowerCase();
    return detail.contains('i2p transport refuses non-i2p') ||
        detail.contains('i2p transport refuses non-i2p url');
  }

  static Future<Uint8List> _defaultDownloadBytes(String url) {
    return FfiBridge.fetchExternalBytes(url: url, userAgent: 'StashiWallet');
  }

  static Future<void> _defaultRetryDelay(Duration duration) {
    return Future<void>.delayed(duration);
  }

  static Future<List<LocalReleaseArtifact>> _defaultLocalArtifacts() async {
    if (Platform.isAndroid || Platform.isIOS) {
      return const [];
    }

    final paths = <String>[];
    if (Platform.isLinux) {
      final appImage = Platform.environment['APPIMAGE'];
      if (appImage != null && appImage.trim().isNotEmpty) {
        paths.add(appImage);
      }
    }
    paths.add(Platform.resolvedExecutable);

    final artifacts = <LocalReleaseArtifact>[];
    final seen = <String>{};
    for (final path in paths) {
      final file = File(path);
      if (!file.existsSync()) continue;
      final absolute = file.absolute.path;
      if (!seen.add(absolute)) continue;
      final digest = await sha256.bind(file.openRead()).first;
      artifacts.add(
        LocalReleaseArtifact(
          path: absolute,
          name: _basename(absolute),
          sha256: digest.toString(),
        ),
      );
    }
    return artifacts;
  }

  static ReleaseVerificationResult _signatureFailure({
    required String tag,
    required String releaseUrl,
    required String signatureAssetName,
    required String checksumAssetName,
  }) {
    return ReleaseVerificationResult(
      status: ReleaseVerificationStatus.mismatch,
      reason: ReleaseVerificationReason.signatureInvalid,
      releaseTag: tag,
      releaseUrl: releaseUrl,
      signatureAssetName: signatureAssetName,
      checksumAssetName: checksumAssetName,
    );
  }

  static Uint8List _requiredFile(Map<String, Uint8List> files, String name) {
    final value = files[name];
    if (value == null) {
      throw FormatException('Missing $name');
    }
    return value;
  }

  static Map<String, Uint8List> _readSignatureBundle(Uint8List bytes) {
    if (bytes.length > _maxBundleBytes) {
      throw const FormatException('Signature bundle is too large');
    }

    final archive = ZipDecoder().decodeBytes(bytes);
    if (archive.files.length > _maxBundleFiles) {
      throw const FormatException('Signature bundle contains too many files');
    }

    final files = <String, Uint8List>{};
    var totalSize = 0;
    for (final file in archive.files) {
      if (!file.isFile) continue;
      final name = _basename(file.name);
      if (name.isEmpty || file.name != name || files.containsKey(name)) {
        throw const FormatException('Invalid signature bundle layout');
      }
      if (file.size < 0 || file.size > _maxBundleEntryBytes) {
        throw const FormatException('Signature bundle entry is too large');
      }
      totalSize += file.size;
      if (totalSize > _maxBundleBytes) {
        throw const FormatException('Expanded signature bundle is too large');
      }
      final content = file.readBytes();
      if (content == null || content.length != file.size) {
        throw const FormatException('Invalid signature bundle entry');
      }
      files[name] = content;
    }
    return files;
  }

  bool _verifyDetached(
    Uint8List content,
    Uint8List signature,
    String armoredPublicKey,
  ) {
    try {
      final key = OpenPGP.readPublicKey(armoredPublicKey);
      final detached = OpenPGP.readSignature(_armorSignature(signature));
      if (_hex(key.keyID) == _expectedSigningKeyId) {
        for (final packet in detached.packets) {
          if (_hex(packet.issuerKeyID) == _expectedSigningKeyId &&
              packet.verify(key.keyPacket, content)) {
            return true;
          }
        }
      }
      for (final subkey in key.subkeys) {
        if (_hex(subkey.keyID) != _expectedSigningKeyId ||
            !subkey.isSigningKey) {
          continue;
        }
        for (final packet in detached.packets) {
          if (_hex(packet.issuerKeyID) == _expectedSigningKeyId &&
              packet.verify(subkey.keyPacket, content)) {
            return true;
          }
        }
      }
    } catch (_) {
      return false;
    }
    return false;
  }

  static String _armorSignature(Uint8List signature) {
    final encoded = base64.encode(signature);
    final lines = <String>[];
    for (var offset = 0; offset < encoded.length; offset += 64) {
      final end = (offset + 64).clamp(0, encoded.length);
      lines.add(encoded.substring(offset, end));
    }
    final checksum = _crc24(signature);
    final checksumBytes = Uint8List.fromList([
      (checksum >> 16) & 0xff,
      (checksum >> 8) & 0xff,
      checksum & 0xff,
    ]);
    return '-----BEGIN PGP SIGNATURE-----\n\n'
        '${lines.join('\n')}\n'
        '=${base64.encode(checksumBytes)}\n'
        '-----END PGP SIGNATURE-----\n';
  }

  static int _crc24(Uint8List bytes) {
    var crc = 0xb704ce;
    for (final byte in bytes) {
      crc ^= byte << 16;
      for (var i = 0; i < 8; i++) {
        crc <<= 1;
        if ((crc & 0x1000000) != 0) {
          crc ^= 0x1864cfb;
        }
      }
    }
    return crc & 0xffffff;
  }

  static Map<String, String> _parseChecksums(Uint8List bytes) {
    final entries = <String, String>{};
    final pattern = RegExp(r'^([0-9a-fA-F]{64})[ \t]+[*]?(.+)$');
    for (final rawLine in LineSplitter.split(utf8.decode(bytes))) {
      final line = rawLine.trim();
      if (line.isEmpty || line.startsWith('#')) continue;
      final match = pattern.firstMatch(line);
      if (match == null) {
        throw FormatException('Invalid checksum line');
      }
      final name = _basename(match.group(2)!.trim());
      final hash = _normalizeHash(match.group(1)!);
      final previous = entries[name];
      if (previous != null && previous != hash) {
        throw FormatException('Conflicting checksum for $name');
      }
      entries[name] = hash;
    }
    return entries;
  }

  static LocalReleaseArtifact? _findMatchingArtifact(
    List<LocalReleaseArtifact> artifacts,
    Map<String, String> checksums,
  ) {
    for (final artifact in artifacts) {
      if (_lookupName(checksums, artifact.name) != null) {
        return artifact;
      }
    }
    return null;
  }

  static String? _lookupName(Map<String, String> checksums, String localName) {
    final canonical = _basename(localName).toLowerCase();
    for (final name in checksums.keys) {
      if (_basename(name).toLowerCase() == canonical) return name;
    }
    return null;
  }

  static String _basename(String value) {
    return value.replaceAll(r'\', '/').split('/').last;
  }

  static String _normalizeHash(String value) => value.trim().toLowerCase();

  static String _hex(Uint8List bytes) {
    return bytes.map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();
  }
}
