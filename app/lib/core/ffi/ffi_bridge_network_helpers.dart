// ignore_for_file: cascade_invocations, use_if_null_to_convert_nulls_to_bools

part of 'ffi_bridge.dart';

class TorStatusDetails {
  final String status;
  final int? progress;
  final String? blocked;
  final String? error;

  const TorStatusDetails({
    required this.status,
    this.progress,
    this.blocked,
    this.error,
  });

  bool get isReady => status == 'ready';

  factory TorStatusDetails.fromRaw(String raw) {
    final trimmed = raw.trim();
    if (trimmed.startsWith('{')) {
      try {
        final decoded = jsonDecode(trimmed);
        if (decoded is Map<String, dynamic>) {
          final status = decoded['status'] as String? ?? trimmed;
          final rawProgress = decoded['progress'];
          final progress = rawProgress is num ? rawProgress.toInt() : null;
          final blocked = decoded['blocked'] as String?;
          final error = decoded['error'] as String?;
          return TorStatusDetails(
            status: status,
            progress: progress,
            blocked: blocked,
            error: error,
          );
        }
      } catch (_) {}
    }
    return TorStatusDetails(status: trimmed);
  }
}

// ============================================================================
// EXCEPTIONS
// ============================================================================

/// Base exception for network tunnel errors
class NetworkTunnelException implements Exception {
  final String message;
  const NetworkTunnelException(this.message);

  @override
  String toString() => 'NetworkTunnelException: $message';
}

/// Exception thrown when Tor connection fails
class TorConnectionException extends NetworkTunnelException {
  const TorConnectionException(super.message);

  @override
  String toString() => 'TorConnectionException: $message';
}

/// Exception thrown when SOCKS5 proxy connection fails
class Socks5ConnectionException extends NetworkTunnelException {
  const Socks5ConnectionException(super.message);

  @override
  String toString() => 'Socks5ConnectionException: $message';
}

/// Exception thrown when TLS pin verification fails
class TlsPinMismatchException implements Exception {
  final String expectedPin;
  final String actualPin;

  const TlsPinMismatchException({
    required this.expectedPin,
    required this.actualPin,
  });

  @override
  String toString() =>
      'TlsPinMismatchException: Expected $expectedPin, got $actualPin';
}

/// Result of testing node connection
class NodeTestResult {
  /// Whether the connection was successful
  final bool success;

  /// Latest block height from the node
  final int? latestBlockHeight;

  /// Transport mode used (Tor/SOCKS5/Direct)
  final String transportMode;

  /// Whether TLS was used
  final bool tlsEnabled;

  /// Whether the TLS pin matched (null if no pin was set)
  final bool? tlsPinMatched;

  /// The SPKI pin that was expected (if set)
  final String? expectedPin;

  /// The actual SPKI pin from the server (if TLS was used)
  final String? actualPin;

  /// Error message if connection failed
  final String? errorMessage;

  /// Response time in milliseconds
  final int responseTimeMs;

  /// Server version info (if available)
  final String? serverVersion;

  /// Chain name from server
  final String? chainName;

  NodeTestResult({
    required this.success,
    this.latestBlockHeight,
    required this.transportMode,
    required this.tlsEnabled,
    this.tlsPinMatched,
    this.expectedPin,
    this.actualPin,
    this.errorMessage,
    required this.responseTimeMs,
    this.serverVersion,
    this.chainName,
  });

  /// Get a human-readable status message
  String get statusMessage {
    if (!success) {
      return errorMessage ?? 'Connection failed';
    }

    final parts = <String>[];
    parts.add('Connected via $transportMode');
    if (tlsEnabled) {
      parts.add('TLS enabled');
      if (tlsPinMatched == true) {
        parts.add('Pin verified (OK)');
      } else if (tlsPinMatched == false) {
        parts.add('Pin MISMATCH');
      }
    }
    if (latestBlockHeight != null) {
      parts.add('Block #$latestBlockHeight');
    }
    return parts.join(' - ');
  }

  /// Get transport icon name
  String get transportIcon {
    switch (transportMode.toLowerCase()) {
      case 'tor':
        return '🧅';
      case 'i2p':
        return 'dYO?';
      case 'socks5':
        return '🔌';
      case 'direct':
        return '⚡';
      default:
        return '🌐';
    }
  }
}

class _FfiBridgeNetworkHelper {
  static Future<NodeTestResult> testNode({
    required String url,
    String? tlsPin,
  }) async {
    final startTime = DateTime.now();
    final tunnelMode = await FfiBridge.getTunnel();
    final transportName = tunnelMode.name;
    final useTls = _usesTls(url);

    try {
      // Note: This function needs to be implemented in Rust FFI layer.
      // The actual connection should be established via the configured
      // tunnel (Tor/SOCKS5/Direct) and should:
      // 1. Establish connection via configured tunnel
      // 2. Perform TLS handshake and extract server certificate SPKI
      // 3. Compare SPKI with provided pin (if any)
      // 4. Call get_latest_block() gRPC method
      if (kUseFrbBindings) {
        final result = await api.testNode(url: url, tlsPin: tlsPin);
        return NodeTestResult(
          success: result.success,
          latestBlockHeight: result.latestBlockHeight?.toInt(),
          transportMode: result.transportMode,
          tlsEnabled: result.tlsEnabled,
          tlsPinMatched: result.tlsPinMatched,
          expectedPin: result.expectedPin,
          actualPin: result.actualPin,
          errorMessage: result.errorMessage,
          responseTimeMs: result.responseTimeMs.toInt(),
          serverVersion: result.serverVersion,
          chainName: result.chainName,
        );
      }

      throw UnimplementedError('FRB bindings not available');
    } on TorConnectionException catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage:
            'Tor connection failed: ${e.message}\n\n'
            'Please check your Tor settings or try a different transport mode.',
        responseTimeMs: _elapsedMs(startTime),
      );
    } on Socks5ConnectionException catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage:
            'SOCKS5 proxy connection failed: ${e.message}\n\n'
            'Please verify your proxy settings.',
        responseTimeMs: _elapsedMs(startTime),
      );
    } catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage: 'Connection failed: $e',
        responseTimeMs: _elapsedMs(startTime),
      );
    }
  }

  static Future<bool> isTunnelReadyForSync(TunnelMode mode) async {
    if (mode is TunnelMode_Tor) {
      try {
        final status = await FfiBridge.getTorStatusDetails();
        if (status.status == 'ready') return true;
        // Allow sync to proceed while Tor is actively bootstrapping; the sync
        // engine already retries network operations until the transport is up.
        if (status.status == 'bootstrapping') {
          final blocked = status.blocked?.trim();
          return blocked == null || blocked.isEmpty;
        }
        return false;
      } catch (_) {
        return false;
      }
    }
    if (mode is TunnelMode_Socks5) {
      return mode.url.trim().isNotEmpty;
    }
    return true;
  }

  static bool _usesTls(String url) {
    return url.startsWith('https://') ||
        (!url.startsWith('http://') && !url.contains('://'));
  }

  static int _elapsedMs(DateTime startTime) {
    return DateTime.now().difference(startTime).inMilliseconds;
  }
}
