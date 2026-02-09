import 'dart:io';

int? windowsBuildNumber() {
  if (!Platform.isWindows) return null;
  final match =
      RegExp(r'(\d+)\.(\d+)\.(\d+)').firstMatch(Platform.operatingSystemVersion);
  if (match == null) return null;
  return int.tryParse(match.group(3) ?? '');
}

bool isWindows11OrLater() {
  final build = windowsBuildNumber();
  if (build == null) return false;
  return build >= 22000;
}

bool shouldUseCustomTitleBar() {
  // macOS should always use native titlebar controls (traffic lights).
  // Rendering custom controls there causes duplicate chrome and overlay issues.
  if (Platform.isMacOS) return false;

  // Keep custom titlebar on Linux for visual consistency.
  if (Platform.isLinux) return true;

  // Windows: only use custom titlebar on Windows 11+.
  if (!Platform.isWindows) return false;
  final build = windowsBuildNumber();
  if (build == null) return true;
  return build >= 22000;
}
