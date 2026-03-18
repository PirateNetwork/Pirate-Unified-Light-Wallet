// ignore_for_file: use_setters_to_change_properties

part of 'ffi_bridge.dart';

class _SyncProgressPollState {
  int idleCount = 0;
  int lastHeight = 0;
  int lastTargetHeight = 0;
  DateTime? lastRestartAttempt;
  DateTime? lastStartAttempt;

  void recordProgress(SyncStatus status) {
    final currentHeight = status.localHeight.toInt();
    final targetHeight = status.targetHeight.toInt();
    if (currentHeight != lastHeight || targetHeight != lastTargetHeight) {
      lastHeight = currentHeight;
      lastTargetHeight = targetHeight;
      idleCount = 0;
      return;
    }
    idleCount++;
  }

  bool shouldRestart(DateTime now) {
    return lastRestartAttempt == null ||
        now.difference(lastRestartAttempt!) > const Duration(seconds: 8);
  }

  bool shouldStart(DateTime now) {
    return lastStartAttempt == null ||
        now.difference(lastStartAttempt!) > const Duration(seconds: 3);
  }

  void markRestartAttempt(DateTime now) {
    lastRestartAttempt = now;
  }

  void markStartAttempt(DateTime now) {
    lastStartAttempt = now;
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
          final status = await FfiBridge.syncStatus(id);
          yield status;
          await Future<void>.delayed(const Duration(seconds: 2));
          continue;
        }

        final isRunning = await FfiBridge.isSyncRunning(id);
        final status = await FfiBridge.syncStatus(id);
        await _ensureSyncLoopRunning(
          id: id,
          status: status,
          isRunning: isRunning,
          state: state,
        );

        yield status;
        state.recordProgress(status);
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

  static Future<void> _ensureSyncLoopRunning({
    required WalletId id,
    required SyncStatus status,
    required bool isRunning,
    required _SyncProgressPollState state,
  }) async {
    // Auto-restart sync whenever it stops after initial height discovery.
    // Even when local == target, the running sync loop is what keeps the
    // wallet tracking new blocks as they arrive.
    if (!isRunning && status.targetHeight > BigInt.zero) {
      final now = DateTime.now();
      if (state.shouldRestart(now)) {
        state.markRestartAttempt(now);
        try {
          await FfiBridge.startSync(id, SyncMode.compact);
        } catch (_) {
          // Ignore restart failures; stream will retry later.
        }
      }
      return;
    }

    if (status.targetHeight == BigInt.zero) {
      // Recover from stale or cleared session states (for example after
      // transport-mode toggles) even when localHeight is non-zero.
      final now = DateTime.now();
      if (state.shouldStart(now)) {
        state.markStartAttempt(now);
        try {
          await FfiBridge.startSync(id, SyncMode.compact);
        } catch (_) {
          // Ignore start failures; stream will retry later.
        }
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
