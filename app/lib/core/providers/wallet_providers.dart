// Wallet providers using Riverpod + FFI

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../background/background_sync_handler.dart';
import '../ffi/ffi_bridge.dart';
import '../ffi/generated/models.dart' hide SyncLogEntryFfi;
import '../ffi/generated/api.dart' as api;
import 'rust_init_provider.dart';
import '../security/duress_passphrase_store.dart';
import '../sync/sync_status_cache.dart';
import '../services/birthday_update_service.dart';

// ============================================================================
// Session & Active Wallet
// ============================================================================

/// Active wallet ID
final activeWalletProvider = NotifierProvider<ActiveWalletNotifier, WalletId?>(
  ActiveWalletNotifier.new,
);

class ActiveWalletNotifier extends Notifier<WalletId?> {
  @override
  WalletId? build() {
    _loadActiveWallet();
    return null;
  }

  bool _isSwitching = false;

  Future<void> _loadActiveWallet() async {
    final walletId = await FfiBridge.getActiveWallet();
    state = walletId;
    if (walletId != null) {
      BackgroundSyncHandler().updateActiveWallet(walletId);
      // Auto-start sync when loading active wallet on app startup
      unawaited(_startWalletSessions(walletId));
    }
  }

  Future<void> setActiveWallet(WalletId id) async {
    if (_isSwitching) return;

    _isSwitching = true;
    try {
      final previous = state;
      if (previous == id) {
        await FfiBridge.switchWallet(id);
        _notifyBackgroundHandler(id);
        return;
      }

      if (previous != null) {
        await _stopWalletSessions(previous);
      }

      await FfiBridge.switchWallet(id);
      state = id;
      _notifyBackgroundHandler(id);

      await _startWalletSessions(id);
    } finally {
      _isSwitching = false;
    }
  }

  void clearActiveWallet() {
    state = null;
    BackgroundSyncHandler().updateActiveWallet(null);
  }

  Future<void> _stopWalletSessions(WalletId walletId) async {
    try {
      await FfiBridge.cancelSync(walletId).timeout(const Duration(seconds: 3));
    } catch (_) {
      // Ignore cancellation errors in stubbed environment
    }
  }

  Future<void> _startWalletSessions(WalletId walletId) async {
    try {
      await FfiBridge.startSync(walletId, SyncMode.compact);
    } catch (_) {
      // Sync failures are surfaced via sync providers; swallow here
    }
  }

  void _notifyBackgroundHandler(WalletId walletId) {
    BackgroundSyncHandler().updateActiveWallet(walletId);
  }
}

/// All wallets list
final walletsProvider = FutureProvider<List<WalletMeta>>((ref) async {
  return FfiBridge.listWallets();
});

/// Metadata for the currently active wallet (if available)
final activeWalletMetaProvider = Provider<WalletMeta?>((ref) {
  final activeWalletId = ref.watch(activeWalletProvider);
  final walletsAsync = ref.watch(walletsProvider);

  return walletsAsync.maybeWhen(
    data: (wallets) {
      if (activeWalletId == null) return null;
      for (final wallet in wallets) {
        if (wallet.id == activeWalletId) {
          return wallet;
        }
      }
      return null;
    },
    orElse: () => null,
  );
});

/// Refresh wallets list
final refreshWalletsProvider = Provider<void Function()>((ref) {
  return () {
    ref.invalidate(walletsProvider);
  };
});

// ============================================================================
// Wallet Creation & Restore
// ============================================================================

/// Create new wallet
final createWalletProvider = Provider<
    Future<WalletId> Function({
      required String name,
      int entropyLen,
      int? birthday,
    })>((ref) {
  return ({required String name, int entropyLen = 256, int? birthday}) async {
    final walletId = await FfiBridge.createWallet(
      name: name,
      entropyLen: entropyLen,
      birthday: birthday,
    );

    // Set as active
    unawaited(ref.read(activeWalletProvider.notifier).setActiveWallet(walletId));

    // Refresh wallets list
    ref.read(refreshWalletsProvider)();

    return walletId;
  };
});

/// Restore wallet from mnemonic
final restoreWalletProvider = Provider<
    Future<WalletId> Function({
      required String name,
      required String mnemonic,
      String? passphrase,
      int? birthday,
    })>((ref) {
  return ({
    required String name,
    required String mnemonic,
    String? passphrase,
    int? birthday,
  }) async {
    final walletId = await FfiBridge.restoreWallet(
      name: name,
      mnemonic: mnemonic,
      passphrase: passphrase,
      birthday: birthday,
    );

    // Set as active
    unawaited(ref.read(activeWalletProvider.notifier).setActiveWallet(walletId));

    // Refresh wallets list
    ref.read(refreshWalletsProvider)();

    return walletId;
  };
});

/// Import viewing key for watch-only wallet
final importIvkProvider = Provider<
    Future<WalletId> Function({
      required String name,
      String? saplingIvk,
      String? orchardIvk,
      required int birthday,
    })>((ref) {
  return ({
    required String name,
    String? saplingIvk,
    String? orchardIvk,
    required int birthday,
  }) async {
    final walletId = await FfiBridge.importIvk(
      name: name,
      saplingIvk: saplingIvk,
      orchardIvk: orchardIvk,
      birthday: birthday,
    );

    // Set as active
    unawaited(ref.read(activeWalletProvider.notifier).setActiveWallet(walletId));

    // Refresh wallets list
    ref.read(refreshWalletsProvider)();

    return walletId;
  };
});

/// Check if active wallet is watch-only
final isWatchOnlyProvider = FutureProvider<bool>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return false;

  final capabilities = await FfiBridge.getWatchOnlyCapabilities(walletId);
  return capabilities.isWatchOnly;
});

/// Watch-only banner info for active wallet
final watchOnlyBannerProvider =
    FutureProvider<api.WatchOnlyBannerInfo?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return null;

  return FfiBridge.getWatchOnlyBanner(walletId);
});

// ============================================================================
// Balance
// ============================================================================

/// Wallet balance (requires active wallet)
final balanceProvider = FutureProvider<Balance?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return null;

  return FfiBridge.getBalance(walletId);
});

/// Balance stream
final balanceStreamProvider = StreamProvider<Balance?>((ref) async* {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) {
    yield null;
    return;
  }

  yield* FfiBridge.balanceStream(walletId).distinct();
});

// ============================================================================
// Addresses
// ============================================================================

/// Current receive address
final currentAddressProvider = FutureProvider<String?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return null;

  return FfiBridge.currentReceiveAddress(walletId);
});

/// All addresses with labels
final addressesProvider = FutureProvider<List<AddressInfo>>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return [];

  return FfiBridge.listAddresses(walletId);
});

/// Generate next address
final generateAddressProvider = Provider<Future<String> Function()>((ref) {
  return () async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    final address = await FfiBridge.nextReceiveAddress(walletId);

    // Refresh addresses list
    ref
      ..invalidate(addressesProvider)
      ..invalidate(currentAddressProvider);

    return address;
  };
});

// ============================================================================
// Sync
// ============================================================================

/// Sync status (polling)
final syncStatusProvider = FutureProvider<SyncStatus?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return null;

  return FfiBridge.syncStatus(walletId);
});

/// Sync progress stream
final syncProgressStreamProvider = StreamProvider<SyncStatus?>((ref) async* {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) {
    yield null;
    return;
  }

  final isDecoy = ref.watch(decoyModeProvider);
  SyncStatus? lastStatus;
  await for (final status in FfiBridge.syncProgressStream(walletId)) {
    if (lastStatus != null && status == lastStatus) {
      continue;
    }
    lastStatus = status;
    if (!isDecoy && status.targetHeight > BigInt.zero) {
      unawaited(SyncStatusCache.update(status.targetHeight.toInt()));
    }
    yield status;
  }
});

final decoySyncHeightProvider = FutureProvider<int>((ref) async {
  return SyncStatusCache.read();
});

/// Start sync
final startSyncProvider = Provider<Future<void> Function(SyncMode mode)>((ref) {
  return (SyncMode mode) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    await FfiBridge.startSync(walletId, mode);

    // Refresh sync status
    ref.invalidate(syncStatusProvider);
  };
});

/// Rescan from height
final rescanProvider = Provider<Future<void> Function(int fromHeight)>((ref) {
  return (int fromHeight) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    // Kick sync status/progress listeners immediately so UI reflects rescan start.
    ref
      ..invalidate(syncStatusProvider)
      ..invalidate(syncProgressStreamProvider);

    await FfiBridge.rescan(walletId, fromHeight);

    // Refresh sync status/progress and clear cached transaction state after rescan.
    ref
      ..invalidate(syncStatusProvider)
      ..invalidate(syncProgressStreamProvider)
      ..invalidate(transactionsProvider)
      ..invalidate(balanceProvider)
      ..invalidate(transactionStreamProvider);
  };
});

/// Cancel ongoing sync
final cancelSyncProvider = Provider<Future<void> Function()>((ref) {
  return () async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    await FfiBridge.cancelSync(walletId);

    // Refresh sync status
    ref.invalidate(syncStatusProvider);
  };
});

/// Check if sync is running
final isSyncRunningProvider = FutureProvider<bool>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return false;

  return FfiBridge.isSyncRunning(walletId);
});

/// Get last checkpoint info for diagnostics
final lastCheckpointProvider = FutureProvider<api.CheckpointInfo?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return null;

  return FfiBridge.getLastCheckpoint(walletId);
});

/// Get sync logs for diagnostics (last 200 entries)
final syncLogsProvider = FutureProvider<List<SyncLogEntryFfi>>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return [];

  return FfiBridge.getSyncLogs(walletId, limit: 200);
});

/// Refresh sync logs
final refreshSyncLogsProvider = Provider<void Function()>((ref) {
  return () {
    ref.invalidate(syncLogsProvider);
  };
});

// ============================================================================
// Transactions
// ============================================================================

/// Transaction history
final transactionsProvider = FutureProvider<List<TxInfo>>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return [];

  final transactions = await FfiBridge.listTransactions(walletId);
  _prefetchRecentMemos(ref, walletId, transactions);
  return transactions;
});

const int _memoPrefetchLimit = 100;
const int _memoPrefetchConcurrency = 3;
final Set<String> _memoPrefetchInFlight = <String>{};
final Set<String> _memoPrefetchComplete = <String>{};

String _memoPrefetchKey(WalletId walletId, String txid) {
  return '$walletId:$txid';
}

void _prefetchRecentMemos(
  Ref ref,
  WalletId walletId,
  List<TxInfo> transactions,
) {
  final candidates = transactions
      .where((tx) => tx.memo == null || tx.memo!.isEmpty)
      .take(_memoPrefetchLimit)
      .toList();
  if (candidates.isEmpty) {
    return;
  }

  final pending = <String>[];
  for (final tx in candidates) {
    final key = _memoPrefetchKey(walletId, tx.txid);
    if (_memoPrefetchComplete.contains(key) ||
        _memoPrefetchInFlight.contains(key)) {
      continue;
    }
    _memoPrefetchInFlight.add(key);
    pending.add(tx.txid);
  }
  if (pending.isEmpty) {
    return;
  }

  // Best-effort memo prefetch so memo badges can show without opening details.
  unawaited(() async {
    var memoFetched = false;
    for (var i = 0; i < pending.length; i += _memoPrefetchConcurrency) {
      final chunk = pending.sublist(
        i,
        i + _memoPrefetchConcurrency > pending.length
            ? pending.length
            : i + _memoPrefetchConcurrency,
      );
      final results = await Future.wait(
        chunk.map((txid) async {
          final key = _memoPrefetchKey(walletId, txid);
          try {
            final memo = await FfiBridge.fetchTransactionMemo(
              walletId: walletId,
              txid: txid,
            );
            _memoPrefetchComplete.add(key);
            return memo != null && memo.isNotEmpty;
          } catch (_) {
            // Ignore failures; memo can still be fetched when opening details.
            return false;
          } finally {
            _memoPrefetchInFlight.remove(key);
          }
        }),
      );
      if (results.any((fetched) => fetched)) {
        memoFetched = true;
      }
    }
    if (memoFetched) {
      ref.invalidate(transactionsProvider);
    }
  }());
}

/// Transaction stream (new transactions)
final transactionStreamProvider = StreamProvider<TxInfo?>((ref) async* {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) {
    yield null;
    return;
  }

  yield* FfiBridge.transactionStream(walletId).distinct(
        (prev, next) => prev.txid == next.txid,
      );
});

/// Watch for new transactions and refresh dependent providers.
final transactionWatcherProvider = Provider<void>((ref) {
  ref.listen<AsyncValue<TxInfo?>>(transactionStreamProvider, (_, next) {
    final tx = next.asData?.value;
    if (tx != null) {
      ref
        ..invalidate(transactionsProvider)
        ..invalidate(balanceProvider);
    }
  });
});

/// Refresh transactions/balance when a sync run completes (e.g., after rescan).
final syncCompletionWatcherProvider = Provider<void>((ref) {
  ref.watch(activeWalletProvider);
  var wasSyncing = false;
  ref.listen<AsyncValue<SyncStatus?>>(syncProgressStreamProvider, (_, next) {
    next.whenData((status) {
      final isSyncing = status?.isSyncing ?? false;
      if (wasSyncing && !isSyncing) {
        ref
          ..invalidate(transactionsProvider)
          ..invalidate(balanceProvider);
      }
      wasSyncing = isSyncing;
    });
  });
});

/// Build transaction
final buildTransactionProvider = Provider<
    Future<PendingTx> Function({
      required List<Output> outputs,
      int? fee,
    })>((ref) {
  return ({required List<Output> outputs, int? fee}) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    return FfiBridge.buildTx(
      walletId: walletId,
      outputs: outputs,
      fee: fee,
    );
  };
});

/// Sign and broadcast transaction
final sendTransactionProvider =
    Provider<Future<String> Function(PendingTx)>((ref) {
  return (PendingTx pending) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    final signed = await FfiBridge.signTx(walletId, pending);
    final txid = await FfiBridge.broadcastTx(signed);

    // Refresh transactions and balance
    ref
      ..invalidate(transactionsProvider)
      ..invalidate(balanceProvider);

    return txid;
  };
});

// ============================================================================
// Network & Settings
// ============================================================================

/// Network tunnel mode
final tunnelModeProvider = NotifierProvider<TunnelModeNotifier, TunnelMode>(
  TunnelModeNotifier.new,
);

class TunnelModeNotifier extends Notifier<TunnelMode> {
  @override
  TunnelMode build() {
    _loadTunnelMode();
    return const TunnelMode.tor();
  }

  Future<void> _loadTunnelMode() async {
    state = await FfiBridge.getTunnel();
  }

  Future<void> setTunnelMode(TunnelMode mode, {String? socksUrl}) async {
    await FfiBridge.setTunnel(mode, socksUrl: socksUrl);
    state = mode;
  }

  Future<void> setTor() => setTunnelMode(const TunnelMode.tor());

  Future<void> setI2p() => setTunnelMode(const TunnelMode.i2P());

  Future<void> setDirect() => setTunnelMode(const TunnelMode.direct());

  Future<void> setSocks5(String url) =>
      setTunnelMode(TunnelMode.socks5(url: url), socksUrl: url);
}

/// Lightwalletd endpoint URL
final lightdEndpointProvider = FutureProvider<String?>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return FfiBridge.defaultLightdUrl;

  return FfiBridge.getLightdEndpoint(walletId);
});

/// Lightwalletd endpoint configuration (full config with TLS pin)
final lightdEndpointConfigProvider =
    FutureProvider<LightdEndpointConfig>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) {
    return LightdEndpointConfig(url: FfiBridge.defaultLightdUrl);
  }

  return FfiBridge.getLightdEndpointConfig(walletId);
});

/// Set lightwalletd endpoint
final setLightdEndpointProvider = Provider<
    Future<void> Function({
      required String url,
      String? tlsPin,
    })>((ref) {
  return ({required String url, String? tlsPin}) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) throw Exception('No active wallet');

    await FfiBridge.setLightdEndpoint(
      walletId: walletId,
      url: url,
      tlsPin: tlsPin,
    );

    ref
      ..invalidate(lightdEndpointProvider)
      ..invalidate(lightdEndpointConfigProvider);
  };
});

/// Network info
final networkInfoProvider = FutureProvider<NetworkInfo>((ref) async {
  return FfiBridge.getNetworkInfo();
});

// ============================================================================
// Auto Consolidation
// ============================================================================

/// Auto-consolidation enabled flag for active wallet
final autoConsolidationEnabledProvider = FutureProvider<bool>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return false;

  return FfiBridge.getAutoConsolidationEnabled(walletId);
});

/// Number of eligible notes for auto-consolidation
final autoConsolidationCandidateCountProvider =
    FutureProvider<int>((ref) async {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return 0;

  return FfiBridge.getAutoConsolidationCandidateCount(walletId);
});

/// Auto-consolidation threshold
final autoConsolidationThresholdProvider = FutureProvider<int>((ref) async {
  return FfiBridge.getAutoConsolidationThreshold();
});

// ============================================================================
// Utilities
// ============================================================================

/// Generate mnemonic
final generateMnemonicProvider =
    Provider<Future<String> Function({int wordCount})>((ref) {
  return ({int wordCount = 24}) async {
    return FfiBridge.generateMnemonic(wordCount: wordCount);
  };
});

/// Validate mnemonic
final validateMnemonicProvider = Provider<Future<bool> Function(String)>((ref) {
  return (String mnemonic) async {
    return FfiBridge.validateMnemonic(mnemonic);
  };
});

/// Format amount (arrrtoshis to ARRR string)
final formatAmountProvider = Provider<Future<String> Function(int)>((ref) {
  return (int arrrtoshis) async {
    return FfiBridge.formatAmount(arrrtoshis);
  };
});

/// Parse amount (ARRR string to arrrtoshis)
final parseAmountProvider = Provider<Future<int> Function(String)>((ref) {
  return (String arrr) async {
    return FfiBridge.parseAmount(arrr);
  };
});

// ============================================================================
// App Unlock & Wallet Existence
// ============================================================================

/// Check if any wallets exist
///
/// First checks if the wallet registry database file exists without opening it.
/// Only tries to list wallets if the file exists (to avoid creating the database).
/// This allows checking wallet existence before the app is unlocked.
final walletsExistProvider = FutureProvider<bool>((ref) async {
  await ref.watch(rustInitProvider.future);
  try {
    // First, check if the database file exists (doesn't create it)
    final fileExists = await FfiBridge.walletRegistryExists();

    if (!fileExists) {
      // File doesn't exist - no wallets created yet
      return false;
    }

    // File exists - try to list wallets (will work if app is unlocked)
    try {
      final wallets = await FfiBridge.listWallets();
      return wallets.isNotEmpty;
    } catch (e) {
      // File exists but can't be opened - likely wrong passphrase or locked
      // Since file exists, wallets were likely created
      return true;
    }
  } catch (e) {
    // If file existence check fails, assume no wallets
    return false;
  }
});

/// Check if app passphrase is configured
final hasAppPassphraseProvider = FutureProvider<bool>((ref) async {
  try {
    await ref.watch(rustInitProvider.future);
    return await FfiBridge.hasAppPassphrase();
  } catch (e) {
    return false;
  }
});

/// App unlock state (true if unlocked, false if locked)
final appUnlockedProvider =
    NotifierProvider<AppUnlockedNotifier, bool>(AppUnlockedNotifier.new);

class AppUnlockedNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  bool get unlocked => state;
  set unlocked(bool value) => state = value;
}

final decoyModeProvider =
    NotifierProvider<DecoyModeNotifier, bool>(DecoyModeNotifier.new);

class DecoyModeNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  bool get enabled => state;
  set enabled(bool value) => state = value;
}

/// Verify and unlock app with passphrase
final unlockAppProvider = Provider<Future<void> Function(String)>((ref) {
  return (String passphrase) async {
    final isValid = await FfiBridge.verifyAppPassphrase(passphrase);
    if (isValid) {
      await FfiBridge.unlockApp(passphrase);
      ref.read(decoyModeProvider.notifier).enabled = false;
      ref.read(appUnlockedProvider.notifier).unlocked = true;
      // Refresh wallet list after unlock
      ref.invalidate(activeWalletProvider);
      ref.invalidate(walletsExistProvider);
      unawaited(
        BirthdayUpdateService.resumePendingUpdates(
          onWalletsUpdated: ref.read(refreshWalletsProvider),
        ),
      );
      return;
    }

    final duressHash = await DuressPassphraseStore.read();
    if (duressHash != null && duressHash.isNotEmpty) {
      final isDuress = await FfiBridge.verifyDuressPassphrase(
        passphrase: passphrase,
        hash: duressHash,
      );
      if (isDuress) {
        ref.read(decoyModeProvider.notifier).enabled = true;
        ref.read(appUnlockedProvider.notifier).unlocked = true;
        ref.invalidate(activeWalletProvider);
        ref.invalidate(walletsExistProvider);
        return;
      }
    }

    throw Exception('Invalid passphrase');
  };
});
