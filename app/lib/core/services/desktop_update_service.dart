import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:package_info_plus/package_info_plus.dart';

class DesktopReleaseAsset {
  const DesktopReleaseAsset({required this.name, required this.downloadUrl});

  final String name;
  final String downloadUrl;
}

class DesktopReleaseInfo {
  const DesktopReleaseInfo({
    required this.tagName,
    required this.name,
    required this.releaseUrl,
    required this.publishedAt,
    required this.isDraft,
    required this.isPrerelease,
    required this.assets,
  });

  final String tagName;
  final String name;
  final String releaseUrl;
  final DateTime? publishedAt;
  final bool isDraft;
  final bool isPrerelease;
  final List<DesktopReleaseAsset> assets;
}

enum DesktopUpdateAssetKind {
  windowsInstaller,
  windowsPortableZip,
  macDmg,
  linuxAppImage,
}

class DesktopUpdateCandidate {
  const DesktopUpdateCandidate({
    required this.currentVersion,
    required this.release,
    required this.asset,
    required this.assetKind,
  });

  final String currentVersion;
  final DesktopReleaseInfo release;
  final DesktopReleaseAsset asset;
  final DesktopUpdateAssetKind assetKind;
}

/// Checks GitHub releases for desktop updates and can launch updater scripts.
class DesktopUpdateService {
  DesktopUpdateService._();

  static final DesktopUpdateService instance = DesktopUpdateService._();

  static const String _releaseApiUrl =
      'https://api.github.com/repos/PirateNetwork/Pirate-Unified-Light-Wallet/releases';
  static const Duration minimumReleaseAge = Duration(hours: 1);
  static const Duration _networkTimeout = Duration(seconds: 25);

  bool get isDesktop =>
      !kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS);

  Future<String> currentAppVersion() async {
    final info = await PackageInfo.fromPlatform();
    final raw = info.version.trim();
    return raw.isEmpty ? '0.0.0' : raw;
  }

  Future<DesktopUpdateCandidate?> checkForCurrentVersionUpdate() async {
    final version = await currentAppVersion();
    return checkForUpdate(currentVersion: version);
  }

  Future<DesktopUpdateCandidate?> checkForUpdate({
    required String currentVersion,
  }) async {
    if (!isDesktop) {
      return null;
    }

    final release = await _fetchLatestEligibleRelease();
    if (release == null) {
      return null;
    }

    if (_compareVersions(release.tagName, currentVersion) <= 0) {
      return null;
    }

    final selection = _selectBestAsset(release.assets);
    if (selection == null) {
      return null;
    }

    return DesktopUpdateCandidate(
      currentVersion: currentVersion,
      release: release,
      asset: selection.asset,
      assetKind: selection.kind,
    );
  }

  Future<void> launchUpdate(DesktopUpdateCandidate candidate) async {
    final tempDir = await Directory.systemTemp.createTemp(
      'pirate_wallet_update_',
    );
    final downloadFile = File(
      '${tempDir.path}${Platform.pathSeparator}${candidate.asset.name}',
    );
    await _downloadToFile(candidate.asset.downloadUrl, downloadFile);

    switch (candidate.assetKind) {
      case DesktopUpdateAssetKind.windowsInstaller:
        await Process.start(
          downloadFile.path,
          const <String>[],
          mode: ProcessStartMode.detached,
        );
      case DesktopUpdateAssetKind.windowsPortableZip:
        await _launchWindowsZipUpdater(downloadFile.path);
      case DesktopUpdateAssetKind.linuxAppImage:
        await _launchLinuxAppImageUpdater(downloadFile.path);
      case DesktopUpdateAssetKind.macDmg:
        await _launchMacDmgUpdater(downloadFile.path);
    }
  }

  Future<DesktopReleaseInfo?> _fetchLatestEligibleRelease() async {
    final releases = await _fetchReleases();
    final now = DateTime.now().toUtc();
    for (final release in releases) {
      if (release.isDraft || release.isPrerelease) {
        continue;
      }
      final publishedAt = release.publishedAt?.toUtc();
      if (publishedAt == null) {
        continue;
      }
      if (now.difference(publishedAt) < minimumReleaseAge) {
        continue;
      }
      return release;
    }
    return null;
  }

  Future<List<DesktopReleaseInfo>> _fetchReleases() async {
    final client = HttpClient()..connectionTimeout = _networkTimeout;
    try {
      final request = await client.getUrl(Uri.parse(_releaseApiUrl));
      request.headers.set('Accept', 'application/vnd.github+json');
      request.headers.set('User-Agent', 'PirateWallet-DesktopUpdater');
      final response = await request.close().timeout(_networkTimeout);
      if (response.statusCode >= 400) {
        throw Exception('GitHub API returned ${response.statusCode}');
      }

      final body = await response.transform(utf8.decoder).join();
      final json = jsonDecode(body);
      if (json is! List) {
        throw Exception('Unexpected releases payload');
      }

      final releases = <DesktopReleaseInfo>[];
      for (final entry in json.whereType<Map<String, dynamic>>()) {
        final assets = <DesktopReleaseAsset>[];
        final rawAssets = entry['assets'];
        if (rawAssets is List) {
          for (final asset in rawAssets.whereType<Map<String, dynamic>>()) {
            final name = asset['name']?.toString() ?? '';
            final url = asset['browser_download_url']?.toString() ?? '';
            if (name.isEmpty || url.isEmpty) {
              continue;
            }
            assets.add(DesktopReleaseAsset(name: name, downloadUrl: url));
          }
        }

        final publishedRaw = entry['published_at']?.toString();
        releases.add(
          DesktopReleaseInfo(
            tagName: entry['tag_name']?.toString() ?? '',
            name: entry['name']?.toString() ?? '',
            releaseUrl: entry['html_url']?.toString() ?? '',
            publishedAt: publishedRaw == null || publishedRaw.isEmpty
                ? null
                : DateTime.tryParse(publishedRaw),
            isDraft: entry['draft'] == true,
            isPrerelease: entry['prerelease'] == true,
            assets: assets,
          ),
        );
      }
      return releases;
    } finally {
      client.close(force: true);
    }
  }

  ({DesktopReleaseAsset asset, DesktopUpdateAssetKind kind})? _selectBestAsset(
    List<DesktopReleaseAsset> assets,
  ) {
    if (assets.isEmpty) {
      return null;
    }

    DesktopReleaseAsset? prefer(
      bool Function(DesktopReleaseAsset asset) predicate,
    ) {
      for (final asset in assets) {
        if (predicate(asset) &&
            !asset.name.toLowerCase().contains('unsigned')) {
          return asset;
        }
      }
      for (final asset in assets) {
        if (predicate(asset)) {
          return asset;
        }
      }
      return null;
    }

    if (Platform.isWindows) {
      final installer = prefer(
        (asset) =>
            asset.name.toLowerCase().endsWith('.exe') &&
            asset.name.toLowerCase().contains('windows') &&
            asset.name.toLowerCase().contains('installer'),
      );
      if (installer != null) {
        return (
          asset: installer,
          kind: DesktopUpdateAssetKind.windowsInstaller,
        );
      }
      final portable = prefer(
        (asset) =>
            asset.name.toLowerCase().endsWith('.zip') &&
            asset.name.toLowerCase().contains('windows') &&
            asset.name.toLowerCase().contains('portable'),
      );
      if (portable != null) {
        return (
          asset: portable,
          kind: DesktopUpdateAssetKind.windowsPortableZip,
        );
      }
      final anyExe = prefer(
        (asset) => asset.name.toLowerCase().endsWith('.exe'),
      );
      if (anyExe != null) {
        return (asset: anyExe, kind: DesktopUpdateAssetKind.windowsInstaller);
      }
      return null;
    }

    if (Platform.isLinux) {
      final appImage = prefer(
        (asset) =>
            asset.name.toLowerCase().endsWith('.appimage') &&
            asset.name.toLowerCase().contains('linux'),
      );
      if (appImage != null) {
        return (asset: appImage, kind: DesktopUpdateAssetKind.linuxAppImage);
      }
      return null;
    }

    if (Platform.isMacOS) {
      final dmg = prefer(
        (asset) =>
            asset.name.toLowerCase().endsWith('.dmg') &&
            asset.name.toLowerCase().contains('macos'),
      );
      if (dmg != null) {
        return (asset: dmg, kind: DesktopUpdateAssetKind.macDmg);
      }
      return null;
    }

    return null;
  }

  Future<void> _downloadToFile(String url, File destination) async {
    final client = HttpClient()..connectionTimeout = _networkTimeout;
    try {
      final request = await client.getUrl(Uri.parse(url));
      request.headers.set('User-Agent', 'PirateWallet-DesktopUpdater');
      final response = await request.close().timeout(_networkTimeout);
      if (response.statusCode >= 400) {
        throw Exception('Download failed with status ${response.statusCode}');
      }
      await destination.parent.create(recursive: true);
      final sink = destination.openWrite(mode: FileMode.writeOnly);
      await response.pipe(sink);
      await sink.flush();
      await sink.close();
    } finally {
      client.close(force: true);
    }
  }

  Future<void> _launchWindowsZipUpdater(String zipPath) async {
    final currentExe = Platform.resolvedExecutable;
    final appDir = File(currentExe).parent.path;
    final exeName = currentExe.split(Platform.pathSeparator).last;
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update.cmd',
    );
    await script.writeAsString('''
@echo off
setlocal
set "ZIP_PATH=$zipPath"
set "APP_DIR=$appDir"
set "EXE_NAME=$exeName"
set "STAGE_DIR=%TEMP%\\pirate_wallet_update_%RANDOM%%RANDOM%"
timeout /t 2 /nobreak >nul
powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -LiteralPath '%ZIP_PATH%' -DestinationPath '%STAGE_DIR%' -Force"
robocopy "%STAGE_DIR%" "%APP_DIR%" /E /R:2 /W:1 /NFL /NDL /NJH /NJS /NP >nul
start "" "%APP_DIR%\\%EXE_NAME%"
rmdir /S /Q "%STAGE_DIR%" >nul 2>&1
del "%ZIP_PATH%" >nul 2>&1
del "%~f0" >nul 2>&1
''', flush: true);
    await Process.start('cmd.exe', <String>[
      '/c',
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchLinuxAppImageUpdater(String appImagePath) async {
    final currentExe = Platform.resolvedExecutable;
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update.sh',
    );
    await script.writeAsString('''
#!/usr/bin/env bash
set -euo pipefail
sleep 2
cp "$appImagePath" "$currentExe.new"
chmod +x "$currentExe.new"
mv "$currentExe.new" "$currentExe"
"$currentExe" >/dev/null 2>&1 &
''', flush: true);
    await Process.run('chmod', <String>['+x', script.path]);
    await Process.start('bash', <String>[
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchMacDmgUpdater(String dmgPath) async {
    final currentExe = File(Platform.resolvedExecutable);
    final appBundle = _resolveMacAppBundlePath(currentExe.path);
    final appName = appBundle.split('/').last;
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update_macos.sh',
    );
    await script.writeAsString('''
#!/usr/bin/env bash
set -euo pipefail
sleep 2
DMG_PATH="$dmgPath"
CURRENT_APP="$appBundle"
TARGET_APP="\$CURRENT_APP"
if [ ! -w "\$(dirname "\$TARGET_APP")" ]; then
  TARGET_APP="/Applications/$appName"
fi

MOUNT_POINT=\$(hdiutil attach "\$DMG_PATH" -nobrowse -readonly | awk '/\\/Volumes\\// {print \$3; exit}')
if [ -z "\$MOUNT_POINT" ]; then
  open "\$DMG_PATH"
  exit 0
fi
SOURCE_APP=\$(find "\$MOUNT_POINT" -maxdepth 1 -name "*.app" -print -quit)
if [ -z "\$SOURCE_APP" ]; then
  hdiutil detach "\$MOUNT_POINT" || true
  open "\$DMG_PATH"
  exit 0
fi
rm -rf "\$TARGET_APP"
cp -R "\$SOURCE_APP" "\$TARGET_APP"
hdiutil detach "\$MOUNT_POINT" || true
open "\$TARGET_APP"
''', flush: true);
    await Process.run('chmod', <String>['+x', script.path]);
    await Process.start('bash', <String>[
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  String _resolveMacAppBundlePath(String executablePath) {
    final normalized = executablePath.replaceAll(String.fromCharCode(92), '/');
    const marker = '.app/Contents/MacOS/';
    final index = normalized.indexOf(marker);
    if (index == -1) {
      return File(executablePath).parent.parent.parent.path;
    }
    return normalized.substring(0, index + '.app'.length);
  }

  int _compareVersions(String left, String right) {
    final a = _SimpleSemver.parse(left);
    final b = _SimpleSemver.parse(right);
    return a.compareTo(b);
  }
}

class _SimpleSemver implements Comparable<_SimpleSemver> {
  const _SimpleSemver({
    required this.major,
    required this.minor,
    required this.patch,
  });

  final int major;
  final int minor;
  final int patch;

  factory _SimpleSemver.parse(String raw) {
    final normalized = raw
        .trim()
        .toLowerCase()
        .replaceFirst(RegExp('^v'), '')
        .split('+')
        .first
        .split('-')
        .first;
    final parts = normalized.split('.');
    int parsePart(int index) {
      if (index >= parts.length) {
        return 0;
      }
      return int.tryParse(parts[index]) ?? 0;
    }

    return _SimpleSemver(
      major: parsePart(0),
      minor: parsePart(1),
      patch: parsePart(2),
    );
  }

  @override
  int compareTo(_SimpleSemver other) {
    final majorCmp = major.compareTo(other.major);
    if (majorCmp != 0) {
      return majorCmp;
    }
    final minorCmp = minor.compareTo(other.minor);
    if (minorCmp != 0) {
      return minorCmp;
    }
    return patch.compareTo(other.patch);
  }
}
