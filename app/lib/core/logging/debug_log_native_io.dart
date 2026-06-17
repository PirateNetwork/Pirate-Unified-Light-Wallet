import 'dart:ffi' as ffi;
import 'dart:io';

class _DebugLogNativeBindings {
  _DebugLogNativeBindings(ffi.DynamicLibrary library)
    : setEnabled = library
          .lookupFunction<ffi.Void Function(ffi.Uint8), void Function(int)>(
            'pirate_debug_log_set_enabled',
          ),
      isEnabled = library.lookupFunction<ffi.Uint8 Function(), int Function()>(
        'pirate_debug_log_is_enabled',
      ),
      clear = library.lookupFunction<ffi.Void Function(), void Function()>(
        'pirate_debug_log_clear',
      );

  final void Function(int) setEnabled;
  final int Function() isEnabled;
  final void Function() clear;
}

_DebugLogNativeBindings? _bindings;

ffi.DynamicLibrary _openLibrary() {
  if (Platform.isIOS || Platform.isMacOS) {
    try {
      return ffi.DynamicLibrary.process();
    } catch (_) {
      if (Platform.isIOS) {
        rethrow;
      }
    }
  }

  if (Platform.isWindows) {
    return ffi.DynamicLibrary.open('pirate_ffi_frb.dll');
  }
  if (Platform.isAndroid || Platform.isLinux) {
    return ffi.DynamicLibrary.open('libpirate_ffi_frb.so');
  }
  if (Platform.isMacOS) {
    return ffi.DynamicLibrary.open('libpirate_ffi_frb.dylib');
  }

  return ffi.DynamicLibrary.process();
}

_DebugLogNativeBindings? _loadBindings() {
  final existing = _bindings;
  if (existing != null) {
    return existing;
  }

  try {
    final loaded = _DebugLogNativeBindings(_openLibrary());
    _bindings = loaded;
    return loaded;
  } catch (_) {
    return null;
  }
}

Future<void> setNativeDebugLoggingEnabled({required bool enabled}) async {
  try {
    _loadBindings()?.setEnabled(enabled ? 1 : 0);
  } catch (_) {
    // Native logging may not be loaded yet during early Flutter startup.
  }
}

Future<bool?> getNativeDebugLoggingEnabled() async {
  try {
    final value = _loadBindings()?.isEnabled();
    return value == null ? null : value != 0;
  } catch (_) {
    return null;
  }
}

Future<void> clearNativeDebugLogs() async {
  try {
    _loadBindings()?.clear();
  } catch (_) {
    // Best-effort cleanup only.
  }
}
