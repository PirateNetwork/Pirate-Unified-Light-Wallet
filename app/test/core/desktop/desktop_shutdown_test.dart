import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/desktop/desktop_shutdown.dart';

void main() {
  test('completes cleanup before requesting native window close', () async {
    final calls = <String>[];
    final cleanupGate = Completer<void>();
    final coordinator = DesktopShutdownCoordinator(
      hideWindow: () async => calls.add('hide'),
      cleanUp: () async {
        calls.add('cleanup-start');
        await cleanupGate.future;
        calls.add('cleanup-finish');
      },
      releaseInstanceLock: () async => calls.add('release'),
      allowWindowClose: () async => calls.add('allow-close'),
      closeWindow: () async => calls.add('close'),
      forceDestroyWindow: () async => calls.add('destroy'),
    );

    final close = coordinator.close();
    await Future<void>.delayed(Duration.zero);

    expect(calls, ['hide', 'cleanup-start']);
    cleanupGate.complete();
    await close;

    expect(calls, [
      'hide',
      'cleanup-start',
      'cleanup-finish',
      'release',
      'allow-close',
      'close',
    ]);
  });

  test('coalesces repeated close requests', () async {
    var cleanupCalls = 0;
    var closeCalls = 0;
    final coordinator = DesktopShutdownCoordinator(
      hideWindow: () async {},
      cleanUp: () async => cleanupCalls += 1,
      releaseInstanceLock: () async {},
      allowWindowClose: () async {},
      closeWindow: () async => closeCalls += 1,
      forceDestroyWindow: () async {},
    );

    await Future.wait([coordinator.close(), coordinator.close()]);

    expect(cleanupCalls, 1);
    expect(closeCalls, 1);
  });

  test('continues after cleanup failures', () async {
    final calls = <String>[];
    final coordinator = DesktopShutdownCoordinator(
      hideWindow: () async => throw StateError('hide failed'),
      cleanUp: () async => throw StateError('cleanup failed'),
      releaseInstanceLock: () async => calls.add('release'),
      allowWindowClose: () async => calls.add('allow-close'),
      closeWindow: () async => calls.add('close'),
      forceDestroyWindow: () async => calls.add('destroy'),
    );

    await coordinator.close();

    expect(calls, ['release', 'allow-close', 'close']);
  });

  test('force-destroys only when the normal close path fails', () async {
    final calls = <String>[];
    final coordinator = DesktopShutdownCoordinator(
      hideWindow: () async {},
      cleanUp: () async {},
      releaseInstanceLock: () async {},
      allowWindowClose: () async => calls.add('allow-close'),
      closeWindow: () async {
        calls.add('close');
        throw StateError('close failed');
      },
      forceDestroyWindow: () async => calls.add('destroy'),
    );

    await coordinator.close();

    expect(calls, ['allow-close', 'close', 'destroy']);
  });
}
