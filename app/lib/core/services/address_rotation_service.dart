import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../ffi/ffi_bridge.dart' hide AddressBookColorTag;
import '../ffi/generated/models.dart';
import '../providers/wallet_providers.dart';
import '../sync/sync_status_cache.dart';
import '../../features/receive/receive_viewmodel.dart';

/// Service for automatic address rotation when funds are received.
///
/// Privacy-first design:
/// - Automatically rotates to a fresh address when current address receives funds
/// - Never reuses addresses that have received funds (enforced by Rust backend)
class AddressRotationService {
  final Ref ref;

  AddressRotationService(this.ref);

  /// Check if the current receive address has received funds and auto-rotate if needed.
  ///
  /// Called when:
  /// 1. A new transaction is detected
  /// 2. Sync completes (handles recovery/rescan)
  /// 3. Wallet is initialized
  Future<void> checkAndRotateIfNeeded(String walletId) async {
    try {
      debugPrint('[AddressRotation] Checking if rotation needed for wallet $walletId');

      // Get current receive address
      final initialAddress = await FfiBridge.currentReceiveAddress(walletId);

      // Get all address balances (may be empty on brand-new wallets)
      final addressBalances = await FfiBridge.listAddressBalances(walletId);

      // The backend may auto-rotate while loading balances; detect and refresh UI.
      final currentAddress = await FfiBridge.currentReceiveAddress(walletId);
      if (currentAddress != initialAddress) {
        debugPrint(
          '[AddressRotation] Backend rotated address from $initialAddress to $currentAddress',
        );
        ref.invalidate(receiveViewModelProvider);
        return;
      }

      // If we have no address records yet, nothing to do.
      if (addressBalances.isEmpty) {
        debugPrint('[AddressRotation] No address balance records found');
        return;
      }

      // Find current address record (if any), otherwise treat as zero-balance.
      final currentAddressBalance = addressBalances.firstWhere(
        (addr) => addr.address == currentAddress,
        orElse: () => AddressBalanceInfo(
          address: currentAddress,
          balance: BigInt.zero,
          spendable: BigInt.zero,
          pending: BigInt.zero,
          keyId: 0,
          addressId: -1,
          label: null,
          createdAt: 0,
          diversifierIndex: 0,
          colorTag: AddressBookColorTag.none,
        ),
      );

      // If current address has received any funds (balance > 0), rotate.
      if (currentAddressBalance.balance > BigInt.zero ||
          currentAddressBalance.pending > BigInt.zero) {
        debugPrint(
          '[AddressRotation] Current address has balance: ${currentAddressBalance.balance}, '
          'pending: ${currentAddressBalance.pending}. Rotating...',
        );

        await _rotateToNextAddress(walletId);
      } else {
        debugPrint('[AddressRotation] Current address is unused, no rotation needed');
      }
    } catch (e, stackTrace) {
      debugPrint('[AddressRotation] Error checking rotation: $e');
      debugPrint('Stack trace: $stackTrace');
      // Don't throw - auto-rotation is a nice-to-have, not critical
    }
  }

  /// Rotate to the next receive address.
  ///
  /// The Rust backend guarantees that `next_receive_address` always advances
  /// the diversifier index (no wraparound), so we don't need an artificial
  /// limit or loop on the Dart side.
  Future<void> _rotateToNextAddress(String walletId) async {
    try {
      final newAddress = await FfiBridge.nextReceiveAddress(walletId);
      debugPrint('[AddressRotation] Rotated to next address: $newAddress');

      // Invalidate receive provider to refresh UI
      ref.invalidate(receiveViewModelProvider);
    } catch (e) {
      debugPrint('[AddressRotation] WARNING during rotation: $e');
      // Auto-rotation failure shouldn't break the app
    }
  }

  /// Manually rotate to next address (used by \"New Address\" button).
  ///
  /// This delegates to the backend, which advances to the next diversifier
  /// index and never reuses previously stored addresses.
  Future<String> manualRotate(String walletId) async {
    try {
      debugPrint('[AddressRotation] Manual rotation requested');

      final newAddress = await FfiBridge.nextReceiveAddress(walletId);
      debugPrint('[AddressRotation] Manually rotated to: $newAddress');

      // Invalidate receive provider to refresh UI
      ref.invalidate(receiveViewModelProvider);

      return newAddress;
    } catch (e, stackTrace) {
      debugPrint('[AddressRotation] Error during manual rotation: $e');
      debugPrint('Stack trace: $stackTrace');
      rethrow; // Manual rotation should throw on error
    }
  }
}

/// Provider for address rotation service.
final addressRotationServiceProvider = Provider<AddressRotationService>((ref) {
  return AddressRotationService(ref);
});

/// Auto-rotation watcher that triggers rotation on incoming transactions.
///
/// Watches the transaction stream and automatically rotates when:
/// 1. A new incoming transaction is detected (amount > 0)
/// 2. The transaction is confirmed or unconfirmed (we rotate immediately)
final autoRotationWatcherProvider = Provider<void>((ref) {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return;

  final rotationService = ref.watch(addressRotationServiceProvider);

  // Watch for new transactions
  ref.listen<AsyncValue<TxInfo?>>(transactionStreamProvider, (_, next) {
    next.whenData((tx) async {
      if (tx == null) return;

      // Positive amount => incoming transaction
      final isIncoming = tx.amount.toInt() > 0;

      if (isIncoming) {
        debugPrint(
          '[AutoRotation] Incoming transaction detected: ${tx.txid} '
          '(amount: ${tx.amount}, confirmed: ${tx.confirmed})',
        );

        await rotationService.checkAndRotateIfNeeded(walletId);
      }
    });
  });
});

/// Sync completion watcher for auto-rotation after rescan/recovery.
///
/// After sync completes, we check the current address and rotate if it has
/// received funds during the sync.
final syncCompletionRotationWatcherProvider = Provider<void>((ref) {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return;

  final rotationService = ref.watch(addressRotationServiceProvider);

  var wasSyncing = false;
  ref.listen<AsyncValue<SyncStatus?>>(syncProgressStreamProvider, (_, next) {
    next.whenData((status) async {
      final isSyncing = status?.isSyncing ?? false;

      // Sync just completed
      if (wasSyncing && !isSyncing) {
        debugPrint('[SyncCompletionRotation] Sync completed, checking address rotation...');
        await rotationService.checkAndRotateIfNeeded(walletId);
      }

      wasSyncing = isSyncing;
    });
  });
});

/// Wallet initialization watcher for auto-rotation on startup.
///
/// Handles the case where the wallet is loaded and the current address already
/// has a balance (e.g., after app restart or wallet switch).
final walletInitRotationWatcherProvider = Provider<void>((ref) {
  final walletId = ref.watch(activeWalletProvider);
  if (walletId == null) return;

  final rotationService = ref.watch(addressRotationServiceProvider);

  // Check rotation on wallet load
  Future.microtask(() async {
    debugPrint('[WalletInitRotation] Wallet loaded, checking if rotation needed...');
    await rotationService.checkAndRotateIfNeeded(walletId);
  });
});
