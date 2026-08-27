import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/logging/debug_log_preference_store.dart';

void main() {
  late Directory temporaryDirectory;

  setUp(() async {
    temporaryDirectory = await Directory.systemTemp.createTemp(
      'pirate-debug-preference-',
    );
  });

  tearDown(() async {
    if (temporaryDirectory.existsSync()) {
      await temporaryDirectory.delete(recursive: true);
    }
  });

  test('macOS file source remains usable when secure storage fails', () async {
    final store = DebugLogPreferenceStore(
      secureRead: (_) => throw Exception('missing entitlement'),
      secureWrite: (_, _) => throw Exception('missing entitlement'),
      supportDirectory: () async => temporaryDirectory,
      preferFile: true,
    );

    await store.write(key: 'debug-enabled', enabled: true);

    expect(await store.read(key: 'debug-enabled'), isTrue);
    final fallback = File(
      '${temporaryDirectory.path}${Platform.pathSeparator}preferences'
      '${Platform.pathSeparator}${DebugLogPreferenceStore.fallbackFileName}',
    );
    expect(await fallback.readAsString(), 'true');
  });

  test('macOS file value wins after secure storage recovers stale data', () async {
    final preferences = Directory(
      '${temporaryDirectory.path}${Platform.pathSeparator}preferences',
    );
    await preferences.create(recursive: true);
    await File(
      '${preferences.path}${Platform.pathSeparator}'
      '${DebugLogPreferenceStore.fallbackFileName}',
    ).writeAsString('true');
    var secureReads = 0;
    final store = DebugLogPreferenceStore(
      secureRead: (_) async {
        secureReads++;
        return 'false';
      },
      secureWrite: (_, _) async {},
      supportDirectory: () async => temporaryDirectory,
      preferFile: true,
    );

    expect(await store.read(key: 'debug-enabled'), isTrue);
    expect(secureReads, 0);
  });

  test('secure value migrates to fallback when no file exists', () async {
    final store = DebugLogPreferenceStore(
      secureRead: (_) async => 'true',
      secureWrite: (_, _) async {},
      supportDirectory: () async => temporaryDirectory,
      preferFile: true,
    );

    expect(await store.read(key: 'debug-enabled'), isTrue);
    final fallback = File(
      '${temporaryDirectory.path}${Platform.pathSeparator}preferences'
      '${Platform.pathSeparator}${DebugLogPreferenceStore.fallbackFileName}',
    );
    expect(await fallback.readAsString(), 'true');
  });
}
