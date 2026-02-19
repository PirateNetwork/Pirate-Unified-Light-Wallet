import 'dart:io';

String? _cachedDebugLogPath;

String _joinPath(String base, List<String> parts) {
  var path = base;
  for (final part in parts) {
    if (part.isEmpty) continue;
    if (path.endsWith(Platform.pathSeparator)) {
      path = '$path$part';
    } else {
      path = '$path${Platform.pathSeparator}$part';
    }
  }
  return path;
}

Future<String> resolveDebugLogPath() {
  if (_cachedDebugLogPath != null) {
    return Future.value(_cachedDebugLogPath!);
  }

  final envPath = Platform.environment['PIRATE_DEBUG_LOG_PATH'];
  if (envPath != null && envPath.trim().isNotEmpty) {
    _cachedDebugLogPath = envPath;
    return Future.value(envPath);
  }

  final fallbackBase = Directory.current.path;
  if (Platform.isWindows) {
    final base =
        Platform.environment['LOCALAPPDATA'] ??
        Platform.environment['APPDATA'] ??
        fallbackBase;
    final path = _joinPath(base, [
      'Pirate',
      'PirateWallet',
      'data',
      'logs',
      'debug.log',
    ]);
    _cachedDebugLogPath = path;
    return Future.value(path);
  }
  if (Platform.isMacOS || Platform.isIOS) {
    final home = Platform.environment['HOME'] ?? fallbackBase;
    final base = _joinPath(home, ['Library', 'Application Support']);
    final path = _joinPath(base, [
      'com.Pirate.PirateWallet',
      'logs',
      'debug.log',
    ]);
    _cachedDebugLogPath = path;
    return Future.value(path);
  }
  if (Platform.isLinux || Platform.isAndroid) {
    final home = Platform.environment['HOME'] ?? fallbackBase;
    final base =
        Platform.environment['XDG_DATA_HOME'] ??
        _joinPath(home, ['.local', 'share']);
    final path = _joinPath(base, ['piratewallet', 'logs', 'debug.log']);
    _cachedDebugLogPath = path;
    return Future.value(path);
  }

  final path = _joinPath(fallbackBase, ['.cursor', 'debug.log']);
  _cachedDebugLogPath = path;
  return Future.value(path);
}
