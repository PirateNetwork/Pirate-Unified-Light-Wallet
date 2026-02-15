import 'dart:io';

bool shouldSkipFfiTests() {
  final env = Platform.environment;
  if (env['FORCE_FFI_TESTS'] == 'true') {
    return false;
  }
  if (env['CI'] == 'true' ||
      env['GITHUB_ACTIONS'] == 'true' ||
      env['SKIP_FFI_TESTS'] == 'true') {
    return true;
  }
  return !_ffiLibraryExists();
}

bool _ffiLibraryExists() {
  final candidates = <String>[];
  if (Platform.isWindows) {
    candidates.addAll(const [
      'pirate_ffi_frb.dll',
      '../pirate_ffi_frb.dll',
      'build/windows/x64/runner/Debug/pirate_ffi_frb.dll',
      'build/windows/x64/runner/Release/pirate_ffi_frb.dll',
      '../build/windows/x64/runner/Debug/pirate_ffi_frb.dll',
      '../build/windows/x64/runner/Release/pirate_ffi_frb.dll',
    ]);
  } else if (Platform.isLinux) {
    candidates.addAll(const [
      'libpirate_ffi_frb.so',
      '../libpirate_ffi_frb.so',
      'build/linux/x64/release/bundle/lib/libpirate_ffi_frb.so',
      '../build/linux/x64/release/bundle/lib/libpirate_ffi_frb.so',
    ]);
  } else if (Platform.isMacOS) {
    candidates.addAll(const [
      'libpirate_ffi_frb.dylib',
      '../libpirate_ffi_frb.dylib',
      'build/macos/Build/Products/Release/libpirate_ffi_frb.dylib',
      '../build/macos/Build/Products/Release/libpirate_ffi_frb.dylib',
    ]);
  }
  for (final path in candidates) {
    if (File(path).existsSync()) {
      return true;
    }
  }
  return false;
}
