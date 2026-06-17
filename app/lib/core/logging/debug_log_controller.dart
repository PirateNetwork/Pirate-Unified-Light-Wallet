import 'dart:io';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:path_provider/path_provider.dart';

import 'debug_log_native.dart';
import 'debug_log_path.dart';

const String kDebugLoggingStorageKey = 'ui_debug_logging_enabled_v1';

class DebugLogController {
  DebugLogController._();

  static const _storage = FlutterSecureStorage();
  static bool _enabled = false;

  static bool get isEnabled => _enabled;

  static Future<void> initialize() async {
    final enabled = await _readEnabled();
    _enabled = enabled;
    await setNativeDebugLoggingEnabled(enabled: enabled);
    if (!enabled) {
      await clearAllLogs();
    }
  }

  static Future<void> syncNativeState() async {
    await setNativeDebugLoggingEnabled(enabled: _enabled);
    if (!_enabled) {
      await clearNativeDebugLogs();
    }
  }

  static Future<void> setEnabled({required bool enabled}) async {
    _enabled = enabled;
    await _storage.write(
      key: kDebugLoggingStorageKey,
      value: enabled.toString(),
    );
    await setNativeDebugLoggingEnabled(enabled: enabled);
    if (!enabled) {
      await clearAllLogs();
    }
  }

  static Future<void> clearAllLogs() async {
    await clearNativeDebugLogs();
    final paths = await _candidateLogPaths();
    for (final path in paths) {
      await _deleteLogFamily(path);
      await _deleteRuntimeMarker(path);
    }
  }

  static String redactDebugLogText(String value) {
    var text = value;
    text = text.replaceAllMapped(
      RegExp(
        r'("?(?:mnemonic|seed|passphrase|password|pin|panic_pin|duress_passphrase|spending_key|sapling_key|orchard_key|sapling_viewing_key|orchard_viewing_key|viewing_key|extsk|ovk|ivk|fvk|private_key|secret|panic|panic_location|backtrace|stack)"?\s*[:=]\s*)("[^"]*"|[^,}\n]+)',
        caseSensitive: false,
      ),
      (match) => '${match[1]}"[REDACTED_SECRET]"',
    );
    text = text.replaceAllMapped(
      RegExp(
        r'("?(?:wallet_id|account_id|key_id|address|addresses|z_addresses|address_id|txid|txids|spent_txid|pending_txid|recent_txids|last_seen_txids|txid_prefix|nullifier|nullifiers|nf|cmu|cmx|cmx_prefix|commitment|memo|memo_hex|path|cwd|db_path|endpoint|url|host|server|server_name|tls_server_name|tls_pin)"?\s*[:=]\s*)("[^"]*"|[^,}\n]+)',
        caseSensitive: false,
      ),
      (match) => '${match[1]}"[REDACTED]"',
    );
    text = text.replaceAll(
      RegExp(
        r'\b(?:zs1|ztestsapling1|zregtestsapling1|pirate1|pirate-test1|pirate-regtest1)[a-z0-9_-]{20,}\b',
        caseSensitive: false,
      ),
      '[REDACTED_ADDRESS]',
    );
    text = text.replaceAll(
      RegExp(r'\b(?:0x)?[0-9a-f]{64,}\b', caseSensitive: false),
      '[REDACTED_HEX]',
    );
    text = text.replaceAll(RegExp(r'[A-Za-z]:\\[^\s",}]+'), '[REDACTED_PATH]');
    text = text.replaceAll(
      RegExp(r'/(?:Users|home|var|private|tmp|data|storage)/[^\s",}]+'),
      '[REDACTED_PATH]',
    );
    return text;
  }

  static Future<File?> exportRedactedDebugLogFile() async {
    final sourcePath = await resolveDebugLogPath();
    final source = File(sourcePath);
    if (!source.existsSync()) {
      return null;
    }

    final redacted = redactDebugLogText(await source.readAsString());
    final tempDir = await getTemporaryDirectory();
    final target = File(
      '${tempDir.path}${Platform.pathSeparator}pirate-debug-log-redacted.txt',
    );
    await target.writeAsString(redacted, flush: true);
    return target;
  }

  static Future<bool> hasDebugLog() async {
    final path = await resolveDebugLogPath();
    return File(path).existsSync();
  }

  static Future<bool> _readEnabled() async {
    try {
      final raw = await _storage.read(key: kDebugLoggingStorageKey);
      return raw?.toLowerCase() == 'true';
    } catch (_) {
      return false;
    }
  }

  static Future<Set<String>> _candidateLogPaths() async {
    final paths = <String>{await resolveDebugLogPath()};
    final envPath = Platform.environment['PIRATE_DEBUG_LOG_PATH'];
    if (envPath != null && envPath.trim().isNotEmpty) {
      paths.add(envPath);
    }

    final fallbackBase = Directory.current.path;
    if (Platform.isWindows) {
      for (final key in ['LOCALAPPDATA', 'APPDATA']) {
        final base = Platform.environment[key];
        if (base == null || base.isEmpty) continue;
        paths.add(
          _joinPath(base, [
            'Pirate',
            'PirateWallet',
            'data',
            'logs',
            'debug.log',
          ]),
        );
      }
    } else if (Platform.isMacOS || Platform.isIOS) {
      final home = Platform.environment['HOME'] ?? fallbackBase;
      paths.add(
        _joinPath(home, [
          'Library',
          'Application Support',
          'com.Pirate.PirateWallet',
          'logs',
          'debug.log',
        ]),
      );
    } else if (Platform.isLinux || Platform.isAndroid) {
      final home = Platform.environment['HOME'] ?? fallbackBase;
      final base =
          Platform.environment['XDG_DATA_HOME'] ??
          _joinPath(home, ['.local', 'share']);
      paths.add(_joinPath(base, ['piratewallet', 'logs', 'debug.log']));
    }

    return paths;
  }

  static Future<void> _deleteLogFamily(String path) async {
    for (final candidate in [
      path,
      for (var index = 1; index <= 10; index++) '$path.$index',
    ]) {
      try {
        final file = File(candidate);
        if (file.existsSync()) {
          file.deleteSync();
        }
      } catch (_) {
        // Best-effort cleanup only.
      }
    }
  }

  static Future<void> _deleteRuntimeMarker(String debugLogPath) async {
    try {
      final marker = File(
        '${File(debugLogPath).parent.path}${Platform.pathSeparator}runtime_session.marker',
      );
      if (marker.existsSync()) {
        marker.deleteSync();
      }
    } catch (_) {
      // Best-effort cleanup only.
    }
  }

  static String _joinPath(String base, List<String> parts) {
    var path = base;
    for (final part in parts) {
      if (part.isEmpty) continue;
      path = path.endsWith(Platform.pathSeparator)
          ? '$path$part'
          : '$path${Platform.pathSeparator}$part';
    }
    return path;
  }
}
