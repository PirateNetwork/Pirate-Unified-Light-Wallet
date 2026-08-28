import 'dart:convert';
import 'dart:io';

void main(List<String> args) {
  _synchronizeLocales(baseReference: _argumentValue(args, '--base-ref'));
}

void _synchronizeLocales({String? baseReference}) {
  final directory = Directory('assets/i18n');
  final englishFile = File('${directory.path}/app_en.arb');
  if (!englishFile.existsSync()) {
    stderr.writeln('Run this tool from the app directory.');
    exitCode = 64;
    return;
  }

  final english = _readArb(englishFile);
  final localeFiles =
      directory
          .listSync()
          .whereType<File>()
          .where(
            (file) =>
                file.path.endsWith('.arb') &&
                !file.path.replaceAll(r'\', '/').endsWith('/app_en.arb'),
          )
          .toList()
        ..sort((left, right) => left.path.compareTo(right.path));

  for (final file in localeFiles) {
    final locale = _readArb(file);
    final sourceOrder = baseReference == null
        ? locale
        : _readGitArb(baseReference, file.path) ?? locale;
    final synchronized = <String, dynamic>{'@@locale': locale['@@locale']};
    for (final entry in sourceOrder.entries) {
      if (entry.key == '@@locale' || !english.containsKey(entry.key)) continue;
      synchronized[entry.key] = locale[entry.key] is String
          ? locale[entry.key]
          : english[entry.key];
    }
    for (final entry in english.entries) {
      if (entry.key == '@@locale' || synchronized.containsKey(entry.key)) {
        continue;
      }
      synchronized[entry.key] = locale[entry.key] is String
          ? locale[entry.key]
          : entry.value;
    }
    final added = synchronized.keys.toSet().difference(locale.keys.toSet());
    final removed = locale.keys.toSet().difference(synchronized.keys.toSet());
    file.writeAsStringSync(
      '${const JsonEncoder.withIndent('  ').convert(synchronized)}\n',
    );
    stdout.writeln(
      '${file.path}: added ${added.length}, removed ${removed.length}',
    );
  }
}

String? _argumentValue(List<String> args, String name) {
  for (var index = 0; index < args.length - 1; index++) {
    if (args[index] == name) return args[index + 1];
  }
  return null;
}

Map<String, dynamic>? _readGitArb(String reference, String path) {
  final normalized = path.replaceAll(r'\', '/');
  final repositoryPath = normalized.startsWith('app/')
      ? normalized
      : 'app/$normalized';
  final result = Process.runSync('git', ['show', '$reference:$repositoryPath']);
  if (result.exitCode != 0) return null;
  return Map<String, dynamic>.from(jsonDecode(result.stdout as String) as Map);
}

Map<String, dynamic> _readArb(File file) {
  return Map<String, dynamic>.from(
    jsonDecode(file.readAsStringSync()) as Map<String, dynamic>,
  );
}
