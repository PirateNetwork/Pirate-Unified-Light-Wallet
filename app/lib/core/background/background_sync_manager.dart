import 'dart:async';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'background_sync_execution_result.dart';
import '../ffi/ffi_bridge.dart';

/// Background sync configuration
class BackgroundSyncConfig {
  /// Compact sync interval (minutes) - Android WorkManager / iOS BGAppRefreshTask
  final int compactIntervalMinutes;

  /// Deep sync interval (hours) - Android WorkManager / iOS BGProcessingTask
  final int deepIntervalHours;

  /// Use foreground service for long operations (Android)
  final bool useForegroundService;

  /// Show notification on received funds
  final bool notifyOnReceive;

  /// Maximum compact sync duration (seconds)
  final int maxCompactDurationSecs;

  /// Maximum deep sync duration (seconds)
  final int maxDeepDurationSecs;

  const BackgroundSyncConfig({
    this.compactIntervalMinutes = 24 * 60,
    this.deepIntervalHours = 24,
    this.useForegroundService = true,
    this.notifyOnReceive = true,
    this.maxCompactDurationSecs = 90,
    this.maxDeepDurationSecs = 600,
  });

  Map<String, dynamic> toMap() => {
    'compactIntervalMinutes': compactIntervalMinutes,
    'deepIntervalHours': deepIntervalHours,
    'useForegroundService': useForegroundService,
    'notifyOnReceive': notifyOnReceive,
    'maxCompactDurationSecs': maxCompactDurationSecs,
    'maxDeepDurationSecs': maxDeepDurationSecs,
  };
}

/// Background sync result
class BackgroundSyncResult {
  final String mode;
  final int blocksSynced;
  final int durationSecs;
  final int newTransactions;
  final int? newBalance;
  final String tunnelUsed;
  final List<String> errors;

  const BackgroundSyncResult({
    required this.mode,
    required this.blocksSynced,
    required this.durationSecs,
    required this.newTransactions,
    this.newBalance,
    required this.tunnelUsed,
    this.errors = const [],
  });

  factory BackgroundSyncResult.fromMap(Map<String, dynamic> map) {
    return BackgroundSyncResult(
      mode: map['mode'] as String? ?? 'compact',
      blocksSynced: map['blocks_synced'] as int? ?? 0,
      durationSecs: map['duration_secs'] as int? ?? 0,
      newTransactions: map['new_transactions'] as int? ?? 0,
      newBalance: map['new_balance'] as int?,
      tunnelUsed: map['tunnel_used'] as String? ?? 'tor',
      errors: (map['errors'] as List<dynamic>?)?.cast<String>() ?? [],
    );
  }

  factory BackgroundSyncResult.fromExecution(
    BackgroundSyncExecutionResult result,
  ) {
    return BackgroundSyncResult(
      mode: result.mode,
      blocksSynced: result.blocksSynced,
      durationSecs: result.durationSecs,
      newTransactions: result.newTransactions,
      newBalance: result.newBalance,
      tunnelUsed: result.tunnelUsed,
      errors: result.errors,
    );
  }

  bool get hasErrors => errors.isNotEmpty;
  bool get hasNewTransactions => newTransactions > 0;
}

/// Sync status for UI
class SyncStatus {
  final DateTime? lastCompactSync;
  final DateTime? lastDeepSync;
  final String tunnelMode;
  final bool isRunning;
  final String? currentMode;

  const SyncStatus({
    this.lastCompactSync,
    this.lastDeepSync,
    this.tunnelMode = 'tor',
    this.isRunning = false,
    this.currentMode,
  });

  int get minutesSinceCompact {
    if (lastCompactSync == null) return -1;
    return DateTime.now().difference(lastCompactSync!).inMinutes;
  }

  int get hoursSinceDeep {
    if (lastDeepSync == null) return -1;
    return DateTime.now().difference(lastDeepSync!).inHours;
  }

  factory SyncStatus.fromMap(Map<String, dynamic> map) {
    return SyncStatus(
      lastCompactSync: map['last_compact_sync'] != null
          ? DateTime.fromMillisecondsSinceEpoch(map['last_compact_sync'] as int)
          : null,
      lastDeepSync: map['last_deep_sync'] != null
          ? DateTime.fromMillisecondsSinceEpoch(map['last_deep_sync'] as int)
          : null,
      tunnelMode: map['tunnel_mode'] as String? ?? 'tor',
      isRunning: map['is_running'] as bool? ?? false,
      currentMode: map['current_mode'] as String?,
    );
  }
}

/// Network tunnel mode
enum TunnelMode {
  /// Tor (default, maximum privacy)
  tor,

  /// I2P eepsite routing
  i2p,

  /// SOCKS5 proxy
  socks5,

  /// Direct connection (no privacy, not recommended)
  direct,
}

extension TunnelModeExtension on TunnelMode {
  String get name {
    switch (this) {
      case TunnelMode.tor:
        return 'tor';
      case TunnelMode.i2p:
        return 'i2p';
      case TunnelMode.socks5:
        return 'socks5';
      case TunnelMode.direct:
        return 'direct';
    }
  }

  static TunnelMode fromName(String name) {
    switch (name) {
      case 'i2p':
        return TunnelMode.i2p;
      case 'socks5':
        return TunnelMode.socks5;
      case 'direct':
        return TunnelMode.direct;
      default:
        return TunnelMode.tor;
    }
  }
}

/// Platform-agnostic background sync manager
class BackgroundSyncManager {
  static const _channel = MethodChannel('com.pirate.wallet/background_sync');

  final BackgroundSyncConfig config;

  // Stream controller for sync events
  final _syncEventController =
      StreamController<BackgroundSyncResult>.broadcast();

  // Stream controller for status updates
  final _statusController = StreamController<SyncStatus>.broadcast();

  BackgroundSyncManager({BackgroundSyncConfig? config})
    : config = config ?? const BackgroundSyncConfig() {
    // Listen for platform events
    _channel.setMethodCallHandler(_handlePlatformCall);
  }

  /// Stream of sync completion events
  Stream<BackgroundSyncResult> get syncEvents => _syncEventController.stream;

  /// Stream of status updates
  Stream<SyncStatus> get statusUpdates => _statusController.stream;

  /// Handle calls from native platform
  Future<dynamic> _handlePlatformCall(MethodCall call) async {
    switch (call.method) {
      case 'onSyncComplete':
        final result = BackgroundSyncResult.fromMap(
          Map<String, dynamic>.from(call.arguments as Map),
        );
        _syncEventController.add(result);
        debugPrint(
          '[BackgroundSync] Sync complete: ${result.mode}, ${result.blocksSynced} blocks',
        );
        return null;

      case 'onSyncProgress':
        // Handle progress updates if needed
        return null;

      case 'onSyncError':
        debugPrint('[BackgroundSync] Sync error: ${call.arguments}');
        return null;

      default:
        throw PlatformException(
          code: 'UNKNOWN_METHOD',
          message: 'Unknown method: ${call.method}',
        );
    }
  }

  /// Initialize background sync on the platform
  Future<void> initialize() async {
    debugPrint(
      '[BackgroundSync] Initializing for platform: ${Platform.operatingSystem}',
    );

    if (Platform.isAndroid) {
      await _initializeAndroid();
    } else if (Platform.isIOS) {
      await _initializeIOS();
    } else {
      debugPrint(
        '[BackgroundSync] Background sync not supported on ${Platform.operatingSystem}',
      );
    }
  }

  /// Initialize Android WorkManager
  Future<void> _initializeAndroid() async {
    try {
      await _channel.invokeMethod('initializeBackgroundSync', {
        'compactIntervalMinutes': config.compactIntervalMinutes,
        'deepIntervalHours': config.deepIntervalHours,
        'maxCompactDurationSecs': config.maxCompactDurationSecs,
        'maxDeepDurationSecs': config.maxDeepDurationSecs,
        'maxCompactBlocks': 250000,
        'maxDeepBlocks': 5000000,
        'compactFlexMinutes': 60,
        'deepFlexHours': 2,
        'requiresCharging': true,
        'requiresWifi': true,
      });

      debugPrint('[BackgroundSync] Android WorkManager initialized');
      debugPrint('  - Compact sync: every ${config.compactIntervalMinutes}m');
      debugPrint(
        '  - Deep sync: every ${config.deepIntervalHours}h (charging + WiFi)',
      );
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to initialize Android: $e');
    }
  }

  /// Initialize iOS BackgroundTasks
  Future<void> _initializeIOS() async {
    try {
      await _channel.invokeMethod('initializeBackgroundSync', {
        'compactIntervalMinutes': config.compactIntervalMinutes,
        'deepIntervalHours': config.deepIntervalHours,
        'maxCompactDurationSecs': 25,
        'maxDeepDurationSecs': 300,
        'maxCompactBlocks': 250000,
        'maxDeepBlocks': 5000000,
        'requiresCharging': true,
      });

      debugPrint('[BackgroundSync] iOS BackgroundTasks initialized');
      debugPrint(
        '  - Compact sync: BGAppRefreshTask (~${config.compactIntervalMinutes}m)',
      );
      debugPrint('  - Deep sync: BGProcessingTask (daily, charging + network)');
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to initialize iOS: $e');
    }
  }

  /// Execute background sync (called by platform or manually)
  Future<BackgroundSyncResult> executeSync({
    required String walletId,
    required String mode,
    int? maxDurationSecs,
  }) async {
    debugPrint('[BackgroundSync] Executing $mode sync for wallet: $walletId');

    try {
      // Call FFI to perform sync (all RPC via NetTunnel)
      final result = await FfiBridge.executeBackgroundSync(
        walletId: walletId,
        mode: mode,
        maxDurationSecs:
            maxDurationSecs ??
            (mode == 'compact'
                ? config.maxCompactDurationSecs
                : config.maxDeepDurationSecs),
      );

      final syncResult = BackgroundSyncResult.fromExecution(result);
      debugPrint(
        '[BackgroundSync] Sync completed: ${syncResult.blocksSynced} blocks via ${syncResult.tunnelUsed}',
      );

      // Notify listeners
      _syncEventController.add(syncResult);

      return syncResult;
    } catch (e) {
      debugPrint('[BackgroundSync] Sync failed: $e');
      rethrow;
    }
  }

  /// Trigger immediate sync
  Future<void> triggerImmediateSync({String mode = 'compact'}) async {
    debugPrint('[BackgroundSync] Triggering immediate $mode sync');

    try {
      await _channel.invokeMethod('triggerImmediateSync', {
        'mode': mode,
        'maxDurationSecs': mode == 'compact'
            ? config.maxCompactDurationSecs
            : config.maxDeepDurationSecs,
        'maxBlocks': mode == 'compact' ? 250000 : 5000000,
      });
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to trigger immediate sync: $e');
      rethrow;
    }
  }

  /// Set network tunnel mode
  Future<void> setTunnelMode(TunnelMode mode, {String? socks5Url}) async {
    debugPrint('[BackgroundSync] Setting tunnel mode: ${mode.name}');

    try {
      await syncNativeTunnelMode(mode, socks5Url: socks5Url);

      // Also update FFI
      await FfiBridge.setTunnelMode(mode: mode.name, socks5Url: socks5Url);
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to set tunnel mode: $e');
      rethrow;
    }
  }

  Future<void> syncNativeTunnelMode(
    TunnelMode mode, {
    String? socks5Url,
  }) async {
    await _channel.invokeMethod('setTunnelMode', {
      'mode': mode.name,
      'socks5Url': socks5Url,
    });
  }

  Future<void> setActiveWalletId(String? walletId) async {
    try {
      if (walletId == null || walletId.isEmpty) {
        await _channel.invokeMethod('clearActiveWalletId');
      } else {
        await _channel.invokeMethod('setActiveWalletId', {
          'walletId': walletId,
        });
      }
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to set active wallet ID: $e');
    }
  }

  /// Get current tunnel mode
  Future<TunnelMode> getTunnelMode() async {
    try {
      final result = await _channel.invokeMethod<String>('getTunnelMode');
      return TunnelModeExtension.fromName(result ?? 'tor');
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to get tunnel mode: $e');
      return TunnelMode.tor;
    }
  }

  /// Get sync status
  Future<SyncStatus> getSyncStatus() async {
    try {
      final result = await _channel.invokeMethod<Map<String, dynamic>>(
        'getSyncStatus',
      );
      if (result != null) {
        return SyncStatus.fromMap(result);
      }
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to get sync status: $e');
    }
    return const SyncStatus();
  }

  /// Cancel all scheduled background tasks
  Future<void> cancelAll() async {
    try {
      if (Platform.isAndroid) {
        await _channel.invokeMethod('cancelAllSync');
      } else if (Platform.isIOS) {
        await _channel.invokeMethod('cancelAllTasks');
      }

      debugPrint('[BackgroundSync] Cancelled all background tasks');
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to cancel tasks: $e');
    }
  }

  /// Pause background sync (keep scheduled but don't execute)
  Future<void> pause() async {
    try {
      await _channel.invokeMethod('pauseSync');
      debugPrint('[BackgroundSync] Paused background sync');
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to pause sync: $e');
    }
  }

  /// Resume background sync
  Future<void> resume() async {
    try {
      await _channel.invokeMethod('resumeSync');
      debugPrint('[BackgroundSync] Resumed background sync');
    } catch (e) {
      debugPrint('[BackgroundSync] Failed to resume sync: $e');
    }
  }

  /// Check if background sync is enabled
  bool get isEnabled {
    return Platform.isAndroid || Platform.isIOS;
  }

  /// Check if running on Android
  bool get isAndroid => Platform.isAndroid;

  /// Check if running on iOS
  bool get isIOS => Platform.isIOS;

  /// Dispose resources
  void dispose() {
    _syncEventController.close();
    _statusController.close();
  }
}

/// Provider for background sync manager
final backgroundSyncManagerProvider = Provider<BackgroundSyncManager>((ref) {
  final manager = BackgroundSyncManager();
  ref.onDispose(manager.dispose);
  return manager;
});

/// Provider to initialize background sync
final backgroundSyncInitProvider = FutureProvider<void>((ref) async {
  final manager = ref.read(backgroundSyncManagerProvider);
  await manager.initialize();
});

/// Provider for sync status
final syncStatusProvider = FutureProvider<SyncStatus>((ref) async {
  final manager = ref.read(backgroundSyncManagerProvider);
  return manager.getSyncStatus();
});

/// Provider for tunnel mode
final tunnelModeProvider = NotifierProvider<TunnelModeNotifier, TunnelMode>(
  TunnelModeNotifier.new,
);

/// Tunnel mode state notifier
class TunnelModeNotifier extends Notifier<TunnelMode> {
  late final BackgroundSyncManager _manager;

  @override
  TunnelMode build() {
    _manager = ref.read(backgroundSyncManagerProvider);
    _loadTunnelMode();
    return TunnelMode.tor;
  }

  Future<void> _loadTunnelMode() async {
    state = await _manager.getTunnelMode();
  }

  Future<void> setMode(TunnelMode mode, {String? socks5Url}) async {
    await _manager.setTunnelMode(mode, socks5Url: socks5Url);
    state = mode;
  }
}

/// Provider for sync events stream
final syncEventsProvider = StreamProvider<BackgroundSyncResult>((ref) {
  final manager = ref.read(backgroundSyncManagerProvider);
  return manager.syncEvents;
});
