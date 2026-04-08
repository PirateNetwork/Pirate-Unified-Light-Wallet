import 'dart:convert';
import 'dart:io';

import 'debug_log_path.dart';

const int _defaultDebugLogMaxBytes = 100 * 1024 * 1024;
const int _defaultDebugLogBackups = 2;
const int _maxDebugLogBackups = 10;

int _parsePositiveIntEnv(String key, int fallback) {
  final raw = Platform.environment[key];
  if (raw == null) {
    return fallback;
  }

  final parsed = int.tryParse(raw.trim());
  if (parsed == null || parsed <= 0) {
    return fallback;
  }
  return parsed;
}

int _parseNonNegativeIntEnv(String key, int fallback) {
  final raw = Platform.environment[key];
  if (raw == null) {
    return fallback;
  }

  final parsed = int.tryParse(raw.trim());
  if (parsed == null || parsed < 0) {
    return fallback;
  }
  return parsed;
}

int _maxDebugLogBytes() {
  return _parsePositiveIntEnv(
    'PIRATE_DEBUG_LOG_MAX_BYTES',
    _defaultDebugLogMaxBytes,
  );
}

int _debugLogBackupCount() {
  final count = _parseNonNegativeIntEnv(
    'PIRATE_DEBUG_LOG_BACKUPS',
    _defaultDebugLogBackups,
  );
  if (count > _maxDebugLogBackups) {
    return _maxDebugLogBackups;
  }
  return count;
}

String _backupPath(String path, int index) {
  return '$path.$index';
}

Future<int> _fileLength(File file) async {
  try {
    if (!file.existsSync()) {
      return 0;
    }
    return file.lengthSync();
  } catch (_) {
    return 0;
  }
}

Future<void> _rotateDebugLog({
  required File activeFile,
  required String activePath,
  required int maxBytes,
  required int backups,
}) async {
  final currentBytes = await _fileLength(activeFile);
  if (currentBytes > maxBytes) {
    try {
      if (activeFile.existsSync()) {
        activeFile.deleteSync();
      }
    } catch (_) {
      // Ignore rotation failures.
    }
    return;
  }

  if (backups <= 0) {
    try {
      if (activeFile.existsSync()) {
        activeFile.deleteSync();
      }
    } catch (_) {
      // Ignore rotation failures.
    }
    return;
  }

  for (var index = backups; index >= 1; index--) {
    final srcPath = index == 1
        ? activePath
        : _backupPath(activePath, index - 1);
    final dstPath = _backupPath(activePath, index);
    final src = File(srcPath);
    final dst = File(dstPath);

    try {
      if (dst.existsSync()) {
        dst.deleteSync();
      }
      if (src.existsSync()) {
        src.renameSync(dstPath);
      }
    } catch (_) {
      // Ignore rotation failures.
    }
  }
}

Future<void> appendDebugLogLine(String line, {String? logPath}) async {
  try {
    final resolvedPath = logPath ?? await resolveDebugLogPath();
    final file = File(resolvedPath);
    file.parent.createSync(recursive: true);

    final maxBytes = _maxDebugLogBytes();
    final backups = _debugLogBackupCount();
    final payload = '$line\n';
    final incomingBytes = utf8.encode(payload).length;

    final beforeWrite = await _fileLength(file);
    if (beforeWrite + incomingBytes > maxBytes) {
      await _rotateDebugLog(
        activeFile: file,
        activePath: resolvedPath,
        maxBytes: maxBytes,
        backups: backups,
      );
    }

    file.writeAsStringSync(payload, mode: FileMode.append, flush: true);

    final afterWrite = await _fileLength(file);
    if (afterWrite > maxBytes) {
      await _rotateDebugLog(
        activeFile: file,
        activePath: resolvedPath,
        maxBytes: maxBytes,
        backups: backups,
      );
    }
  } catch (_) {
    // Ignore logging failures.
  }
}
