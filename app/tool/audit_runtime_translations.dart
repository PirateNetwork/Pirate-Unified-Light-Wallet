import 'dart:convert';
import 'dart:io';

import 'package:analyzer/dart/analysis/features.dart';
import 'package:analyzer/dart/analysis/utilities.dart';
import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/ast/visitor.dart';
import 'package:analyzer/source/line_info.dart';
import 'package:path/path.dart' as p;

final class _Use {
  const _Use(this.path, this.line);

  final String path;
  final int line;

  @override
  String toString() => '$path:$line';
}

final class _TranslationVisitor extends RecursiveAstVisitor<void> {
  _TranslationVisitor(this.path, this.lineInfo);

  final String path;
  final LineInfo lineInfo;
  final Map<String, List<_Use>> literals = <String, List<_Use>>{};
  final Map<String, List<_Use>> translated = <String, List<_Use>>{};
  final List<String> dynamicTranslations = <String>[];
  final List<String> untranslatedUiCandidates = <String>[];
  final List<String> untranslatedInterpolations = <String>[];
  final List<String> humanReviewCandidates = <String>[];

  static const uiArgumentNames = <String>{
    'actionLabel',
    'cancelLabel',
    'content',
    'description',
    'emptyText',
    'errorText',
    'helperText',
    'hintText',
    'label',
    'message',
    'placeholder',
    'semanticLabel',
    'subtitle',
    'suggestion',
    'text',
    'title',
    'tooltip',
  };

  static const uiInvocationNames = <String>{
    'PButton',
    'PText',
    'SelectableText',
    'Text',
    'TextSpan',
  };

  static bool isUiName(String name) => RegExp(
    '(description|displayName|empty|error|helper|hint|label|message|placeholder|prompt|reason|semantic|status|subtitle|suggestion|text|title|tooltip|warning)',
    caseSensitive: false,
  ).hasMatch(name);

  _Use useFor(AstNode node) {
    final location = lineInfo.getLocation(node.offset);
    return _Use(path, location.lineNumber);
  }

  void recordLiteral(String value, AstNode node) {
    literals.putIfAbsent(value, () => <_Use>[]).add(useFor(node));
  }

  @override
  void visitSimpleStringLiteral(SimpleStringLiteral node) {
    recordLiteral(node.value, node);
    if (!isInsideTranslation(node)) {
      if (!isInsideDirective(node) && looksLikeHumanText(node.value)) {
        humanReviewCandidates.add('${useFor(node)} ${jsonEncode(node.value)}');
      }
      if (isUiCandidate(node) && shouldTranslateUiLiteral(path, node.value)) {
        untranslatedUiCandidates.add(
          '${useFor(node)} ${jsonEncode(node.value)}',
        );
      }
    }
    super.visitSimpleStringLiteral(node);
  }

  bool isInsideDirective(AstNode node) {
    AstNode? current = node.parent;
    while (current != null) {
      if (current is Directive) return true;
      current = current.parent;
    }
    return false;
  }

  @override
  void visitStringInterpolation(StringInterpolation node) {
    if (!isInsideTranslation(node) &&
        isUiCandidate(node) &&
        shouldTranslateInterpolation(path, node)) {
      untranslatedInterpolations.add(
        '${useFor(node)} ${jsonEncode(node.toSource())}',
      );
    }
    super.visitStringInterpolation(node);
  }

  bool isInsideTranslation(AstNode node) {
    AstNode? current = node.parent;
    while (current != null && current is! Statement) {
      if (current case PropertyAccess(
        propertyName: final propertyName,
      ) when const {'tr', 'trArgs'}.contains(propertyName.name)) {
        return true;
      }
      if (current case PrefixedIdentifier(
        identifier: final identifier,
      ) when const {'tr', 'trArgs'}.contains(identifier.name)) {
        return true;
      }
      if (current case MethodInvocation(
        methodName: final methodName,
      ) when methodName.name == 'trArgs') {
        return true;
      }
      current = current.parent;
    }
    return false;
  }

  bool isUiCandidate(AstNode node) {
    AstNode? current = node.parent;
    while (current != null && current is! Statement) {
      if (current is NamedExpression &&
          uiArgumentNames.contains(current.name.label.name)) {
        return true;
      }
      if (current is VariableDeclaration && isUiName(current.name.lexeme)) {
        return true;
      }
      if (current is AssignmentExpression &&
          isUiName(current.leftHandSide.toSource())) {
        return true;
      }
      if (current is ReturnStatement) {
        AstNode? declaration = current.parent;
        while (declaration != null && declaration is! Declaration) {
          declaration = declaration.parent;
        }
        if (declaration is MethodDeclaration &&
            isUiName(declaration.name.lexeme)) {
          return true;
        }
      }
      if (current is ArgumentList) {
        final invocation = current.parent?.toSource() ?? '';
        final openParen = invocation.indexOf('(');
        final name = openParen < 0
            ? invocation
            : invocation.substring(0, openParen).trim();
        if (uiInvocationNames.any(
          (candidate) => name == candidate || name.endsWith('.$candidate'),
        )) {
          return true;
        }
      }
      current = current.parent;
    }
    return false;
  }

  @override
  void visitPropertyAccess(PropertyAccess node) {
    if (const {'tr', 'trArgs'}.contains(node.propertyName.name) &&
        node.target != null) {
      recordTranslation(node.target!, node);
    }
    super.visitPropertyAccess(node);
  }

  @override
  void visitPrefixedIdentifier(PrefixedIdentifier node) {
    if (const {'tr', 'trArgs'}.contains(node.identifier.name)) {
      recordTranslation(node.prefix, node);
    }
    super.visitPrefixedIdentifier(node);
  }

  @override
  void visitMethodInvocation(MethodInvocation node) {
    if (node.methodName.name == 'trArgs' && node.target != null) {
      recordTranslation(node.target!, node);
    }
    super.visitMethodInvocation(node);
  }

  void recordTranslation(Expression expression, AstNode node) {
    final values = staticValues(expression);
    if (values.isEmpty) {
      dynamicTranslations.add('${useFor(node)} ${expression.toSource()}.tr');
      return;
    }
    for (final value in values) {
      translated.putIfAbsent(value, () => <_Use>[]).add(useFor(node));
    }
  }

  Set<String> staticValues(Expression expression) {
    if (expression is ParenthesizedExpression) {
      return staticValues(expression.expression);
    }
    if (expression is StringLiteral) {
      final value = expression.stringValue;
      return value == null ? <String>{} : <String>{value};
    }
    if (expression is ConditionalExpression) {
      return <String>{
        ...staticValues(expression.thenExpression),
        ...staticValues(expression.elseExpression),
      };
    }
    return <String>{};
  }
}

bool shouldTranslateUiLiteral(String path, String value) {
  final normalized = path.replaceAll(r'\', '/');
  if (normalized.contains('/core/ffi/') ||
      normalized.contains('/core/logging/')) {
    return false;
  }
  if (const <String>{
    '*******',
    '-',
    '0',
    '0.05234 ARRR',
    '0.00000000',
    '1/2',
    'CSV',
    'Exception: ',
    'NETWORK_ERROR',
    'TLS',
    'blk/s',
    'direct',
    'error',
    'error_data',
    'i2p',
    'i2p_first_use_ack',
    'ltc1... or L...',
    'obfs4',
    'Pirate Unified Wallet',
    'Pirate Wallet',
    'Snowflake',
    'socks5',
    'status',
    'tor',
    '•',
  }.contains(value)) {
    return false;
  }
  if (value.contains(r'\') ||
      RegExp(r'^\d{1,3}(\.\d{1,3}){3}:\d+$').hasMatch(value) ||
      (value.startsWith('[') && value.endsWith(']'))) {
    return false;
  }
  if (value.trim().isEmpty ||
      value.startsWith('/') ||
      const <String>{
        '--',
        '---',
        '0.00',
        '1',
        'ARRR',
        'label',
        'monospace',
        'orchard',
      }.contains(value)) {
    return false;
  }
  if (normalized.endsWith('/settings/decoy_view.dart') &&
      value.contains('USD')) {
    return false;
  }
  if (normalized.endsWith('/settings/screens/diagnostics_screen.dart') &&
      value.contains('→')) {
    return false;
  }
  return true;
}

bool shouldTranslateInterpolation(String path, StringInterpolation node) {
  final normalized = path.replaceAll(r'\', '/');
  if (normalized.endsWith('/core/background/background_sync_handler.dart') ||
      normalized.endsWith('/core/background/background_sync_manager.dart') ||
      normalized.contains('/core/logging/')) {
    return false;
  }
  const technicalFragments = <String>{'ARRR', 'KDF', 'blk/s', 'ms'};
  return node.elements.whereType<InterpolationString>().any((element) {
    final fragment = element.value.trim();
    return !technicalFragments.contains(fragment) &&
        !RegExp(r'^[0-9A-Z/ .:+≈><-]+$').hasMatch(fragment) &&
        RegExp('[A-Za-z]{2,}').hasMatch(fragment);
  });
}

bool looksLikeHumanText(String value) {
  final trimmed = value.trim();
  if (trimmed.length < 2 || !RegExp('[A-Za-z]').hasMatch(trimmed)) {
    return false;
  }
  if (trimmed.startsWith(RegExp('(https?://|package:|dart:|assets/|lib/)')) ||
      trimmed.startsWith(RegExp(r'[-^\\./_(]')) ||
      trimmed.contains(r'\') ||
      RegExp(
        r'\.(dart|json|yaml|yml|csv|txt|log|png|svg|so|dll|dylib)$',
      ).hasMatch(trimmed) ||
      RegExp(r'^[a-z][A-Za-z0-9_]*$').hasMatch(trimmed) ||
      RegExp(r'^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$').hasMatch(trimmed) ||
      RegExp(r'^[A-Z0-9_./:+-]{2,12}$').hasMatch(trimmed)) {
    return false;
  }
  return trimmed.contains(RegExp(r'\s')) || trimmed.length > 12;
}

bool shouldScan(String path) {
  final normalized = path.replaceAll(r'\', '/');
  return normalized.endsWith('.dart') &&
      !normalized.endsWith('.g.dart') &&
      !normalized.endsWith('.freezed.dart') &&
      !normalized.contains('/core/ffi/generated/') &&
      !normalized.contains('/features/buy_arrr/') &&
      !normalized.contains('/features/showcase/') &&
      !normalized.contains('/previews/');
}

void printEntries(String heading, Iterable<String> entries) {
  final sorted = entries.toList()..sort();
  stdout.writeln('\n$heading (${sorted.length})');
  for (final entry in sorted) {
    stdout.writeln('  $entry');
  }
}

Map<String, String> readArb(File file) {
  final decoded = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  return <String, String>{
    for (final entry in decoded.entries)
      if (!entry.key.startsWith('@') && entry.value is String)
        entry.key: entry.value as String,
  };
}

Set<String> placeholders(String value) => RegExp(
  r'\{([A-Za-z][A-Za-z0-9_]*)\}',
).allMatches(value).map((match) => match.group(1)!).toSet();

String buildRuntimeEnglishArb(Iterable<String> keys) {
  final sortedKeys = keys.toList()..sort();
  final output = <String, dynamic>{
    '@@locale': 'en',
    for (final key in sortedKeys) key: key,
  };
  return '${const JsonEncoder.withIndent('  ').convert(output)}\n';
}

bool writeIfChanged(File file, String contents) {
  if (file.existsSync() && file.readAsStringSync() == contents) {
    return false;
  }
  file.parent.createSync(recursive: true);
  file.writeAsStringSync(contents);
  return true;
}

void main(List<String> arguments) {
  final write = arguments.contains('--write');
  final reviewAll = arguments.contains('--review-all');
  final arbFile = File('assets/i18n/app_en.arb');
  if (!arbFile.existsSync()) {
    stderr.writeln('Run this tool from the app directory.');
    exitCode = 64;
    return;
  }

  var arb = readArb(arbFile);
  final literals = <String, List<_Use>>{};
  final translated = <String, List<_Use>>{};
  final dynamicTranslations = <String>[];
  final untranslatedUiCandidates = <String>[];
  final untranslatedInterpolations = <String>[];
  final humanReviewCandidates = <String>[];
  final files =
      Directory('lib')
          .listSync(recursive: true)
          .whereType<File>()
          .where((file) => shouldScan(file.path))
          .toList()
        ..sort((left, right) => left.path.compareTo(right.path));

  for (final file in files) {
    final result = parseFile(
      path: file.path,
      featureSet: FeatureSet.latestLanguageVersion(),
      throwIfDiagnostics: false,
    );
    final relativePath = file.path.replaceAll(r'\', '/');
    final visitor = _TranslationVisitor(relativePath, result.lineInfo);
    result.unit.accept(visitor);
    for (final entry in visitor.literals.entries) {
      literals.putIfAbsent(entry.key, () => <_Use>[]).addAll(entry.value);
    }
    for (final entry in visitor.translated.entries) {
      translated.putIfAbsent(entry.key, () => <_Use>[]).addAll(entry.value);
    }
    dynamicTranslations.addAll(visitor.dynamicTranslations);
    untranslatedUiCandidates.addAll(visitor.untranslatedUiCandidates);
    untranslatedInterpolations.addAll(visitor.untranslatedInterpolations);
    humanReviewCandidates.addAll(visitor.humanReviewCandidates);
  }

  final translatedKeys = translated.keys.toSet();
  final literalKeys = literals.keys.toSet();

  if (write) {
    final changed = <String>[];
    if (writeIfChanged(arbFile, buildRuntimeEnglishArb(translatedKeys))) {
      changed.add(arbFile.path);
    }
    arb = <String, String>{for (final key in translatedKeys) key: key};
    printEntries('Updated translation files', changed);
  }

  final arbKeys = arb.keys.toSet();
  final missing = translatedKeys.difference(arbKeys);
  final stale = arbKeys.difference(translatedKeys);
  final englishMismatches = arb.entries
      .where((entry) => entry.key != entry.value)
      .map((entry) => '${entry.key} => ${entry.value}');
  final localeProblems = <String>[];
  final runtimeLocaleFiles = Directory('assets/i18n')
      .listSync()
      .whereType<File>()
      .where(
        (file) =>
            p.basename(file.path).startsWith('app_') &&
            p.extension(file.path) == '.arb' &&
            p.basename(file.path) != 'app_en.arb',
      );
  for (final localeFile in runtimeLocaleFiles) {
    final locale = readArb(localeFile);
    final localeKeys = locale.keys.toSet();
    for (final key in translatedKeys.difference(localeKeys)) {
      localeProblems.add('${localeFile.path}: missing ${jsonEncode(key)}');
    }
    for (final key in localeKeys.difference(translatedKeys)) {
      localeProblems.add('${localeFile.path}: stale ${jsonEncode(key)}');
    }
    for (final key in translatedKeys.intersection(localeKeys)) {
      final expected = placeholders(key);
      final actual = placeholders(locale[key]!);
      if (!expected.containsAll(actual) || !actual.containsAll(expected)) {
        localeProblems.add(
          '${localeFile.path}: placeholders for ${jsonEncode(key)} are '
          '${actual.toList()..sort()}, expected ${expected.toList()..sort()}',
        );
      }
    }
  }

  stdout
    ..writeln('Runtime ARB keys: ${arbKeys.length}')
    ..writeln('Static .tr strings: ${translatedKeys.length}')
    ..writeln('All source literals: ${literalKeys.length}');
  printEntries('Missing from runtime ARB', missing);
  printEntries('Runtime ARB keys not translated by production source', stale);
  printEntries('English key/value mismatches', englishMismatches);
  printEntries(
    'Dynamic .tr expressions requiring manual review',
    dynamicTranslations,
  );
  printEntries(
    'UI string literals without direct localization',
    untranslatedUiCandidates,
  );
  printEntries(
    'Interpolated UI strings without localization',
    untranslatedInterpolations,
  );
  printEntries('Runtime locale parity or placeholder problems', localeProblems);

  if (reviewAll) {
    printEntries(
      'All human-readable untranslated literals',
      humanReviewCandidates,
    );
  }

  if (missing.isNotEmpty ||
      stale.isNotEmpty ||
      englishMismatches.isNotEmpty ||
      dynamicTranslations.isNotEmpty ||
      untranslatedUiCandidates.isNotEmpty ||
      untranslatedInterpolations.isNotEmpty ||
      localeProblems.isNotEmpty) {
    exitCode = 1;
  }
}
