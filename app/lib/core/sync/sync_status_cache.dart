import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class SyncStatusCache {
  static const FlutterSecureStorage _storage = FlutterSecureStorage();
  static const String _heightKey = 'sync_last_known_height_v1';
  static const Duration _writeInterval = Duration(seconds: 10);
  static int? _cachedHeight;
  static DateTime? _lastWrite;

  static Future<int> read() async {
    if (_cachedHeight != null) {
      return _cachedHeight!;
    }
    String? raw;
    try {
      raw = await _storage.read(key: _heightKey);
    } catch (_) {
      raw = null;
    }
    if (raw == null || raw.isEmpty) {
      _cachedHeight = 0;
      return 0;
    }
    final parsed = int.tryParse(raw) ?? 0;
    _cachedHeight = parsed;
    return parsed;
  }

  static Future<void> update(int height) async {
    if (height <= 0) {
      return;
    }
    _cachedHeight = height;
    final now = DateTime.now();
    if (_lastWrite != null && now.difference(_lastWrite!) < _writeInterval) {
      return;
    }
    _lastWrite = now;
    try {
      await _storage.write(key: _heightKey, value: height.toString());
    } catch (_) {
      // Best-effort cache persistence.
    }
  }
}
