import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';

void main() {
  test('restarts only after confirming the sync task stopped', () {
    final policy = SyncRestartBackoff();
    final now = DateTime.utc(2026, 8, 24);

    expect(policy.shouldRestart(isRunning: false, now: now), isFalse);
    expect(
      policy.shouldRestart(
        isRunning: false,
        now: now.add(const Duration(seconds: 1)),
      ),
      isTrue,
    );
  });

  test('backs off repeated failed restart attempts', () {
    final policy = SyncRestartBackoff();
    final now = DateTime.utc(2026, 8, 24);

    expect(policy.shouldRestart(isRunning: false, now: now), isFalse);
    final firstAttempt = now.add(const Duration(seconds: 1));
    expect(policy.shouldRestart(isRunning: false, now: firstAttempt), isTrue);
    policy.recordRestartAttempt(firstAttempt);

    expect(
      policy.shouldRestart(
        isRunning: false,
        now: firstAttempt.add(const Duration(seconds: 1)),
      ),
      isFalse,
    );
    expect(
      policy.shouldRestart(
        isRunning: false,
        now: firstAttempt.add(const Duration(seconds: 2)),
      ),
      isTrue,
    );
    policy.recordRestartAttempt(firstAttempt.add(const Duration(seconds: 2)));

    expect(
      policy.shouldRestart(
        isRunning: false,
        now: firstAttempt.add(const Duration(seconds: 6)),
      ),
      isFalse,
    );
    expect(
      policy.shouldRestart(
        isRunning: false,
        now: firstAttempt.add(const Duration(seconds: 7)),
      ),
      isTrue,
    );
  });

  test('stable running state clears recovery backoff', () {
    final policy = SyncRestartBackoff();
    final now = DateTime.utc(2026, 8, 24);

    expect(policy.shouldRestart(isRunning: false, now: now), isFalse);
    expect(
      policy.shouldRestart(
        isRunning: false,
        now: now.add(const Duration(seconds: 1)),
      ),
      isTrue,
    );
    policy.recordRestartAttempt(now.add(const Duration(seconds: 1)));

    for (var poll = 0; poll < 3; poll++) {
      expect(
        policy.shouldRestart(
          isRunning: true,
          now: now.add(Duration(seconds: poll + 2)),
        ),
        isFalse,
      );
    }

    expect(
      policy.shouldRestart(
        isRunning: false,
        now: now.add(const Duration(seconds: 5)),
      ),
      isFalse,
    );
    expect(
      policy.shouldRestart(
        isRunning: false,
        now: now.add(const Duration(seconds: 6)),
      ),
      isTrue,
    );
  });
}
