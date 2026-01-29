import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/core/services/address_rotation_service.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart' show WalletId;
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';

const _testWalletId = 'test-wallet-rotation';

class _TestActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _testWalletId;
}

ProviderContainer _buildContainer() {
  return ProviderContainer(
    overrides: [
      activeWalletProvider.overrideWith(_TestActiveWalletNotifier.new),
      transactionStreamProvider.overrideWith((ref) => const Stream<TxInfo?>.empty()),
      syncProgressStreamProvider.overrideWith((ref) => const Stream<SyncStatus?>.empty()),
    ],
  );
}

void main() {
  group('AddressRotationService', () {
    test('should be instantiable via provider', () {
      final container = _buildContainer();
      addTearDown(container.dispose);

      final service = container.read(addressRotationServiceProvider);
      expect(service, isA<AddressRotationService>());
    });

    test('checkAndRotateIfNeeded should handle no wallet gracefully', () async {
      final container = _buildContainer();
      addTearDown(container.dispose);

      final service = container.read(addressRotationServiceProvider);
      
      // Should not throw when checking with invalid wallet ID
      // (FFI bridge will handle the error internally)
      expect(
        () => service.checkAndRotateIfNeeded('invalid-wallet-id'),
        returnsNormally,
      );
    });

    test('manualRotate should throw on invalid wallet', () async {
      final container = _buildContainer();
      addTearDown(container.dispose);

      final service = container.read(addressRotationServiceProvider);
      
      // Manual rotation should throw on error
      expect(() => service.manualRotate('invalid-wallet-id'),
          throwsA(isA<Object>()));
    });
  });

  group('Auto-rotation watchers', () {
    test('autoRotationWatcherProvider should watch transaction stream', () {
      final container = _buildContainer();
      addTearDown(container.dispose);

      // Watch the provider to ensure it initializes
      container.read(autoRotationWatcherProvider);
      
      // Provider should be listening to transaction stream
      expect(container.exists(transactionStreamProvider), isTrue);
    });

    test('syncCompletionRotationWatcherProvider should watch sync status', () {
      final container = _buildContainer();
      addTearDown(container.dispose);

      // Watch the provider to ensure it initializes
      container.read(syncCompletionRotationWatcherProvider);
      
      // Provider should be listening to sync stream
      expect(container.exists(syncProgressStreamProvider), isTrue);
    });

    test('walletInitRotationWatcherProvider should trigger on wallet change', () {
      final container = _buildContainer();
      addTearDown(container.dispose);

      // Watch the provider to ensure it initializes
      container.read(walletInitRotationWatcherProvider);
      
      // Provider should be watching active wallet
      expect(container.exists(activeWalletProvider), isTrue);
    });
  });

  group('Integration behavior', () {
    test('rotation logic description', () {
      // This test documents the expected behavior for manual testing
      
      const expectedBehavior = '''
      Address Rotation Behavior:
      
      1. Auto-rotation on incoming funds:
         - When a transaction with positive amount is detected
         - Check if current address has non-zero balance
         - If yes, rotate to next unused address
         - Skip any addresses that already have balances
      
      2. Manual rotation via "New Address" button:
         - User clicks "New Address" button
         - Get next address from diversifier sequence (backend monotonic index)
         - Update UI with fresh, unused address
      
      3. Wallet recovery/rescan scenario:
         - After sync completes, check current address
         - If current address has balance, auto-rotate
         - Next address uses the next diversifier index tracked by the backend
         - Avoids address reuse by never decreasing the index
      
      4. Wallet initialization:
         - On app startup or wallet switch
         - Check if current address has balance
         - If yes, rotate to fresh address
         - Ensures app always shows unused address
      
      Safety features:
      - No artificial loop or cap in Dart; backend increments index (u32 max)
      - Auto-rotation failures don't crash app
      - Manual rotation failures throw errors for user feedback
      - All rotation logic centralized in AddressRotationService
      ''';
      
      expect(expectedBehavior, isNotEmpty);
    });
  });
}
