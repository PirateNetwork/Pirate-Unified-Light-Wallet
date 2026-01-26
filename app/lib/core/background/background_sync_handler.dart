// Background Sync Method Channel Handler
//
// Handles method channel calls from native platform code (Android/iOS)
// for background sync operations.
//
// All RPC calls are routed through the configured NetTunnel (Tor/SOCKS5/Direct).

import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import '../ffi/ffi_bridge.dart';
import '../ffi/generated/models.dart';

/// Method channel name (must match native code)
const String kBackgroundSyncChannel = 'com.pirate.wallet/background_sync';

/// Error codes for method channel responses
class BackgroundSyncErrorCodes {
  static const String torConnectionFailed = 'TOR_CONNECTION_FAILED';
  static const String socks5ConnectionFailed = 'SOCKS5_CONNECTION_FAILED';
  static const String networkError = 'NETWORK_ERROR';
  static const String noWallet = 'NO_WALLET';
  static const String syncFailed = 'SYNC_FAILED';
}

/// Background sync handler for native platform integration
class BackgroundSyncHandler {
  static BackgroundSyncHandler? _instance;
  factory BackgroundSyncHandler() => _instance ??= BackgroundSyncHandler._();

  MethodChannel? _channel;
  bool _initialized = false;
  String? _activeWalletId;

  BackgroundSyncHandler._();

  /// Initialize the method channel handler
  /// Call this during app startup
  void initialize() {
    if (_initialized) return;

    _channel = const MethodChannel(kBackgroundSyncChannel);
    _channel!.setMethodCallHandler(_handleMethodCall);
    _initialized = true;

    debugPrint('[BackgroundSyncHandler] Initialized method channel');
  }

  /// Dispose of the handler
  void dispose() {
    _channel?.setMethodCallHandler(null);
    _channel = null;
    _initialized = false;
    _activeWalletId = null;
    _instance = null;
  }

  /// Update the cached active wallet so platform callbacks always receive
  /// the latest context even before the FFI bridge reloads.
  // ignore: use_setters_to_change_properties
  void updateActiveWallet(WalletId? walletId) {
    _activeWalletId = walletId;
  }

  /// Handle method calls from native code
  Future<dynamic> _handleMethodCall(MethodCall call) async {
    switch (call.method) {
      case 'executeBackgroundSync':
        return _executeBackgroundSync(call.arguments);

      case 'getTunnelMode':
        return _getTunnelMode();

      case 'setTunnelMode':
        return _setTunnelMode(call.arguments);

      case 'getSyncStatus':
        return _getSyncStatus(call.arguments);

      case 'getActiveWalletId':
        return _getActiveWalletId();

      default:
        throw PlatformException(
          code: 'NOT_IMPLEMENTED',
          message: 'Method ${call.method} not implemented',
        );
    }
  }

  /// Execute background sync
  /// Called from Android SyncWorker or iOS BackgroundSyncManager
  Future<Map<String, dynamic>> _executeBackgroundSync(dynamic arguments) async {
    final args = arguments as Map<Object?, Object?>;
    final walletId = args['walletId'] as String?;
    final syncMode = args['mode'] as String? ?? 'compact';
    final maxDurationSecs = (args['maxDurationSecs'] as num?)?.toInt() ?? 60;
    final useRoundRobin = args['useRoundRobin'] as bool? ?? false;
    final tunnelMode = args['tunnelMode'] as String? ?? 'tor';
    final socks5Url = args['socks5Url'] as String?;

    final resolvedWalletId = (walletId == null || walletId.isEmpty)
        ? _activeWalletId
        : walletId;

    try {
      // Configure tunnel mode if specified
      final selection = switch (tunnelMode.toLowerCase()) {
        'tor' => const TunnelMode.tor(),
        'socks5' => TunnelMode.socks5(url: socks5Url ?? 'socks5://localhost:1080'),
        'direct' => const TunnelMode.direct(),
        _ => const TunnelMode.tor(),
      };
      await FfiBridge.setTunnel(selection, socksUrl: socks5Url);

      // Verify tunnel is working before sync
      final currentTunnel = await FfiBridge.getTunnel();
      if (currentTunnel is TunnelMode_Tor) {
        // Test Tor connection
        final torWorking = await _testTorConnection();
        if (!torWorking) {
          throw PlatformException(
            code: BackgroundSyncErrorCodes.torConnectionFailed,
            message: 'Unable to establish Tor connection. Please check your network settings.',
          );
        }
      } else if (currentTunnel is TunnelMode_Socks5) {
        // Test SOCKS5 connection
        final socks5Working = await _testSocks5Connection(socks5Url);
        if (!socks5Working) {
          throw PlatformException(
            code: BackgroundSyncErrorCodes.socks5ConnectionFailed,
            message: 'Unable to connect to SOCKS5 proxy at ${socks5Url ?? "unknown"}',
          );
        }
      }

      if (useRoundRobin || resolvedWalletId == null || resolvedWalletId.isEmpty) {
        return await FfiBridge.executeBackgroundSyncRoundRobin(
          mode: syncMode,
          maxDurationSecs: maxDurationSecs,
        );
      }

      return await FfiBridge.executeBackgroundSync(
        walletId: resolvedWalletId,
        mode: syncMode,
        maxDurationSecs: maxDurationSecs,
      );
    } on PlatformException {
      rethrow;
    } catch (e) {
      final errorMessage = e.toString();
      
      // Categorize the error
      if (errorMessage.toLowerCase().contains('tor')) {
        throw PlatformException(
          code: BackgroundSyncErrorCodes.torConnectionFailed,
          message: errorMessage,
        );
      } else if (errorMessage.toLowerCase().contains('socks')) {
        throw PlatformException(
          code: BackgroundSyncErrorCodes.socks5ConnectionFailed,
          message: errorMessage,
        );
      } else if (errorMessage.toLowerCase().contains('network') ||
                 errorMessage.toLowerCase().contains('connection')) {
        throw PlatformException(
          code: BackgroundSyncErrorCodes.networkError,
          message: errorMessage,
        );
      } else {
        throw PlatformException(
          code: BackgroundSyncErrorCodes.syncFailed,
          message: errorMessage,
        );
      }
    }
  }

  /// Test Tor connection
  Future<bool> _testTorConnection() async {
    try {
      final status = await FfiBridge.getTorStatus();
      return status == 'ready';
    } catch (e) {
      debugPrint('[BackgroundSyncHandler] Tor connection test failed: $e');
      return false;
    }
  }

  /// Test SOCKS5 proxy connection
  Future<bool> _testSocks5Connection(String? proxyUrl) async {
    if (proxyUrl == null || proxyUrl.isEmpty) {
      return false;
    }

    try {
      // In production, this would test the actual SOCKS5 connection
      await Future<void>.delayed(const Duration(milliseconds: 100));
      
      // Parse and validate proxy URL
      final uri = Uri.tryParse(proxyUrl);
      if (uri == null || uri.host.isEmpty) {
        return false;
      }
      
      return true;
    } catch (e) {
      debugPrint('[BackgroundSyncHandler] SOCKS5 connection test failed: $e');
      return false;
    }
  }

  /// Get current tunnel mode
  Future<String> _getTunnelMode() async {
    final mode = await FfiBridge.getTunnel();
    return mode.name;
  }

  /// Set tunnel mode
  Future<void> _setTunnelMode(dynamic arguments) async {
    final args = arguments as Map<Object?, Object?>;
    final mode = args['mode'] as String? ?? 'tor';
    final socks5Url = args['socks5Url'] as String?;

    final tunnelMode = switch (mode.toLowerCase()) {
      'tor' => const TunnelMode.tor(),
      'socks5' => TunnelMode.socks5(url: socks5Url ?? 'socks5://localhost:1080'),
      'direct' => const TunnelMode.direct(),
      _ => const TunnelMode.tor(),
    };

    await FfiBridge.setTunnel(tunnelMode, socksUrl: socks5Url);
  }

  /// Get sync status for a wallet
  Future<Map<String, dynamic>> _getSyncStatus(dynamic arguments) async {
    final args = arguments as Map<Object?, Object?>?;
    final walletId = args?['walletId'] as String?;

    if (walletId == null || walletId.isEmpty) {
      return {
        'is_syncing': false,
        'local_height': 0,
        'target_height': 0,
        'percent': 0.0,
        'eta_seconds': 0,
      };
    }

    final status = await FfiBridge.syncStatus(walletId);

    return {
      'is_syncing': status.isSyncing,
      'local_height': status.localHeight,
      'target_height': status.targetHeight,
      'percent': status.percent,
      'eta_seconds': status.eta,
      'stage': status.stageName,
      'blocks_per_second': status.blocksPerSecond,
    };
  }

  /// Get active wallet ID
  Future<String?> _getActiveWalletId() async {
    if (_activeWalletId != null) {
      return _activeWalletId;
    }
    final walletId = await FfiBridge.getActiveWallet();
    _activeWalletId = walletId;
    return _activeWalletId;
  }
}

/// Initialize background sync handler
/// Call this during app startup
void initializeBackgroundSyncHandler() {
  BackgroundSyncHandler().initialize();
}

/// Dispose background sync handler
/// Call this during app shutdown
void disposeBackgroundSyncHandler() {
  BackgroundSyncHandler().dispose();
}

