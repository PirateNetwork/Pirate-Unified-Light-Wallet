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
  if (!Platform.isWindows) return true;
  final build = windowsBuildNumber();
  if (build == null) return true;
  return build >= 22000;
}
