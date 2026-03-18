import 'dart:convert';
import 'dart:io';
import 'package:archive/archive.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:package_info_plus/package_info_plus.dart';

part 'desktop_update_service_asset_selection.dart';

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
  linuxDeb,
  linuxFlatpak,
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

class DesktopUpdateLaunchResult {
  const DesktopUpdateLaunchResult({required this.shouldCloseApp});

  final bool shouldCloseApp;
}

class _ChecksumResult {
  const _ChecksumResult({required this.entries, required this.sourceName});

  final Map<String, String> entries;
  final String? sourceName;
}

enum _LinuxInstallMode { appImage, flatpak, systemPackage, unknown }

/// Checks GitHub releases for desktop updates and can launch updater scripts.
class DesktopUpdateService {
  DesktopUpdateService._();

  static final DesktopUpdateService instance = DesktopUpdateService._();
  static final _DesktopUpdateAssetSelectionHelper _assetSelection =
      _DesktopUpdateAssetSelectionHelper();

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

    final selection = _assetSelection.selectBestAsset(release.assets);
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

  Future<DesktopUpdateLaunchResult> launchUpdate(
    DesktopUpdateCandidate candidate,
  ) async {
    final tempDir = await Directory.systemTemp.createTemp(
      'pirate_wallet_update_',
    );
    final downloadFile = File(
      '${tempDir.path}${Platform.pathSeparator}${candidate.asset.name}',
    );
    await _downloadToFile(candidate.asset.downloadUrl, downloadFile);
    await _verifyDownloadedAsset(candidate, downloadFile);

    switch (candidate.assetKind) {
      case DesktopUpdateAssetKind.windowsInstaller:
        await Process.start(
          downloadFile.path,
          const <String>[],
          mode: ProcessStartMode.detached,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
      case DesktopUpdateAssetKind.windowsPortableZip:
        await _launchWindowsZipUpdater(
          downloadFile.path,
          candidate.release.releaseUrl,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
      case DesktopUpdateAssetKind.linuxAppImage:
        await _launchLinuxAppImageUpdater(
          downloadFile.path,
          candidate.release.releaseUrl,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
      case DesktopUpdateAssetKind.linuxDeb:
        await _launchLinuxDebUpdater(
          downloadFile.path,
          candidate.release.releaseUrl,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
      case DesktopUpdateAssetKind.linuxFlatpak:
        await _launchLinuxFlatpakUpdater(
          downloadFile.path,
          candidate.release.releaseUrl,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
      case DesktopUpdateAssetKind.macDmg:
        await _launchMacDmgUpdater(
          downloadFile.path,
          candidate.release.releaseUrl,
        );
        return const DesktopUpdateLaunchResult(shouldCloseApp: false);
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

  Future<void> _verifyDownloadedAsset(
    DesktopUpdateCandidate candidate,
    File destination,
  ) async {
    final checksums = await _fetchChecksums(candidate.release.assets);
    if (checksums.entries.isEmpty) {
      throw Exception('No published checksums were found for this release.');
    }

    final expected = _lookupChecksum(checksums.entries, candidate.asset.name);
    if (expected == null) {
      throw Exception(
        'No published checksum was found for ${candidate.asset.name}.',
      );
    }

    final actual = await _hashFile(destination);
    if (_normalizeHash(actual) != _normalizeHash(expected)) {
      throw Exception(
        'Checksum verification failed for ${candidate.asset.name}.',
      );
    }

    if (candidate.assetKind == DesktopUpdateAssetKind.windowsInstaller &&
        !_assetSelection.isUnsignedAsset(candidate.asset.name)) {
      await _verifyWindowsAuthenticode(destination.path);
    }
  }

  Future<void> _verifyWindowsAuthenticode(String path) async {
    if (!Platform.isWindows) {
      return;
    }

    final escapedPath = path.replaceAll("'", "''");
    final command =
        r'''$sig = Get-AuthenticodeSignature -LiteralPath '__PATH__'; '''
                r'''Write-Output $sig.Status; '''
                r'''if ($sig.Status -ne "Valid") { exit 1 }'''
            .replaceAll('__PATH__', escapedPath);
    final result = await Process.run('powershell.exe', <String>[
      '-NoProfile',
      '-ExecutionPolicy',
      'Bypass',
      '-Command',
      command,
    ]);

    final status = result.stdout.toString().trim();
    if (result.exitCode != 0) {
      final details = status.isNotEmpty
          ? status
          : result.stderr.toString().trim();
      throw Exception('Windows signature verification failed: $details');
    }
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

  Future<Uint8List> _downloadBytes(String url) async {
    final client = HttpClient()..connectionTimeout = _networkTimeout;
    try {
      final request = await client.getUrl(Uri.parse(url));
      request.headers.set('Accept', 'application/vnd.github+json');
      request.headers.set('User-Agent', 'PirateWallet-DesktopUpdater');
      final response = await request.close().timeout(_networkTimeout);
      if (response.statusCode >= 400) {
        throw Exception('Download failed with status ${response.statusCode}');
      }
      final bytes = await consolidateHttpClientResponseBytes(response);
      return bytes;
    } finally {
      client.close(force: true);
    }
  }

  Future<String> _downloadText(String url) async {
    final bytes = await _downloadBytes(url);
    return utf8.decode(bytes, allowMalformed: true);
  }

  Future<String> _hashFile(File file) async {
    final digest = await sha256.bind(file.openRead()).first;
    return digest.toString();
  }

  Future<_ChecksumResult> _fetchChecksums(
    List<DesktopReleaseAsset> assets,
  ) async {
    final entries = <String, String>{};
    String? sourceName;

    final checksumAssets = assets.where(
      (asset) => _isDirectChecksumAsset(asset.name),
    );
    for (final asset in checksumAssets) {
      final text = await _downloadText(asset.downloadUrl);
      final parsed = _parseChecksums(text, asset.name);
      if (parsed.isNotEmpty) {
        entries.addAll(parsed);
        sourceName ??= asset.name;
      }
    }

    final checksumBundles = assets.where(
      (asset) => _isChecksumBundleAsset(asset.name),
    );
    for (final asset in checksumBundles) {
      final bundleBytes = await _downloadBytes(asset.downloadUrl);
      final parsed = _parseChecksumsFromZip(bundleBytes);
      if (parsed.isNotEmpty) {
        entries.addAll(parsed);
        sourceName ??= asset.name;
      }
    }

    return _ChecksumResult(entries: entries, sourceName: sourceName);
  }

  bool _isDirectChecksumAsset(String name) {
    final lower = name.toLowerCase();
    return lower.endsWith('.sha256') ||
        lower.endsWith('.sha256sum') ||
        lower.contains('checksums');
  }

  bool _isChecksumBundleAsset(String name) {
    final lower = name.toLowerCase();
    return lower.endsWith('.zip') &&
        (lower.contains('metadata') ||
            lower.contains('checksum') ||
            lower.contains('sha256'));
  }

  Map<String, String> _parseChecksumsFromZip(Uint8List bytes) {
    final map = <String, String>{};
    Archive archive;
    try {
      archive = ZipDecoder().decodeBytes(bytes);
    } catch (_) {
      return map;
    }

    for (final file in archive.files) {
      if (!file.isFile || !_isDirectChecksumAsset(file.name)) {
        continue;
      }

      try {
        final text = utf8.decode(
          file.content as List<int>,
          allowMalformed: true,
        );
        map.addAll(_parseChecksums(text, file.name));
      } catch (_) {
        // Ignore malformed entries and keep parsing the rest.
      }
    }
    return map;
  }

  Map<String, String> _parseChecksums(String text, String assetName) {
    final map = <String, String>{};
    final lines = LineSplitter.split(text);
    for (final line in lines) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || trimmed.startsWith('#')) {
        continue;
      }

      final parts = trimmed.split(RegExp(r'\s+'));
      if (parts.length == 1) {
        final fallbackName = assetName.replaceAll(
          RegExp(r'\.sha256(sum)?$', caseSensitive: false),
          '',
        );
        map[_canonicalAssetName(fallbackName)] = parts.first;
        continue;
      }

      final hash = parts.first.trim();
      final filename = parts.sublist(1).join(' ').replaceFirst('*', '').trim();
      if (hash.isNotEmpty && filename.isNotEmpty) {
        map[_canonicalAssetName(filename)] = hash;
      }
    }
    return map;
  }

  String? _lookupChecksum(Map<String, String> checksums, String assetName) {
    final canonicalName = _canonicalAssetName(assetName);
    for (final entry in checksums.entries) {
      if (_canonicalAssetName(entry.key) == canonicalName) {
        return entry.value;
      }
    }
    return null;
  }

  String _canonicalAssetName(String value) {
    final normalizedPath = value
        .replaceAll(String.fromCharCode(92), '/')
        .trim();
    return normalizedPath.split('/').last.toLowerCase();
  }

  String _normalizeHash(String value) => value.trim().toLowerCase();

  String _shQuote(String value) {
    return "'${value.replaceAll("'", "'\"'\"'")}'";
  }

  Future<void> _launchWindowsZipUpdater(
    String zipPath,
    String releaseUrl,
  ) async {
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
set "RELEASE_URL=$releaseUrl"
set "STAGE_DIR=%TEMP%\\pirate_wallet_update_%RANDOM%%RANDOM%"
timeout /t 2 /nobreak >nul
powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -LiteralPath '%ZIP_PATH%' -DestinationPath '%STAGE_DIR%' -Force"
if errorlevel 1 goto fail
robocopy "%STAGE_DIR%" "%APP_DIR%" /E /R:2 /W:1 /NFL /NDL /NJH /NJS /NP >nul
set "ROBOCOPY_EXIT=%ERRORLEVEL%"
if %ROBOCOPY_EXIT% GEQ 8 goto fail
if not exist "%APP_DIR%\\%EXE_NAME%" goto fail
start "" "%APP_DIR%\\%EXE_NAME%"
rmdir /S /Q "%STAGE_DIR%" >nul 2>&1
del "%ZIP_PATH%" >nul 2>&1
del "%~f0" >nul 2>&1
exit /b 0
:fail
start "" "%RELEASE_URL%"
exit /b 0
''', flush: true);
    await Process.start('cmd.exe', <String>[
      '/c',
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchLinuxAppImageUpdater(
    String appImagePath,
    String releaseUrl,
  ) async {
    final currentAppImage = _assetSelection.resolveLinuxAppImagePath();
    if (currentAppImage == null) {
      throw Exception(
        'Unable to determine the current AppImage path for in-place update.',
      );
    }

    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update_linux_appimage.sh',
    );
    final scriptContents =
        r'''
#!/usr/bin/env bash
set -euo pipefail
NEW_APP=__NEW_APP__
CURRENT_APP=__CURRENT_APP__
RELEASE_URL=__RELEASE_URL__
BACKUP_APP=__BACKUP_APP__
notify_fail() {
  if [ -f "$BACKUP_APP" ] && [ ! -f "$CURRENT_APP" ]; then
    mv -f "$BACKUP_APP" "$CURRENT_APP" >/dev/null 2>&1 || true
  fi
  if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$RELEASE_URL" >/dev/null 2>&1 || true
  fi
  exit 0
}
sleep 2
cp "$CURRENT_APP" "$BACKUP_APP" || notify_fail
cp "$NEW_APP" "$CURRENT_APP.new" || notify_fail
chmod +x "$CURRENT_APP.new" || notify_fail
mv -f "$CURRENT_APP.new" "$CURRENT_APP" || notify_fail
"$CURRENT_APP" >/dev/null 2>&1 &
rm -f "$BACKUP_APP" "$NEW_APP" "$0" >/dev/null 2>&1 || true
'''
            .replaceAll('__NEW_APP__', _shQuote(appImagePath))
            .replaceAll('__CURRENT_APP__', _shQuote(currentAppImage))
            .replaceAll('__RELEASE_URL__', _shQuote(releaseUrl))
            .replaceAll('__BACKUP_APP__', _shQuote('$currentAppImage.backup'));
    await script.writeAsString(scriptContents, flush: true);
    await Process.run('chmod', <String>['+x', script.path]);
    await Process.start('bash', <String>[
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchLinuxDebUpdater(String debPath, String releaseUrl) async {
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update_linux_deb.sh',
    );
    final scriptContents =
        r'''
#!/usr/bin/env bash
set -euo pipefail
DEB_PATH=__DEB_PATH__
RELEASE_URL=__RELEASE_URL__
sleep 2
if command -v pkcon >/dev/null 2>&1; then
  pkcon install-local -y "$DEB_PATH" && exit 0
fi
if command -v pkexec >/dev/null 2>&1 && command -v apt >/dev/null 2>&1; then
  pkexec apt install -y "$DEB_PATH" && exit 0
fi
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$DEB_PATH" >/dev/null 2>&1 && exit 0
  xdg-open "$RELEASE_URL" >/dev/null 2>&1 || true
fi
exit 0
'''
            .replaceAll('__DEB_PATH__', _shQuote(debPath))
            .replaceAll('__RELEASE_URL__', _shQuote(releaseUrl));
    await script.writeAsString(scriptContents, flush: true);
    await Process.run('chmod', <String>['+x', script.path]);
    await Process.start('bash', <String>[
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchLinuxFlatpakUpdater(
    String flatpakPath,
    String releaseUrl,
  ) async {
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update_linux_flatpak.sh',
    );
    final scriptContents =
        r'''
#!/usr/bin/env bash
set -euo pipefail
FLATPAK_PATH=__FLATPAK_PATH__
RELEASE_URL=__RELEASE_URL__
sleep 2
if [ -n "${FLATPAK_ID:-}" ] && command -v flatpak-spawn >/dev/null 2>&1; then
  flatpak-spawn --host flatpak install --user -y "$FLATPAK_PATH" && exit 0
fi
if command -v flatpak >/dev/null 2>&1; then
  flatpak install --user -y "$FLATPAK_PATH" && exit 0
fi
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$FLATPAK_PATH" >/dev/null 2>&1 && exit 0
  xdg-open "$RELEASE_URL" >/dev/null 2>&1 || true
fi
exit 0
'''
            .replaceAll('__FLATPAK_PATH__', _shQuote(flatpakPath))
            .replaceAll('__RELEASE_URL__', _shQuote(releaseUrl));
    await script.writeAsString(scriptContents, flush: true);
    await Process.run('chmod', <String>['+x', script.path]);
    await Process.start('bash', <String>[
      script.path,
    ], mode: ProcessStartMode.detached);
  }

  Future<void> _launchMacDmgUpdater(String dmgPath, String releaseUrl) async {
    final currentExe = File(Platform.resolvedExecutable);
    final appBundle = _resolveMacAppBundlePath(currentExe.path);
    final appName = appBundle.split('/').last;
    final script = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}pirate_wallet_update_macos.sh',
    );
    final scriptContents =
        r'''
#!/usr/bin/env bash
set -euo pipefail
DMG_PATH=__DMG_PATH__
CURRENT_APP=__CURRENT_APP__
APP_NAME=__APP_NAME__
RELEASE_URL=__RELEASE_URL__
show_manual() {
  /usr/bin/osascript -e 'display dialog "Automatic update failed. Open the release page to download and install manually." buttons {"OK"} default button "OK"' >/dev/null 2>&1 || true
  open "$RELEASE_URL" >/dev/null 2>&1 || true
}
verify_bundle() {
  codesign --verify --deep --strict --verbose=2 "$1" >/dev/null 2>&1 && spctl --assess --type execute -v "$1" >/dev/null 2>&1
}
do_install() {
  local source_app="$1"
  local target_app="$2"
  local backup_app="${2}.backup"
  rm -rf "$backup_app"
  if [ -e "$target_app" ]; then
    mv "$target_app" "$backup_app" || return 1
  fi
  if ! cp -R "$source_app" "$target_app"; then
    rm -rf "$target_app"
    if [ -e "$backup_app" ]; then
      mv "$backup_app" "$target_app" >/dev/null 2>&1 || true
    fi
    return 1
  fi
  if ! verify_bundle "$target_app"; then
    rm -rf "$target_app"
    if [ -e "$backup_app" ]; then
      mv "$backup_app" "$target_app" >/dev/null 2>&1 || true
    fi
    return 1
  fi
  rm -rf "$backup_app"
  open "$target_app" >/dev/null 2>&1 || true
  return 0
}
if [ "${1:-}" = "--install" ]; then
  do_install "$2" "$3"
  exit $?
fi
sleep 2
TARGET_APP="$CURRENT_APP"
if [ ! -w "$(dirname "$TARGET_APP")" ]; then
  TARGET_APP="/Applications/$APP_NAME"
fi
MOUNT_POINT=$(hdiutil attach "$DMG_PATH" -nobrowse -readonly | awk '/\/Volumes\// {print $3; exit}')
if [ -z "$MOUNT_POINT" ]; then
  open "$DMG_PATH" >/dev/null 2>&1 || true
  show_manual
  exit 0
fi
SOURCE_APP=$(find "$MOUNT_POINT" -maxdepth 1 -name "*.app" -print -quit)
if [ -z "$SOURCE_APP" ]; then
  hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  open "$DMG_PATH" >/dev/null 2>&1 || true
  show_manual
  exit 0
fi
if ! verify_bundle "$SOURCE_APP"; then
  hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  show_manual
  exit 0
fi
if [ -w "$(dirname "$TARGET_APP")" ]; then
  if ! "$0" --install "$SOURCE_APP" "$TARGET_APP"; then
    hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
    show_manual
    exit 0
  fi
  hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  exit 0
fi
INSTALL_CMD=$(printf '%s' "bash \"$0\" --install \"$SOURCE_APP\" \"$TARGET_APP\"" | sed 's/\\/\\\\/g; s/"/\\"/g')
if /usr/bin/osascript -e "do shell script \"$INSTALL_CMD\" with administrator privileges" >/dev/null 2>&1; then
  hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  exit 0
fi
hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
open "$DMG_PATH" >/dev/null 2>&1 || true
show_manual
'''
            .replaceAll('__DMG_PATH__', _shQuote(dmgPath))
            .replaceAll('__CURRENT_APP__', _shQuote(appBundle))
            .replaceAll('__APP_NAME__', _shQuote(appName))
            .replaceAll('__RELEASE_URL__', _shQuote(releaseUrl));
    await script.writeAsString(scriptContents, flush: true);
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
