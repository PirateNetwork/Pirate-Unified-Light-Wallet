// ignore_for_file: use_setters_to_change_properties

part of 'ffi_bridge.dart';

@visibleForTesting
class SyncRestartBackoff {
  static const List<Duration> _delays = <Duration>[
    Duration(seconds: 2),
    Duration(seconds: 5),
    Duration(seconds: 15),
    Duration(seconds: 30),
    Duration(seconds: 60),
  ];

  int _stoppedPolls = 0;
  int _restartAttempts = 0;
  DateTime? _lastRestartAttempt;
  DateTime? _stoppedAfterAttemptAt;
  bool _wasRunning = false;

  bool shouldRestart({
    required bool isRunning,
    required DateTime now,
    bool madeProgress = false,
  }) {
    if (madeProgress) {
      _restartAttempts = 0;
      _lastRestartAttempt = null;
      _stoppedAfterAttemptAt = null;
    }
    if (isRunning) {
      _stoppedPolls = 0;
      _wasRunning = true;
      return false;
    }

    if (_wasRunning) {
      _wasRunning = false;
      _stoppedPolls = 0;
      _stoppedAfterAttemptAt = now;
    }
    _stoppedPolls += 1;
    if (_stoppedPolls < 2) return false;

    final lastAttempt = _lastRestartAttempt;
    if (lastAttempt == null) return true;
    final delayIndex = (_restartAttempts - 1).clamp(0, _delays.length - 1);
    // A failed async attempt can run longer than its nominal delay. Start the
    // next delay when that task actually stops so long failures cannot bypass
    // the recovery backoff.
    final backoffStartedAt = _stoppedAfterAttemptAt ?? lastAttempt;
    return now.difference(backoffStartedAt) >= _delays[delayIndex];
  }

  void recordRestartAttempt(DateTime now) {
    _lastRestartAttempt = now;
    _stoppedAfterAttemptAt = null;
    _restartAttempts += 1;
    _stoppedPolls = 0;
  }

  void reset() {
    _stoppedPolls = 0;
    _restartAttempts = 0;
    _lastRestartAttempt = null;
    _stoppedAfterAttemptAt = null;
    _wasRunning = false;
  }
}

class _SyncProgressPollState {
  int idleCount = 0;
  int lastHeight = 0;
  int lastTargetHeight = 0;
  final SyncRestartBackoff restartBackoff = SyncRestartBackoff();

  bool recordProgress(SyncStatus status) {
    final currentHeight = status.localHeight.toInt();
    final targetHeight = status.targetHeight.toInt();
    final localHeightChanged = currentHeight != lastHeight;
    if (currentHeight != lastHeight || targetHeight != lastTargetHeight) {
      lastHeight = currentHeight;
      lastTargetHeight = targetHeight;
      idleCount = 0;
      return localHeightChanged;
    }
    idleCount++;
    return false;
  }

  Duration nextInterval({required SyncStatus status, required bool isRunning}) {
    // Keep polling frequent when sync is running (even if caught up) to show
    // new blocks quickly.
    if (status.isSyncing) {
      return const Duration(milliseconds: 500);
    }
    if (isRunning) {
      // Sync is running but caught up; keep checking for new blocks quickly.
      return const Duration(seconds: 1);
    }
    if (idleCount < 10) {
      return const Duration(seconds: 1);
    }
    return const Duration(seconds: 2);
  }
}

class _TransactionPollState {
  final Map<String, String> lastSeenStates = <String, String>{};
  DateTime lastCheckTime = DateTime.now();

  void trimSeenStates(List<TxInfo> transactions) {
    if (lastSeenStates.length <= 1000) {
      return;
    }

    final recentTxids = transactions.take(1000).map((tx) => tx.txid).toSet();
    lastSeenStates.removeWhere((key, _) => !recentTxids.contains(key));
  }

  bool shouldLogError(DateTime now) {
    if (now.difference(lastCheckTime).inSeconds <= 30) {
      return false;
    }
    lastCheckTime = now;
    return true;
  }
}

class _FfiBridgeSyncStreamHelper {
  static Stream<SyncStatus> syncProgressStream(WalletId id) async* {
    final state = _SyncProgressPollState();

    while (true) {
      try {
        if (!FfiBridge.appIsActive) {
          await Future<void>.delayed(const Duration(seconds: 2));
          continue;
        }

        final tunnelMode = await FfiBridge.getTunnel();
        final tunnelReady = await _FfiBridgeNetworkHelper.isTunnelReadyForSync(
          tunnelMode,
        );
        if (!tunnelReady) {
          state.restartBackoff.reset();
          final status = await FfiBridge.syncStatus(id);
          yield status;
          await Future<void>.delayed(const Duration(seconds: 2));
          continue;
        }

        final isRunning = await FfiBridge.isSyncRunning(id);
        final status = await FfiBridge.syncStatus(id);

        final now = DateTime.now();
        final madeProgress = state.recordProgress(status);
        if (state.restartBackoff.shouldRestart(
          isRunning: isRunning,
          now: now,
          madeProgress: madeProgress,
        )) {
          state.restartBackoff.recordRestartAttempt(now);
          if (!await FfiBridge.isDecoyMode()) {
            try {
              await FfiBridge.startSync(id, SyncMode.compact);
            } catch (error) {
              debugPrint('Automatic sync restart failed: $error');
            }
          } else {
            state.restartBackoff.reset();
          }
        }

        yield status;
        await Future<void>.delayed(
          state.nextInterval(status: status, isRunning: isRunning),
        );
      } catch (_) {
        // If sync status fails, yield a default status instead of crashing the
        // stream so the UI can continue polling.
        yield _defaultSyncStatus();
        await Future<void>.delayed(const Duration(seconds: 5));
      }
    }
  }

  static Stream<TxInfo> transactionStream(WalletId id) async* {
    if (!kUseFrbBindings) {
      return;
    }

    final state = _TransactionPollState();

    while (true) {
      try {
        if (!FfiBridge.appIsActive) {
          await Future<void>.delayed(const Duration(seconds: 3));
          continue;
        }

        final isSyncing = await FfiBridge.isSyncRunning(id);
        final pollInterval = isSyncing
            ? const Duration(seconds: 2)
            : const Duration(seconds: 5);
        await Future<void>.delayed(pollInterval);

        final transactions = await FfiBridge.listTransactions(id, limit: 100);
        for (final txInfo in transactions) {
          final stateKey = '${txInfo.height ?? 0}:${txInfo.confirmed ? 1 : 0}';
          final previousState = state.lastSeenStates[txInfo.txid];
          if (previousState != stateKey) {
            state.lastSeenStates[txInfo.txid] = stateKey;
            yield txInfo;
          }
        }

        state.trimSeenStates(transactions);
      } catch (e) {
        final now = DateTime.now();
        if (state.shouldLogError(now)) {
          debugPrint('Failed to get transactions for wallet $id: $e');
        }
        await Future<void>.delayed(const Duration(seconds: 5));
      }
    }
  }

  static SyncStatus _defaultSyncStatus() {
    return SyncStatus(
      localHeight: BigInt.zero,
      targetHeight: BigInt.zero,
      percent: 0.0,
      stage: SyncStage.verify,
      eta: null,
      blocksPerSecond: 0.0,
      notesDecrypted: BigInt.zero,
      lastBatchMs: BigInt.zero,
    );
  }
}
