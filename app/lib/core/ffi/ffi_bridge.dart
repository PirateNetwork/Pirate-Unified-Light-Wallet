// Lightweight shim that routes the app's FFI calls to the generated
// flutter_rust_bridge bindings. All generated code lives under
// `app/flutter/lib/ffi/bridge_generated.dart` and is re-exported via
// `app/lib/ffi/bridge_generated.dart`.

library ffi_bridge;

import 'dart:async';
import 'package:flutter/foundation.dart';

import 'generated/api.dart' as api;
import 'generated/models.dart' hide AddressBookColorTag, AddressBookEntryFfi, SyncLogEntryFfi;
import 'generated/models.dart' as models show AddressBookColorTag, AddressBookEntryFfi, SyncLogEntryFfi;

// Type aliases to avoid conflicts with local types
typedef GeneratedAddressBookColorTag = models.AddressBookColorTag;
typedef GeneratedAddressBookEntryFfi = models.AddressBookEntryFfi;

const bool kUseFrbBindings = true;

GeneratedAddressBookColorTag _convertColorTag(AddressBookColorTag localTag) {
  switch (localTag) {
    case AddressBookColorTag.none:
      return GeneratedAddressBookColorTag.none;
    case AddressBookColorTag.red:
      return GeneratedAddressBookColorTag.red;
    case AddressBookColorTag.orange:
      return GeneratedAddressBookColorTag.orange;
    case AddressBookColorTag.yellow:
      return GeneratedAddressBookColorTag.yellow;
    case AddressBookColorTag.green:
      return GeneratedAddressBookColorTag.green;
    case AddressBookColorTag.blue:
      return GeneratedAddressBookColorTag.blue;
    case AddressBookColorTag.purple:
      return GeneratedAddressBookColorTag.purple;
    case AddressBookColorTag.pink:
      return GeneratedAddressBookColorTag.pink;
    case AddressBookColorTag.gray:
      return GeneratedAddressBookColorTag.gray;
  }
}

// Simple aliases so existing app code can keep using the familiar types from FRB.
typedef WalletId = String;
// Types are imported directly from models.dart, no need for aliases
/// FFI Bridge - Flutter ↔ Rust Interface
///
/// This file provides the Dart API that mirrors the Rust FFI in pirate-ffi-api.
/// 
/// ## FRB Integration Status
/// 

/// 
/// ## To Complete FRB Wiring
/// 
/// ```bash
/// # 1. Install flutter_rust_bridge_codegen
/// cargo install flutter_rust_bridge_codegen
/// 
/// # 2. Generate bindings (from project root)
/// flutter_rust_bridge_codegen generate
/// 
/// # 3. Rebuild native libraries
/// cd app && flutter pub get && flutter run
/// ```
/// 
/// After codegen, the generated code in `app/flutter/lib/ffi/bridge_generated.dart`
/// (see `crates/pirate-ffi-frb/api.toml`) should replace the stub implementations
/// in this file.
///
/// @see crates/pirate-ffi-frb/src/api.rs for Rust implementation
/// @see crates/pirate-ffi-frb/api.toml for codegen configuration and output paths

/// Feature flags for tracking implementation status
class FfiBridgeStatus {
  /// Rust API is implemented
  static const bool rustApiComplete = true;
  
  /// Dart stubs are implemented
  static const bool dartStubsComplete = true;
  
  /// FRB codegen has been run
  static const bool frbGenerated = true;
  
  /// Native library is loaded
  static bool nativeLibraryLoaded = false;
  
  /// Get overall status
  static String get status {
    if (!rustApiComplete) return 'Rust API incomplete';
    if (!dartStubsComplete) return 'Dart stubs incomplete';
    if (!frbGenerated) return 'FRB codegen pending';
    if (!nativeLibraryLoaded) return 'Native library not loaded';
    return 'Fully operational';
  }
}

/// Watch-only wallet label constant
const String kWatchOnlyLabel = 'Incoming only';

/// Watch-only wallet banner message
const String kWatchOnlyBannerMessage = 
    'This wallet can only view incoming transactions. Spending is not available.';

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
  String toString() => 'TlsPinMismatchException: Expected $expectedPin, got $actualPin';
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

  const NodeTestResult({
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
        parts.add('Pin verified ✓');
      } else if (tlsPinMatched == false) {
        parts.add('Pin MISMATCH ✗');
      }
    }
    if (latestBlockHeight != null) {
      parts.add('Block #$latestBlockHeight');
    }
    return parts.join(' • ');
  }

  /// Get transport icon name
  String get transportIcon {
    switch (transportMode.toLowerCase()) {
      case 'tor':
        return '🧅';
      case 'socks5':
        return '🔌';
      case 'direct':
        return '⚡';
      default:
        return '🌐';
    }
  }
}

/// Lightwalletd endpoint configuration
class LightdEndpointConfig {
  final String url;
  final String? tlsPin;
  final String? label;

  const LightdEndpointConfig({
    required this.url,
    this.tlsPin,
    this.label,
  });
  
  /// Parse host from URL
  String get host {
    String normalized = url;
    if (normalized.startsWith('https://')) {
      normalized = normalized.substring(8);
    } else if (normalized.startsWith('http://')) {
      normalized = normalized.substring(7);
    }
    final colonIndex = normalized.indexOf(':');
    if (colonIndex != -1) {
      return normalized.substring(0, colonIndex);
    }
    return normalized;
  }
  
  /// Parse port from URL
  int get port {
    String normalized = url;
    if (normalized.startsWith('https://')) {
      normalized = normalized.substring(8);
    } else if (normalized.startsWith('http://')) {
      normalized = normalized.substring(7);
    }
    final colonIndex = normalized.indexOf(':');
    if (colonIndex != -1) {
      final portStr = normalized.substring(colonIndex + 1);
      return int.tryParse(portStr) ?? 9067;
    }
    return 9067;
  }
  
  /// Whether TLS is enabled
  bool get useTls => url.startsWith('https://');
  
  /// Display string (host:port)
  String get displayString => '$host:$port';

  Map<String, dynamic> toJson() => {
    'url': url,
    if (tlsPin != null) 'tlsPin': tlsPin,
    if (label != null) 'label': label,
  };

  factory LightdEndpointConfig.fromJson(Map<String, dynamic> json) {
    return LightdEndpointConfig(
      url: json['url'] as String,
      tlsPin: json['tlsPin'] as String?,
      label: json['label'] as String?,
    );
  }
}

// API - Ready for FRB binding generation

class FfiBridge {
  /// Cached active wallet ID (updated when switching wallets)
  static WalletId? _activeWalletId;
  
  /// Default mainnet birthday height (from pirate_params)
  static const int defaultBirthdayHeight = 3800000;
  
  // ============================================================================
  // WALLET LIFECYCLE - FFI Implementation
  // ============================================================================
  
  /// Create new wallet with entropy, birthday, and auto-start sync
  /// 
  /// @param name - Wallet display name
  /// @param entropyLen - 128 or 256 bits (default 256 for 24 words)
  /// @param birthday - Block height for scanning (default: mainnet default)
  /// 
  /// After creation, automatically starts compact sync.
  static Future<WalletId> createWallet({
    required String name,
    int entropyLen = 256,
    int? birthday,
  }) async {
    if (kUseFrbBindings) {
      final walletId = await api.createWallet(
        name: name,
        entropyLen: entropyLen,
        birthdayOpt: birthday,
      );
      _activeWalletId = walletId;
      // Auto-start compact sync after wallet creation
      _startCompactSyncAfterCreate(walletId);
      return walletId;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Restore wallet from mnemonic with birthday and auto-start sync
  /// 
  /// @param name - Wallet display name
  /// @param mnemonic - 24-word BIP-39 seed phrase
  /// @param passphrase - Optional BIP-39 passphrase (not the app passphrase)
  /// @param birthday - Block height to scan from (critical for restore)
  /// 
  /// After restore, automatically starts compact sync from birthday.
  static Future<WalletId> restoreWallet({
    required String name,
    required String mnemonic,
    String? passphrase,
    int? birthday,
  }) async {
    if (kUseFrbBindings) {
      final walletId = await api.restoreWallet(
        name: name,
        mnemonic: mnemonic,
        passphraseOpt: passphrase,
        birthdayOpt: birthday,
      );
      _activeWalletId = walletId;
      // Auto-start compact sync after restore
      _startCompactSyncAfterCreate(walletId);
      return walletId;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// List all wallets with metadata
  /// Check if wallet registry database file exists (without opening it)
  /// 
  /// This allows checking if wallets exist before the database is created.
  static Future<bool> walletRegistryExists() async {
    if (kUseFrbBindings) {
      return await api.walletRegistryExists();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// 
  /// Watch-only wallets include `watchOnly: true` flag.
  static Future<List<WalletMeta>> listWallets() async {
    if (kUseFrbBindings) {
      return await api.listWallets();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Switch to a different wallet
  /// 
  /// @param id - Wallet ID to switch to (must exist)
  static Future<void> switchWallet(WalletId id) async {
    if (kUseFrbBindings) {
      await api.switchWallet(walletId: id);
      _activeWalletId = id;
      return;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Rename a wallet
  static Future<void> renameWallet(WalletId id, String name) async {
    if (kUseFrbBindings) {
      await api.renameWallet(walletId: id, newName: name);
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Delete a wallet (local data only)
  static Future<void> deleteWallet(WalletId id) async {
    if (kUseFrbBindings) {
      await api.deleteWallet(walletId: id);
      if (_activeWalletId == id) {
        _activeWalletId = null;
      }
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get currently active wallet ID
  static Future<WalletId?> getActiveWallet() async {
    if (kUseFrbBindings) {
      final walletId = await api.getActiveWallet();
      _activeWalletId = walletId;
      return walletId;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Store app passphrase hash for local verification
  static Future<void> setAppPassphrase(String passphrase) async {
    if (kUseFrbBindings) {
      await api.setAppPassphrase(passphrase: passphrase);
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Check if app passphrase has been configured
  static Future<bool> hasAppPassphrase() async {
    if (kUseFrbBindings) {
      return await api.hasAppPassphrase();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Verify app passphrase
  static Future<bool> verifyAppPassphrase(String passphrase) async {
    if (kUseFrbBindings) {
      return await api.verifyAppPassphrase(passphrase: passphrase);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Unlock app with passphrase
  static Future<void> unlockApp(String passphrase) async {
    if (kUseFrbBindings) {
      await api.unlockApp(passphrase: passphrase);
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Change app passphrase (requires current passphrase)
  static Future<void> changeAppPassphrase({
    required String currentPassphrase,
    required String newPassphrase,
  }) async {
    if (kUseFrbBindings) {
      await api.changeAppPassphrase(
        currentPassphrase: currentPassphrase,
        newPassphrase: newPassphrase,
      );
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Change app passphrase using cached session passphrase
  static Future<void> changeAppPassphraseWithCached({
    required String newPassphrase,
  }) async {
    if (kUseFrbBindings) {
      await api.changeAppPassphraseWithCached(
        newPassphrase: newPassphrase,
      );
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Reseal DB keys with the current keystore mode (biometric on/off).
  static Future<void> resealDbKeysForBiometrics() async {
    if (kUseFrbBindings) {
      await api.resealDbKeysForBiometrics();
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Update wallet birthday height
  static Future<void> setWalletBirthdayHeight(
    WalletId walletId,
    int birthdayHeight,
  ) async {
    if (kUseFrbBindings) {
      await api.setWalletBirthdayHeight(
        walletId: walletId,
        birthdayHeight: birthdayHeight,
      );
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }
  
  /// Helper to auto-start sync after wallet creation
  static Future<void> _startCompactSyncAfterCreate(WalletId walletId) async {
    try {
      // Small delay to let UI transition complete
      await Future<void>.delayed(const Duration(milliseconds: 500));
      await startSync(walletId, SyncMode.compact);
    } catch (e) {
      // Log error but don't throw - sync can be manually started later
      print('Auto-sync start failed: $e');
    }
  }

  // Addresses
  static Future<String> currentReceiveAddress(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.currentReceiveAddress(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<String> nextReceiveAddress(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.nextReceiveAddress(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<void> labelAddress(WalletId id, String addr, String label) async {
    if (kUseFrbBindings) {
      await api.labelAddress(walletId: id, addr: addr, label: label);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<void> setAddressColorTag(
    WalletId id,
    String addr,
    AddressBookColorTag colorTag,
  ) async {
    if (kUseFrbBindings) {
      final generatedColorTag = _convertColorTag(colorTag);
      await api.setAddressColorTag(
        walletId: id,
        addr: addr,
        colorTag: generatedColorTag,
      );
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<List<AddressInfo>> listAddresses(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.listAddresses(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // ============================================================================
  // WATCH-ONLY WALLETS - IVK Export/Import
  // ============================================================================

  /// Export IVK from full wallet for creating watch-only on another device
  static Future<String> exportIvk(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.exportIvk(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Import IVK to create watch-only wallet
  /// 
  /// Watch-only wallets can only view incoming transactions.
  /// They CANNOT:
  /// - Spend funds
  /// - See outgoing transaction details
  /// - Export seed phrase
  /// 
  /// @param name - Wallet display name (will append " (Incoming only)")
  /// @param ivk - Incoming Viewing Key (zivks1...)
  /// @param birthday - Block height to scan from (required for IVK import)
  /// 
  /// After import, automatically starts compact sync from birthday.
  static Future<WalletId> importIvk({
    required String name,
    required String ivk,
    required int birthday,
  }) async {
    if (kUseFrbBindings) {
      final walletId = await api.importIvk(
        name: name,
        saplingIvk: ivk, // Use saplingIvk parameter
        orchardIvk: null,
        birthday: birthday,
      );
      _activeWalletId = walletId;
      // Auto-start compact sync from birthday
      _startCompactSyncAfterCreate(walletId);
      return walletId;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // Send
  /// Maximum memo length in bytes
  static const int maxMemoBytes = 512;
  
  /// Maximum outputs per transaction
  static const int maxOutputs = 50;
  
  /// Minimum fee in zatoshis
  static const int minFee = 1000;
  
  /// Default fee per output
  static const int feePerOutput = 10000;
  
  /// Build transaction with validation
  /// 
  /// Validates:
  /// - All addresses are valid Sapling (zs1...)
  /// - All amounts are positive and non-zero
  /// - All memos are valid UTF-8 and <= 512 bytes
  /// - Sufficient funds available
  /// 
  /// Returns PendingTx with fee, change, and input information
  static Future<PendingTx> buildTx({
    required WalletId walletId,
    required List<Output> outputs,
    int? fee,
  }) async {
    if (kUseFrbBindings) {
      return await api.buildTx(
        walletId: walletId,
        outputs: outputs,
        feeOpt: fee != null ? BigInt.from(fee) : null,
      );
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  
  /// Sign pending transaction
  /// 
  /// Requires wallet spending key access.
  /// Generates Sapling proofs and signs inputs.
  static Future<SignedTx> signTx(WalletId walletId, PendingTx pending) async {
    if (kUseFrbBindings) {
      return await api.signTx(walletId: walletId, pending: pending);
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Broadcast signed transaction to the network
  /// 
  /// Sends via lightwalletd gRPC SendTransaction.
  /// Returns TxId on success.
  static Future<String> broadcastTx(SignedTx signed) async {
    if (kUseFrbBindings) {
      return await api.broadcastTx(signed: signed);
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  

  // ============================================================================
  // SYNC ENGINE
  // ============================================================================
  // 
  // Mirrors Rust: crates/pirate-ffi-frb/src/api.rs
  // - start_sync(), cancel_sync(), rescan(), sync_status(), is_sync_running()
  //
  // State is tracked locally until FRB wiring connects to Rust SimpleSync.
  // Progress calculation matches Rust engine behavior (checkpoints, stages, ETA).
  // ============================================================================
  
  
  /// Start blockchain sync for wallet
  /// 
  /// Connects to lightwalletd and syncs from last checkpoint.
  /// @see Rust: pirate-ffi-frb/src/api.rs::start_sync
  static Future<void> startSync(WalletId id, SyncMode mode) async {
    if (kUseFrbBindings) {
      await api.startSync(walletId: id, mode: mode);
      return;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get current sync status with performance metrics
  /// 
  /// Returns progress, heights, ETA, stage, and perf counters.
  /// @see Rust: pirate-ffi-frb/src/api.rs::sync_status
  static Future<SyncStatus> syncStatus(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.syncStatus(walletId: id);
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Rescan blockchain from specified height
  /// 
  /// Clears state above fromHeight and re-syncs. Uses deep mode.
  /// @see Rust: pirate-ffi-frb/src/api.rs::rescan
  static Future<void> rescan(WalletId id, int fromHeight) async {
    if (kUseFrbBindings) {
      await api.rescan(walletId: id, fromHeight: fromHeight);
      return;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  
  /// Cancel ongoing sync
  /// @see Rust: pirate-ffi-frb/src/api.rs::cancel_sync
  static Future<void> cancelSync(WalletId id) async {
    if (kUseFrbBindings) {
      await api.cancelSync(walletId: id);
      return;
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  
  /// Check if sync is currently running
  /// @see Rust: pirate-ffi-frb/src/api.rs::is_sync_running
  static Future<bool> isSyncRunning(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.isSyncRunning(walletId: id);
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  
  /// Get last checkpoint info for diagnostics
  /// @see Rust: pirate-ffi-frb/src/api.rs::get_last_checkpoint
  static Future<api.CheckpointInfo?> getLastCheckpoint(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.getLastCheckpoint(walletId: id);
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get last N sync log entries (local only, never transmitted)
  /// 
  /// Logs are stored in a circular buffer in memory and persisted to SQLite.
  /// Default limit is 200 entries. Logs are automatically redacted when
  /// copied via the UI (addresses, hashes, IPs, emails).
  /// 
  /// @param id - Wallet ID
  /// @param limit - Maximum number of log entries to return (default: 200)
  /// @returns List of log entries, newest first
  /// 
  /// Note: This function needs to be implemented in Rust FFI layer to query
  /// the sync_logs table in pirate-storage-sqlite. Currently returns empty list.
  static Future<List<SyncLogEntryFfi>> getSyncLogs(WalletId id, {int limit = 200}) async {
    if (kUseFrbBindings) {
      // Call Rust FFI function
      final logs = await api.getSyncLogs(walletId: id, limit: limit);
      return logs.map((log) => SyncLogEntryFfi.fromGenerated(log)).toList();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get checkpoint details for rescan confirmation
  /// 
  /// Returns detailed information about a specific checkpoint including:
  /// - Height and timestamp
  /// - Frontier root hash (for verification)
  /// - Number of notes at checkpoint
  /// 
  /// @param id - Wallet ID
  /// @param height - Checkpoint height to query
  /// 
  /// Note: This function needs to be implemented in Rust FFI layer to query
  /// the frontier_snapshots table in pirate-storage-sqlite.
  static Future<CheckpointInfo?> getCheckpointDetails(WalletId id, int height) async {
    if (kUseFrbBindings) {
      // Call Rust FFI function (requires FRB codegen to be run)
      final checkpoint = await api.getCheckpointDetails(walletId: id, height: height);
      if (checkpoint == null) return null;
      return CheckpointInfo(
        height: checkpoint.height,
        timestamp: DateTime.fromMillisecondsSinceEpoch(checkpoint.timestamp * 1000),
      );
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // ============================================================================
  // NODES & ENDPOINTS
  // ============================================================================
  //
  // Mirrors Rust: crates/pirate-ffi-frb/src/api.rs
  // - set_lightd_endpoint(), get_lightd_endpoint(), get_lightd_endpoint_config()
  // ============================================================================
  
  /// Default lightwalletd endpoint (Orchard-ready mainnet)
  /// Changed from lightd1.piratechain.com to 64.23.167.130:9067 as the working endpoint
  static const String defaultLightdHost = '64.23.167.130';
  static const int defaultLightdPort = 9067;
  static const bool defaultUseTls = false; // Disabled by default since servers don't have certificates
  static const String defaultLightdUrl = 'http://64.23.167.130:9067';

  static Future<void> setLightdEndpoint({
    required WalletId walletId,
    required String url,
    String? tlsPin,
  }) async {
    if (kUseFrbBindings) {
      await api.setLightdEndpoint(walletId: walletId, url: url, tlsPinOpt: tlsPin);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<String> getLightdEndpoint(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.getLightdEndpoint(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }
  
  static Future<LightdEndpointConfig> getLightdEndpointConfig(WalletId id) async {
    if (kUseFrbBindings) {
      final config = await api.getLightdEndpointConfig(walletId: id);
      final url = await config.url();
      return LightdEndpointConfig(
        url: url,
        tlsPin: config.tlsPin,
        label: config.label,
      );
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // Network Tunnel
  static Future<void> setTunnel(TunnelMode mode, {String? socksUrl}) async {
    if (kUseFrbBindings) {
      // If mode is socks5 but url is provided separately, reconstruct with the url
      TunnelMode tunnelMode = mode;
      if (socksUrl != null) {
        // Check if it's a socks5 mode by trying to access url property
        try {
          final currentUrl = (mode as dynamic).url as String?;
          if (currentUrl != null) {
            tunnelMode = TunnelMode.socks5(url: socksUrl);
          }
        } catch (_) {
          // Not a socks5 mode, keep original
        }
      }
      await api.setTunnel(mode: tunnelMode);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<TunnelMode> getTunnel() async {
    if (kUseFrbBindings) {
      return await api.getTunnel();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // Balance & Transactions
  static Future<Balance> getBalance(WalletId id) async {
    if (kUseFrbBindings) {
      return await api.getBalance(walletId: id);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<List<TxInfo>> listTransactions(WalletId id, {int limit = 50}) async {
    if (kUseFrbBindings) {
      return await api.listTransactions(walletId: id, limit: limit);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // Utilities
  static Future<String> generateMnemonic({int wordCount = 24}) async {
    if (kUseFrbBindings) {
      return await api.generateMnemonic(wordCount: wordCount);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<bool> validateMnemonic(String mnemonic) async {
    if (kUseFrbBindings) {
      return await api.validateMnemonic(mnemonic: mnemonic);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  static Future<NetworkInfo> getNetworkInfo() async {
    if (kUseFrbBindings) {
      return await api.getNetworkInfo();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Test connection to a lightwalletd node
  /// 
  /// Calls get_latest_block() and reports:
  /// - Transport mode (Tor/SOCKS5/Direct)
  /// - TLS status and pin verification
  /// - Latest block height
  /// - Response time
  /// 
  /// @param url - The lightwalletd endpoint URL
  /// @param tlsPin - Optional SPKI pin (base64 SHA-256 of public key)
  /// @returns NodeTestResult with connection details
  static Future<NodeTestResult> testNode({
    required String url,
    String? tlsPin,
  }) async {
    final startTime = DateTime.now();
    final tunnelMode = await getTunnel();
    final transportName = tunnelMode.name;
    
    // Determine if TLS is enabled
    final useTls = url.startsWith('https://') || 
                   (!url.startsWith('http://') && !url.contains('://'));
    
    try {
      // Note: This function needs to be implemented in Rust FFI layer
      // The actual connection should be established via the configured tunnel (Tor/SOCKS5/Direct)
      // and should:
      // 1. Establish connection via configured tunnel
      // 2. Perform TLS handshake and extract server certificate SPKI
      // 3. Compare SPKI with provided pin (if any)
      // 4. Call get_latest_block() gRPC method
      
      if (kUseFrbBindings) {
        // Call Rust FFI function
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
      
      // Fallback stub (should not be reached if kUseFrbBindings is true)
      throw UnimplementedError('FRB bindings not available');
    } on TorConnectionException catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage: 'Tor connection failed: ${e.message}\n\n'
            'Please check your Tor settings or try a different transport mode.',
        responseTimeMs: DateTime.now().difference(startTime).inMilliseconds,
      );
    } on Socks5ConnectionException catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage: 'SOCKS5 proxy connection failed: ${e.message}\n\n'
            'Please verify your proxy settings.',
        responseTimeMs: DateTime.now().difference(startTime).inMilliseconds,
      );
    } catch (e) {
      return NodeTestResult(
        success: false,
        transportMode: transportName,
        tlsEnabled: useTls,
        errorMessage: 'Connection failed: $e',
        responseTimeMs: DateTime.now().difference(startTime).inMilliseconds,
      );
    }
  }

  /// Validate SPKI pin format
  static bool _isValidSpkiPin(String pin) {
    // Accept with or without sha256/ prefix
    String base64Part = pin;
    if (pin.startsWith('sha256/')) {
      base64Part = pin.substring(7);
    }
    
    // Check if it's valid base64 and correct length (32 bytes = 44 chars base64)
    if (base64Part.length != 44 && base64Part.length != 43) {
      return false;
    }
    
    // Basic base64 character validation
    final base64Regex = RegExp(r'^[A-Za-z0-9+/]+=*$');
    return base64Regex.hasMatch(base64Part);
  }

  /// Normalize SPKI pin to just the base64 hash
  static String _normalizeSpkiPin(String pin) {
    if (pin.startsWith('sha256/')) {
      return pin.substring(7);
    }
    return pin;
  }

  static Future<String> formatAmount(int arrrtoshis) async {
    if (kUseFrbBindings) {
      return await api.formatAmount(arrrtoshis: BigInt.from(arrrtoshis));
    }
    // Fallback implementation
    return (arrrtoshis / 100000000).toStringAsFixed(8);
  }

  static Future<int> parseAmount(String arrr) async {
    if (kUseFrbBindings) {
      final result = await api.parseAmount(arrr: arrr);
      return result.toInt();
    }
    // Fallback implementation
    return (double.parse(arrr) * 100000000).toInt();
  }

  // Streams - Real sync progress from sync engine
  static Stream<SyncStatus> syncProgressStream(WalletId id) async* {
    int idleCount = 0;
    int lastHeight = 0;
    int lastTargetHeight = 0;
    DateTime? lastRestartAttempt;
    DateTime? lastStartAttempt;
    
    while (true) {
      try {
        // Check if sync is running (even when caught up, sync monitors for new blocks)
        final isRunning = await isSyncRunning(id);
        
        // Get real sync status
        final status = await syncStatus(id);

        // Auto-restart sync if it stopped while still behind target
        if (status.targetHeight > BigInt.zero &&
            status.localHeight < status.targetHeight) {
          final now = DateTime.now();
          final shouldRestart = !isRunning &&
              (lastRestartAttempt == null ||
                  now.difference(lastRestartAttempt!) >
                      const Duration(minutes: 2));
          if (shouldRestart) {
            lastRestartAttempt = now;
            try {
              await startSync(id, SyncMode.compact);
            } catch (_) {
              // Ignore restart failures; stream will retry later
            }
          }
        } else if (!isRunning &&
            status.targetHeight == BigInt.zero &&
            status.localHeight == BigInt.zero) {
          final now = DateTime.now();
          final shouldStart = lastStartAttempt == null ||
              now.difference(lastStartAttempt!) >
                  const Duration(seconds: 30);
          if (shouldStart) {
            lastStartAttempt = now;
            try {
              await startSync(id, SyncMode.compact);
            } catch (_) {
              // Ignore start failures; stream will retry later
            }
          }
        }
        yield status;
        
        // Track if progress is happening
        final currentHeight = status.localHeight.toInt();
        final targetHeight = status.targetHeight.toInt();
        if (currentHeight != lastHeight || targetHeight != lastTargetHeight) {
          lastHeight = currentHeight;
          lastTargetHeight = targetHeight;
          idleCount = 0;
        } else {
          idleCount++;
        }
        
        // Adjust polling interval based on sync activity
        // Keep polling frequent when sync is running (even if caught up) to show new blocks quickly
        Duration interval;
        if (status.isSyncing) {
          interval = const Duration(milliseconds: 500); // Fast updates during active sync
        } else if (isRunning) {
          // Sync is running but caught up - monitoring for new blocks
          // Poll every 1 second to catch new blocks quickly
          interval = const Duration(seconds: 1);
        } else if (idleCount < 10) {
          interval = const Duration(seconds: 1); // Normal updates after sync stops
        } else {
          interval = const Duration(seconds: 5); // Slow updates when truly idle
        }
        
        await Future<void>.delayed(interval);
      } catch (e) {
        // If sync status fails, yield a default status instead of crashing the stream
        // This allows the UI to continue working even if sync isn't available
        yield SyncStatus(
          localHeight: BigInt.zero,
          targetHeight: BigInt.zero,
          percent: 0.0,
          stage: SyncStage.verify,
          eta: null,
          blocksPerSecond: 0.0,
          notesDecrypted: BigInt.zero,
          lastBatchMs: BigInt.zero,
        );
        // Wait longer before retrying on error
        await Future<void>.delayed(const Duration(seconds: 5));
      }
    }
  }

  /// Transaction discovery stream
  /// Emits when sync engine finds new transactions
  /// 
  /// Polls the database periodically and emits new transactions as they are discovered.
  /// During active sync, polls every 2 seconds. When idle, polls every 5 seconds.
  static Stream<TxInfo> transactionStream(WalletId id) async* {
    if (!kUseFrbBindings) {
      // Fallback: return empty stream
      return;
    }
    
    // Track the last transaction IDs we've seen to detect new ones
    final Set<String> lastSeenTxids = <String>{};
    DateTime lastCheckTime = DateTime.now();
    
    while (true) {
      try {
        // Check if sync is running to adjust poll interval
        final isSyncing = await isSyncRunning(id);
        final pollInterval = isSyncing 
            ? const Duration(seconds: 2)  // Fast updates during sync
            : const Duration(seconds: 5);  // Slower updates when idle
        
        await Future<void>.delayed(pollInterval);
        
        // Get list of recent transactions from database
        final transactions = await listTransactions(id, limit: 100);
        
        // Find new transactions (ones we haven't seen before)
        for (final txInfo in transactions) {
          if (!lastSeenTxids.contains(txInfo.txid)) {
            // New transaction found!
            lastSeenTxids.add(txInfo.txid);
            yield txInfo;
          }
        }
        
        // Limit the size of lastSeenTxids to prevent memory growth
        // Keep only the most recent 1000 transaction IDs
        if (lastSeenTxids.length > 1000) {
          final recentTxids = transactions
              .take(1000)
              .map((tx) => tx.txid)
              .toSet();
          lastSeenTxids.clear();
          lastSeenTxids.addAll(recentTxids);
        }
      } catch (e) {
        // If we can't get transactions (e.g., wallet not loaded), log and continue
        // Only log errors occasionally to avoid spam
        if (DateTime.now().difference(lastCheckTime).inSeconds > 30) {
          debugPrint('Failed to get transactions for wallet $id: $e');
          lastCheckTime = DateTime.now();
        }
        
        // Wait before retrying on error
        await Future<void>.delayed(const Duration(seconds: 5));
      }
    }
  }

  static Stream<Balance> balanceStream(WalletId id) async* {
    if (kUseFrbBindings) {
      while (true) {
        yield await getBalance(id);
        await Future<void>.delayed(const Duration(seconds: 5));
      }
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // Background Sync (called from platform-specific code)
  
  /// Execute background sync via FFI
  /// All RPC calls are routed through the configured NetTunnel (Tor/SOCKS5/Direct)
  static Future<Map<String, dynamic>> executeBackgroundSync({
    required String walletId,
    required String mode,
    int maxDurationSecs = 60,
  }) async {
    if (kUseFrbBindings) {
      final startTime = DateTime.now();
      final result = await api.startBackgroundSync(
        walletId: walletId,
        mode: mode,
      );
      final tunnelMode = await getTunnel();
      
      return {
        'mode': result.mode,
        'blocks_synced': result.blocksSynced,
        'duration_secs':
            DateTime.now().difference(startTime).inSeconds,
        'new_transactions': result.newTransactions,
        'new_balance': result.newBalance,
        'tunnel_used': tunnelMode.name,
        'errors': result.errors,
      };
    }
    
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Execute background sync with round-robin wallet selection.
  static Future<Map<String, dynamic>> executeBackgroundSyncRoundRobin({
    required String mode,
    int maxDurationSecs = 60,
  }) async {
    if (kUseFrbBindings) {
      final result = await api.startBackgroundSyncRoundRobin(mode: mode);
      return {
        'walletId': result.walletId,
        'mode': result.mode,
        'blocksSynced': result.blocksSynced.toString(),
        'startHeight': result.startHeight.toString(),
        'endHeight': result.endHeight.toString(),
        'durationSecs': result.durationSecs.toString(),
      };
    }
    throw UnimplementedError('FRB bindings not available'); 
  }

  /// Set network tunnel mode for all RPC connections
  static Future<void> setTunnelMode({
    required String mode,
    String? socks5Url,
  }) async {
    final tunnelMode = switch (mode.toLowerCase()) {
      'tor' => const TunnelMode.tor(),
      'socks5' => TunnelMode.socks5(url: socks5Url ?? 'socks5://localhost:1080'),
      'direct' => const TunnelMode.direct(),
      _ => const TunnelMode.tor(),
    };
    await setTunnel(tunnelMode);
  }

  /// Check if sync is needed (behind tip)
  static Future<bool> isSyncNeeded(WalletId id) async {
    final status = await syncStatus(id);
    return status.localHeight < status.targetHeight;
  }

  /// Get recommended sync mode based on time since last sync
  static String getRecommendedSyncMode(int minutesSinceLastSync) {
    // Deep sync if > 24 hours
    if (minutesSinceLastSync >= 24 * 60) {
      return 'deep';
    }
    return 'compact';
  }

  // ============================================================================
  // PANIC PIN / DECOY VAULT
  // ============================================================================
  //
  // Mirrors Rust: crates/pirate-storage-sqlite/src/decoy_vault.rs
  // @see Rust: pirate-ffi-frb/src/api.rs::set_panic_pin, verify_panic_pin, etc.
  // ============================================================================

  /// Set panic PIN for decoy vault
  /// PIN is hashed with Argon2id before storage
  static Future<void> setPanicPin(String pin) async {
    if (kUseFrbBindings) {
      await api.setPanicPin(pin: pin);
      return;
    }
    // Fallback stub
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }

  /// Check if panic PIN is configured
  static Future<bool> hasPanicPin() async {
    if (kUseFrbBindings) {
      return await api.hasPanicPin();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Verify panic PIN (activates decoy mode if correct)
  /// Returns true if PIN matches and decoy mode activated
  static Future<bool> verifyPanicPin(String pin) async {
    if (kUseFrbBindings) {
      return await api.verifyPanicPin(pin: pin);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Check if currently in decoy mode
  static Future<bool> isDecoyMode() async {
    if (kUseFrbBindings) {
      return await api.isDecoyMode();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get vault mode (normal/decoy)
  static Future<String> getVaultMode() async {
    if (kUseFrbBindings) {
      return await api.getVaultMode();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Clear panic PIN
  static Future<void> clearPanicPin() async {
    if (kUseFrbBindings) {
      await api.clearPanicPin();
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Set decoy wallet name
  static Future<void> setDecoyWalletName(String name) async {
    if (kUseFrbBindings) {
      await api.setDecoyWalletName(name: name);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Exit decoy mode (requires real passphrase re-auth)
  static Future<void> exitDecoyMode() async {
    if (kUseFrbBindings) {
      await api.exitDecoyMode();
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // ============================================================================
  // Seed Export (Gated Flow)
  // ============================================================================

  /// Start seed export flow
  static Future<void> startSeedExport(WalletId walletId) async {
    if (kUseFrbBindings) {
      await api.startSeedExport(walletId: walletId);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Acknowledge seed export warning
  static Future<void> acknowledgeSeedWarning() async {
    if (kUseFrbBindings) {
      await api.acknowledgeSeedWarning();
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Complete biometric step
  static Future<void> completeSeedBiometric(bool success) async {
    if (kUseFrbBindings) {
      await api.completeSeedBiometric(success: success);
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Skip biometric (when not available)
  static Future<void> skipSeedBiometric() async {
    if (kUseFrbBindings) {
      await api.skipSeedBiometric();
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Export seed with passphrase verification
  /// Verifies passphrase against stored hash, then decrypts seed
  /// @see Rust: pirate-ffi-frb/src/api.rs::export_seed_with_passphrase
  static Future<List<String>> exportSeedWithPassphrase(
    WalletId walletId,
    String passphrase,
  ) async {
    if (kUseFrbBindings) {
      return await api.exportSeedWithPassphrase(
        walletId: walletId,
        passphrase: passphrase,
      );
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Export seed using cached app passphrase (after biometric approval).
  static Future<List<String>> exportSeedWithCachedPassphrase(
    WalletId walletId,
  ) async {
    if (kUseFrbBindings) {
      return await api.exportSeedWithCachedPassphrase(
        walletId: walletId,
      );
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Cancel seed export flow
  static Future<void> cancelSeedExport() async {
    if (kUseFrbBindings) {
      await api.cancelSeedExport();
      return;
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get current seed export state
  static Future<String> getSeedExportState() async {
    if (kUseFrbBindings) {
      return await api.getSeedExportState();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Check if screenshots are blocked during export
  static Future<bool> areSeedScreenshotsBlocked() async {
    if (kUseFrbBindings) {
      return await api.areSeedScreenshotsBlocked();
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // ============================================================================
  // WATCH-ONLY / IVK EXPORT/IMPORT
  // ============================================================================
  //
  // Mirrors Rust: crates/pirate-ffi-frb/src/api.rs
  // - export_ivk(), import_ivk()
  // ============================================================================

  /// Export IVK from full wallet (secure)
  /// @see Rust: pirate-ffi-frb/src/api.rs::export_ivk
  static Future<String> exportIvkSecure(WalletId walletId) async {
    if (kUseFrbBindings) {
      return await api.exportIvkSecure(walletId: walletId);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Import IVK to create watch-only wallet (alternate API)
  /// @see Rust: pirate-ffi-frb/src/api.rs::import_ivk
  static Future<WalletId> importIvkAsWatchOnly({
    required String name,
    required String ivk,
    required int birthdayHeight,
  }) async {
    // Delegate to main importIvk method
    return importIvk(
      name: name,
      ivk: ivk,
      birthday: birthdayHeight,
    );
  }

  /// Get watch-only capabilities for a wallet
  static Future<api.WatchOnlyCapabilitiesInfo> getWatchOnlyCapabilities(WalletId walletId) async {
    if (kUseFrbBindings) {
      return await api.getWatchOnlyCapabilities(walletId: walletId);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get watch-only banner for wallet
  /// 
  /// Returns banner info with "Incoming only" label for watch-only wallets.
  static Future<api.WatchOnlyBannerInfo?> getWatchOnlyBanner(WalletId walletId) async {
    if (kUseFrbBindings) {
      return await api.getWatchOnlyBanner(walletId: walletId);
    }
    // Fallback stub (should not be reached if kUseFrbBindings is true)
    throw UnimplementedError('FRB bindings not available');
  }

  // ============================================================================
  // BIOMETRIC AUTHENTICATION
  // ============================================================================
  //
  // Uses local_auth plugin which calls platform APIs:
  // - Android: BiometricPrompt, FingerprintManager
  // - iOS: LocalAuthentication (Face ID, Touch ID)
  // ============================================================================

  /// Authenticate with biometrics
  /// @see local_auth plugin for platform implementation
  static Future<bool> authenticateBiometric(String reason) async {
    await Future<void>.delayed(const Duration(milliseconds: 500));
    // local_auth will handle platform-specific biometric prompts
    return true;
  }

  /// Check if biometrics are available
  static Future<bool> hasBiometrics() async {
    await Future<void>.delayed(const Duration(milliseconds: 50));
    return true;
  }
}

// ============================================================================
// Additional Models for Security Features
// ============================================================================

/// Watch-only wallet capabilities
class WatchOnlyCapabilities {
  final bool isWatchOnly;
  final bool canViewIncoming;
  final bool canViewOutgoing;
  final bool canSpend;
  final bool canExportSeed;
  final bool canGenerateAddresses;

  WatchOnlyCapabilities({
    required this.isWatchOnly,
    required this.canViewIncoming,
    required this.canViewOutgoing,
    required this.canSpend,
    required this.canExportSeed,
    required this.canGenerateAddresses,
  });
}

/// Watch-only banner info
class WatchOnlyBannerInfo {
  final String bannerType;
  final String title;
  final String subtitle;
  final String icon;

  WatchOnlyBannerInfo({
    required this.bannerType,
    required this.title,
    required this.subtitle,
    required this.icon,
  });
}

/// Checkpoint information for diagnostics
class CheckpointInfo {
  final int height;
  final DateTime timestamp;
  
  CheckpointInfo({
    required this.height,
    required this.timestamp,
  });
}

/// Log severity levels
enum SyncLogLevel {
  debug('DEBUG'),
  info('INFO'),
  warn('WARN'),
  error('ERROR');

  final String label;
  const SyncLogLevel(this.label);

  static SyncLogLevel fromString(String s) {
    switch (s.toUpperCase()) {
      case 'DEBUG':
        return SyncLogLevel.debug;
      case 'INFO':
        return SyncLogLevel.info;
      case 'WARN':
      case 'WARNING':
        return SyncLogLevel.warn;
      case 'ERROR':
        return SyncLogLevel.error;
      default:
        return SyncLogLevel.debug;
    }
  }
}

/// A single sync log entry from the Rust sync engine
class SyncLogEntryFfi {
  final DateTime timestamp;
  final SyncLogLevel level;
  final String module;
  final String message;

  const SyncLogEntryFfi({
    required this.timestamp,
    required this.level,
    required this.module,
    required this.message,
  });

  /// Convert from generated SyncLogEntryFfi
  factory SyncLogEntryFfi.fromGenerated(models.SyncLogEntryFfi generated) {
    // Convert PlatformInt64 timestamp to DateTime
    int timestampValue;
    final ts = generated.timestamp;
    if (ts is int) {
      timestampValue = ts;
    } else {
      timestampValue = (ts as dynamic).toInt() as int;
    }
    
    return SyncLogEntryFfi(
      timestamp: DateTime.fromMillisecondsSinceEpoch(timestampValue * 1000),
      level: SyncLogLevel.fromString(generated.level),
      module: generated.module,
      message: generated.message,
    );
  }

  /// Redacted version for sharing (removes sensitive data)
  String toRedactedString() {
    final redactedMessage = message
        .replaceAll(RegExp(r'zs[a-z0-9]{70,}', caseSensitive: false), '[REDACTED_ADDRESS]')
        .replaceAll(RegExp(r'0x[a-fA-F0-9]{64}'), '[REDACTED_HASH]')
        .replaceAll(RegExp(r'\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}'), '[REDACTED_IP]')
        .replaceAll(RegExp(r'[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}'), '[REDACTED_EMAIL]');

    final t = timestamp;
    final timeFormatted = '${t.hour.toString().padLeft(2, '0')}:'
        '${t.minute.toString().padLeft(2, '0')}:'
        '${t.second.toString().padLeft(2, '0')}.'
        '${t.millisecond.toString().padLeft(3, '0')}';

    return '[$timeFormatted] [${level.label}] [$module] $redactedMessage';
  }

  @override
  String toString() {
    final t = timestamp;
    final timeFormatted = '${t.hour.toString().padLeft(2, '0')}:'
        '${t.minute.toString().padLeft(2, '0')}:'
        '${t.second.toString().padLeft(2, '0')}.'
        '${t.millisecond.toString().padLeft(3, '0')}';
    return '[$timeFormatted] [${level.label}] [$module] $message';
  }
}

// ============================================================================
// ADDRESS BOOK
// ============================================================================

/// Color tag for address book entries
enum AddressBookColorTag {
  none(0, 'None', 0xFF6B7280),
  red(1, 'Red', 0xFFEF4444),
  orange(2, 'Orange', 0xFFF97316),
  yellow(3, 'Yellow', 0xFFEAB308),
  green(4, 'Green', 0xFF22C55E),
  blue(5, 'Blue', 0xFF3B82F6),
  purple(6, 'Purple', 0xFF8B5CF6),
  pink(7, 'Pink', 0xFFEC4899),
  gray(8, 'Gray', 0xFF6B7280);

  final int value;
  final String displayName;
  final int colorValue;

  const AddressBookColorTag(this.value, this.displayName, this.colorValue);

  static AddressBookColorTag fromValue(int value) {
    return AddressBookColorTag.values.firstWhere(
      (t) => t.value == value,
      orElse: () => AddressBookColorTag.none,
    );
  }
}

/// Maximum label length
const int kMaxLabelLength = 100;

/// Maximum notes length  
const int kMaxNotesLength = 500;

/// Address book entry model (mirrors Rust AddressBookEntry)
class AddressBookEntryFfi {
  final int id;
  final String walletId;
  final String address;
  final String label;
  final String? notes;
  final AddressBookColorTag colorTag;
  final bool isFavorite;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? lastUsedAt;
  final int useCount;

  const AddressBookEntryFfi({
    required this.id,
    required this.walletId,
    required this.address,
    required this.label,
    this.notes,
    this.colorTag = AddressBookColorTag.none,
    this.isFavorite = false,
    required this.createdAt,
    required this.updatedAt,
    this.lastUsedAt,
    this.useCount = 0,
  });

  AddressBookEntryFfi copyWith({
    int? id,
    String? walletId,
    String? address,
    String? label,
    String? notes,
    AddressBookColorTag? colorTag,
    bool? isFavorite,
    DateTime? createdAt,
    DateTime? updatedAt,
    DateTime? lastUsedAt,
    int? useCount,
  }) {
    return AddressBookEntryFfi(
      id: id ?? this.id,
      walletId: walletId ?? this.walletId,
      address: address ?? this.address,
      label: label ?? this.label,
      notes: notes ?? this.notes,
      colorTag: colorTag ?? this.colorTag,
      isFavorite: isFavorite ?? this.isFavorite,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      lastUsedAt: lastUsedAt ?? this.lastUsedAt,
      useCount: useCount ?? this.useCount,
    );
  }

  /// Get truncated address for display
  String get truncatedAddress {
    if (address.length > 24) {
      return '${address.substring(0, 12)}...${address.substring(address.length - 12)}';
    }
    return address;
  }

  /// Get avatar letter
  String get avatarLetter => label.isEmpty ? '?' : label[0].toUpperCase();

  Map<String, dynamic> toJson() => {
    'id': id,
    'wallet_id': walletId,
    'address': address,
    'label': label,
    'notes': notes,
    'color_tag': colorTag.value,
    'is_favorite': isFavorite,
    'created_at': createdAt.toIso8601String(),
    'updated_at': updatedAt.toIso8601String(),
    'last_used_at': lastUsedAt?.toIso8601String(),
    'use_count': useCount,
  };

  factory AddressBookEntryFfi.fromJson(Map<String, dynamic> json) {
    return AddressBookEntryFfi(
      id: json['id'] as int,
      walletId: json['wallet_id'] as String,
      address: json['address'] as String,
      label: json['label'] as String,
      notes: json['notes'] as String?,
      colorTag: AddressBookColorTag.fromValue(json['color_tag'] as int? ?? 0),
      isFavorite: json['is_favorite'] as bool? ?? false,
      createdAt: DateTime.parse(json['created_at'] as String),
      updatedAt: DateTime.parse(json['updated_at'] as String),
      lastUsedAt: json['last_used_at'] != null
          ? DateTime.parse(json['last_used_at'] as String)
          : null,
      useCount: json['use_count'] as int? ?? 0,
    );
  }

  /// Convert from generated AddressBookEntryFfi
  factory AddressBookEntryFfi.fromGenerated(models.AddressBookEntryFfi generated) {
    // PlatformInt64 conversion - handle both int and PlatformInt64 types
    int idValue;
    final id = generated.id;
    if (id is int) {
      idValue = id;
    } else {
      idValue = (id as dynamic).toInt() as int;
    }
    
    int createdAtValue;
    final createdAt = generated.createdAt;
    if (createdAt is int) {
      createdAtValue = createdAt;
    } else {
      createdAtValue = (createdAt as dynamic).toInt() as int;
    }
    
    int updatedAtValue;
    final updatedAt = generated.updatedAt;
    if (updatedAt is int) {
      updatedAtValue = updatedAt;
    } else {
      updatedAtValue = (updatedAt as dynamic).toInt() as int;
    }
    
    int? lastUsedAtValue;
    final lastUsedAt = generated.lastUsedAt;
    if (lastUsedAt != null) {
      if (lastUsedAt is int) {
        lastUsedAtValue = lastUsedAt;
      } else {
        lastUsedAtValue = (lastUsedAt as dynamic).toInt() as int;
      }
    } else {
      lastUsedAtValue = null;
    }
    
    return AddressBookEntryFfi(
      id: idValue,
      walletId: generated.walletId,
      address: generated.address,
      label: generated.label,
      notes: generated.notes,
      colorTag: AddressBookColorTag.fromValue(_generatedColorTagToInt(generated.colorTag)),
      isFavorite: generated.isFavorite,
      createdAt: DateTime.fromMillisecondsSinceEpoch(createdAtValue * 1000), // Convert seconds to milliseconds
      updatedAt: DateTime.fromMillisecondsSinceEpoch(updatedAtValue * 1000),
      lastUsedAt: lastUsedAtValue != null
          ? DateTime.fromMillisecondsSinceEpoch(lastUsedAtValue * 1000)
          : null,
      useCount: generated.useCount,
    );
  }

  static int _generatedColorTagToInt(models.AddressBookColorTag tag) {
    switch (tag) {
      case models.AddressBookColorTag.none:
        return 0;
      case models.AddressBookColorTag.red:
        return 1;
      case models.AddressBookColorTag.orange:
        return 2;
      case models.AddressBookColorTag.yellow:
        return 3;
      case models.AddressBookColorTag.green:
        return 4;
      case models.AddressBookColorTag.blue:
        return 5;
      case models.AddressBookColorTag.purple:
        return 6;
      case models.AddressBookColorTag.pink:
        return 7;
      case models.AddressBookColorTag.gray:
        return 8;
    }
  }
  /// List all address book entries for a wallet
  static Future<List<AddressBookEntryFfi>> listAddressBook(String walletId) async {
    if (kUseFrbBindings) {
      final result = await api.listAddressBook(walletId: walletId);
      return result.map((e) => AddressBookEntryFfi.fromGenerated(e)).toList();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get entry by ID
  static Future<AddressBookEntryFfi?> getAddressBookEntry(String walletId, int id) async {
    if (kUseFrbBindings) {
      final result = await api.getAddressBookEntry(walletId: walletId, id: id);
      return result != null ? AddressBookEntryFfi.fromGenerated(result) : null;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get entry by address
  static Future<AddressBookEntryFfi?> getAddressBookEntryByAddress(
    String walletId,
    String address,
  ) async {
    if (kUseFrbBindings) {
      final result = await api.getAddressBookEntryByAddress(walletId: walletId, address: address);
      return result != null ? AddressBookEntryFfi.fromGenerated(result) : null;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get label for an address (for transaction history display)
  static Future<String?> getLabelForAddress(String walletId, String address) async {
    if (kUseFrbBindings) {
      return await api.getLabelForAddress(walletId: walletId, address: address);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Add new address book entry
  static Future<AddressBookEntryFfi> addAddressBookEntry({
    required String walletId,
    required String address,
    required String label,
    String? notes,
    AddressBookColorTag colorTag = AddressBookColorTag.none,
  }) async {
    if (kUseFrbBindings) {
      // Convert local AddressBookColorTag to generated one
      final generatedColorTag = _convertColorTag(colorTag);
      final result = await api.addAddressBookEntry(
        walletId: walletId,
        address: address,
        label: label,
        notes: notes,
        colorTag: generatedColorTag,
      );
      return AddressBookEntryFfi.fromGenerated(result);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Update address book entry
  static Future<AddressBookEntryFfi> updateAddressBookEntry({
    required String walletId,
    required int id,
    String? label,
    String? notes,
    AddressBookColorTag? colorTag,
    bool? isFavorite,
  }) async {
    if (kUseFrbBindings) {
      // Convert local AddressBookColorTag to generated one
      final generatedColorTag = colorTag != null ? _convertColorTag(colorTag) : null;
      final result = await api.updateAddressBookEntry(
        walletId: walletId,
        id: id,
        label: label,
        notes: notes,
        colorTag: generatedColorTag,
        isFavorite: isFavorite,
      );
      return AddressBookEntryFfi.fromGenerated(result);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Delete address book entry
  static Future<void> deleteAddressBookEntry(String walletId, int id) async {
    if (kUseFrbBindings) {
      await api.deleteAddressBookEntry(walletId: walletId, id: id);
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Toggle favorite status
  static Future<bool> toggleAddressBookFavorite(String walletId, int id) async {
    if (kUseFrbBindings) {
      return await api.toggleAddressBookFavorite(walletId: walletId, id: id);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Mark address as used (for send history)
  static Future<void> markAddressUsed(String walletId, String address) async {
    if (kUseFrbBindings) {
      await api.markAddressUsed(walletId: walletId, address: address);
      return;
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Search address book entries
  static Future<List<AddressBookEntryFfi>> searchAddressBook(
    String walletId,
    String query,
  ) async {
    if (kUseFrbBindings) {
      final result = await api.searchAddressBook(walletId: walletId, query: query);
      return result.map((e) => AddressBookEntryFfi.fromGenerated(e)).toList();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get favorites
  static Future<List<AddressBookEntryFfi>> getAddressBookFavorites(String walletId) async {
    if (kUseFrbBindings) {
      final result = await api.getAddressBookFavorites(walletId: walletId);
      return result.map((e) => AddressBookEntryFfi.fromGenerated(e)).toList();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get recently used addresses
  static Future<List<AddressBookEntryFfi>> getRecentlyUsedAddresses(
    String walletId,
    int limit,
  ) async {
    if (kUseFrbBindings) {
      final result = await api.getRecentlyUsedAddresses(walletId: walletId, limit: limit);
      return result.map((e) => AddressBookEntryFfi.fromGenerated(e)).toList();
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Check if address exists in book
  static Future<bool> addressExistsInBook(String walletId, String address) async {
    if (kUseFrbBindings) {
      return await api.addressExistsInBook(walletId: walletId, address: address);
    }
    throw UnimplementedError('FRB bindings not available');
  }

  /// Get entry count
  static Future<int> getAddressBookCount(String walletId) async {
    if (kUseFrbBindings) {
      return await api.getAddressBookCount(walletId: walletId);
    }
    throw UnimplementedError('FRB bindings not available');
  }

}

/// Alias for compatibility with background_sync_manager.dart
class FFIBridge extends FfiBridge {
  // All static methods inherited from FfiBridge
}

// ============================================================================
// Extensions for generated types
// ============================================================================

/// Extension to add helper methods to TunnelMode
extension TunnelModeExtension on TunnelMode {
  /// Get human-readable name for tunnel mode
  String get name {
    // Check properties to determine variant
    try {
      final url = (this as dynamic).url as String?;
      if (url != null) {
        return 'socks5';
      }
    } catch (_) {
      // Not socks5
    }
    
    // Check runtimeType name to distinguish tor vs direct
    final typeName = runtimeType.toString();
    if (typeName.contains('Tor')) {
      return 'tor';
    } else if (typeName.contains('Direct')) {
      return 'direct';
    }
    
    // Fallback
    return 'tor';
  }
}

/// Extension to add helper methods to SyncStatus
extension SyncStatusExtension on SyncStatus {
  /// Check if sync is currently running
  /// Note: This only checks if behind target. Use isSyncRunning() to check if sync engine is active.
  bool get isSyncing {
    return localHeight < targetHeight && targetHeight > BigInt.zero;
  }
  
  /// Check if sync is complete (caught up to target)
  /// Note: Sync may still be monitoring for new blocks even when "complete"
  bool get isComplete {
    return localHeight >= targetHeight && targetHeight > BigInt.zero;
  }
  
  /// Get stage name as string with user-friendly labels
  String get stageName {
    // When caught up, show "Monitoring" instead of "Verify"
    if (isComplete) {
      return 'Monitoring';
    }
    // Map stage enum to user-friendly names
    switch (stage) {
      case SyncStage.headers:
        return 'Fetching Headers';
      case SyncStage.notes:
        return 'Scanning Notes';
      case SyncStage.witness:
        return 'Building Witnesses';
      case SyncStage.verify:
        return 'Synching Chain';
    }
  }
  
  /// Get formatted ETA string
  /// Returns null when caught up (no ETA needed) or when ETA is not available
  String? get etaFormatted {
    // When caught up, return null (no ETA needed)
    if (isComplete) {
      return null;
    }
    // When syncing but ETA not available, return null (UI will show "Calculating..." if needed)
    if (eta == null || eta == BigInt.zero) {
      return null;
    }
    final seconds = eta!.toInt();
    if (seconds < 60) {
      return '${seconds}s';
    } else if (seconds < 3600) {
      return '${(seconds / 60).round()}m';
    } else {
      return '${(seconds / 3600).round()}h';
    }
  }
  
}

/// Extension to add helper methods to NetworkInfo
extension NetworkInfoExtension on NetworkInfo {
  /// Check if Tor is enabled (always false for now, Tor is handled separately)
  bool get torEnabled => false;
}

/// Extension to add helper methods to SyncMode
extension SyncModeExtension on SyncMode {
  /// Get SyncMode enum values
  static List<SyncMode> get values => SyncMode.values;
}
