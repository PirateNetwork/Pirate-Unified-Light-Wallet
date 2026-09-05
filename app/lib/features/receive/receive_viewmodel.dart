import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:share_plus/share_plus.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart' show KeyGroupInfo, KeyTypeInfo;
import '../../core/providers/wallet_providers.dart';
import '../../core/security/decoy_data.dart';
import '../../core/security/clipboard_manager.dart';
import '../../core/services/address_rotation_service.dart';
import '../../design/tokens/colors.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// State for receive screen
class ReceiveState {
  final String? currentAddress;
  final List<AddressInfo> addressHistory;
  final bool isLoading;
  final String? error;
  final int diversifierIndex;
  final bool addressWasShared;
  final List<KeyGroupInfo> keyGroups;

  const ReceiveState({
    this.currentAddress,
    this.addressHistory = const [],
    this.isLoading = false,
    this.error,
    this.diversifierIndex = 0,
    this.addressWasShared = false,
    this.keyGroups = const [],
  });

  ReceiveState copyWith({
    String? currentAddress,
    List<AddressInfo>? addressHistory,
    bool? isLoading,
    String? error,
    int? diversifierIndex,
    bool? addressWasShared,
    List<KeyGroupInfo>? keyGroups,
  }) {
    return ReceiveState(
      currentAddress: currentAddress ?? this.currentAddress,
      addressHistory: addressHistory ?? this.addressHistory,
      isLoading: isLoading ?? this.isLoading,
      error: error,
      diversifierIndex: diversifierIndex ?? this.diversifierIndex,
      addressWasShared: addressWasShared ?? this.addressWasShared,
      keyGroups: keyGroups ?? this.keyGroups,
    );
  }
}

/// Address info model with usage tracking
class AddressInfo {
  final int? addressId;
  final int? keyId;
  final String? keyLabel;
  final int? seedAccountIndex;
  final String address;
  final String? label;
  final DateTime createdAt;
  final bool isActive;
  final int diversifierIndex;
  final bool wasShared;
  final bool wasUsedForReceive;
  final AddressBookColorTag colorTag;
  final BigInt balance;
  final BigInt spendable;
  final BigInt pending;
  final bool isPinned;
  final bool isArchived;

  AddressInfo({
    this.addressId,
    this.keyId,
    this.keyLabel,
    this.seedAccountIndex,
    required this.address,
    this.label,
    required this.createdAt,
    this.isActive = false,
    this.diversifierIndex = 0,
    this.wasShared = false,
    this.wasUsedForReceive = false,
    this.colorTag = AddressBookColorTag.none,
    BigInt? balance,
    BigInt? spendable,
    BigInt? pending,
    this.isPinned = false,
    this.isArchived = false,
  }) : balance = balance ?? BigInt.zero,
       spendable = spendable ?? BigInt.zero,
       pending = pending ?? BigInt.zero;

  AddressInfo copyWith({
    int? addressId,
    int? keyId,
    String? keyLabel,
    int? seedAccountIndex,
    String? address,
    String? label,
    DateTime? createdAt,
    bool? isActive,
    int? diversifierIndex,
    bool? wasShared,
    bool? wasUsedForReceive,
    AddressBookColorTag? colorTag,
    BigInt? balance,
    BigInt? spendable,
    BigInt? pending,
    bool? isPinned,
    bool? isArchived,
  }) {
    return AddressInfo(
      addressId: addressId ?? this.addressId,
      keyId: keyId ?? this.keyId,
      keyLabel: keyLabel ?? this.keyLabel,
      seedAccountIndex: seedAccountIndex ?? this.seedAccountIndex,
      address: address ?? this.address,
      label: label ?? this.label,
      createdAt: createdAt ?? this.createdAt,
      isActive: isActive ?? this.isActive,
      diversifierIndex: diversifierIndex ?? this.diversifierIndex,
      wasShared: wasShared ?? this.wasShared,
      wasUsedForReceive: wasUsedForReceive ?? this.wasUsedForReceive,
      colorTag: colorTag ?? this.colorTag,
      balance: balance ?? this.balance,
      spendable: spendable ?? this.spendable,
      pending: pending ?? this.pending,
      isPinned: isPinned ?? this.isPinned,
      isArchived: isArchived ?? this.isArchived,
    );
  }

  /// Get truncated address for display
  String get truncatedAddress {
    if (address.length < 20) return address;
    return '${address.substring(0, 12)}...${address.substring(address.length - 8)}';
  }
}

/// ViewModel for receive screen
///
/// Enforces diversifier rotation:
/// - New Address button always generates fresh address
/// - Current address is never reused after being shared
/// - Address history tracks usage for privacy awareness
class ReceiveViewModel extends Notifier<ReceiveState> {
  WalletId? _walletId;
  bool _initialized = false;
  ReceiveState? _lastState;
  bool _isDecoy = false;
  DateTime? _lastAddressRefreshAt;
  bool _addressRefreshInFlight = false;

  /// Track if current address was shared (copied/shared)
  bool _currentAddressShared = false;

  @override
  ReceiveState build() {
    try {
      // Keep provider alive during initialization
      ref.keepAlive();

      // Watch active wallet provider
      final walletId = ref.watch(activeWalletProvider);
      final isDecoy = ref.watch(decoyModeProvider);

      // Handle wallet changes - reset initialization when wallet changes
      if (walletId != _walletId || isDecoy != _isDecoy) {
        _walletId = walletId;
        _isDecoy = isDecoy;
        _initialized = false;
        _lastState = null;

        // If wallet changed, reset and reinitialize
        if (walletId != null) {
          Future.microtask(_init);
          return const ReceiveState(isLoading: true);
        } else {
          return const ReceiveState(
            error: 'No wallet selected',
            isLoading: false,
          );
        }
      }

      // Initialize if wallet is set and we haven't initialized yet
      if (walletId != null && !_initialized) {
        _initialized = true;
        // Use Future.microtask to avoid calling async code during build
        Future.microtask(_init);
        // Return loading state while initializing
        return const ReceiveState(isLoading: true);
      }

      // If no wallet, return error state
      if (walletId == null) {
        return const ReceiveState(
          error: 'No wallet selected',
          isLoading: false,
        );
      }

      // If we reach here, wallet exists and we've initialized
      // Return last known state if available, otherwise return loading state
      // This handles the case where we're waiting for _init to complete
      return _lastState ?? const ReceiveState(isLoading: true);
    } catch (e, stackTrace) {
      debugPrint('Error in ReceiveViewModel.build(): $e');
      debugPrint('Stack trace: $stackTrace');
      return ReceiveState(error: 'Error initializing: $e', isLoading: false);
    }
  }

  /// Initialize the receive screen
  Future<void> _init() async {
    if (_walletId == null) {
      final errorState = ReceiveState(
        error: 'No wallet selected'.tr,
        isLoading: false,
      );
      _lastState = errorState;
      state = errorState;
      return;
    }

    try {
      const loadingState = ReceiveState(isLoading: true, error: null);
      _lastState = loadingState;
      state = loadingState;
      await loadCurrentAddress();
      await _loadAddressHistory(currentAddressOverride: state.currentAddress);
      _lastState = state;
    } catch (e, stackTrace) {
      debugPrint('Error in _init(): $e');
      debugPrint('Stack trace: $stackTrace');
      // Ensure error is set if initialization fails
      final errorState = ReceiveState(error: e.toString(), isLoading: false);
      _lastState = errorState;
      state = errorState;
    }
  }

  /// Load current receive address
  ///
  /// If address was previously shared, automatically rotate to new one
  Future<void> loadCurrentAddress() async {
    state = state.copyWith(isLoading: true, error: null);
    _lastState = state;

    try {
      final walletId = _requireWallet();
      final isDecoy = ref.read(decoyModeProvider);

      if (isDecoy) {
        final entry = DecoyData.currentAddress();
        _currentAddressShared = false;
        state = state.copyWith(
          currentAddress: entry.address,
          isLoading: false,
          addressWasShared: false,
          diversifierIndex: entry.index,
        );
        _lastState = state;
        return;
      }

      // Get current receive address from FFI
      final address = await FfiBridge.currentReceiveAddress(walletId);

      // Reset shared flag for new address
      _currentAddressShared = false;

      state = state.copyWith(
        currentAddress: address,
        isLoading: false,
        addressWasShared: false,
      );
      _lastState = state;
    } catch (e) {
      state = state.copyWith(error: e.toString(), isLoading: false);
      _lastState = state;
    }
  }

  /// Generate a new receive address via diversifier rotation
  ///
  /// This ALWAYS generates a fresh address (no reuse)
  /// Automatically skips addresses that already have balances (important for recovery/rescan)
  /// Previous address is added to history
  Future<void> generateNewAddress() async {
    state = state.copyWith(isLoading: true, error: null);
    _lastState = state;

    try {
      final walletId = _requireWallet();
      final isDecoy = ref.read(decoyModeProvider);

      // Mark old address as used in history before rotating
      if (state.currentAddress != null) {
        _markAddressAsShared(state.currentAddress!);
      }

      if (isDecoy) {
        final entry = DecoyData.generateNextAddress();
        _currentAddressShared = false;
        state = state.copyWith(
          currentAddress: entry.address,
          isLoading: false,
          addressWasShared: false,
          diversifierIndex: entry.index,
        );
        await _loadAddressHistory(currentAddressOverride: entry.address);
        _lastState = state;
        return;
      }

      // Use rotation service to get next unused address
      // This automatically skips addresses with existing balances (prevents reuse after recovery)
      final rotationService = ref.read(addressRotationServiceProvider);
      final newAddress = await rotationService.manualRotate(walletId);

      // Reset shared flag for fresh address
      _currentAddressShared = false;

      state = state.copyWith(
        currentAddress: newAddress,
        isLoading: false,
        addressWasShared: false,
        diversifierIndex: state.diversifierIndex + 1,
      );
      _lastState = state;

      // Reload address history to include old address
      await _loadAddressHistory(
        currentAddressOverride: newAddress,
        forceCurrentAddress: true,
      );
      _lastState = state;
    } catch (e) {
      state = state.copyWith(error: e.toString(), isLoading: false);
      _lastState = state;
    }
  }

  /// Mark an address as shared in history
  void _markAddressAsShared(String address) {
    final updatedHistory = state.addressHistory.map((info) {
      if (info.address == address) {
        return info.copyWith(wasShared: true);
      }
      return info;
    }).toList();

    state = state.copyWith(addressHistory: updatedHistory);
    _lastState = state;
  }

  /// Copy address to clipboard with protection
  ///
  /// Also marks the address as shared (for privacy tracking)
  Future<void> copyAddress(
    BuildContext context, {
    String? value,
    String? successMessage,
  }) async {
    final text = value ?? state.currentAddress;
    if (text == null || text.isEmpty) return;

    try {
      await ClipboardManager.copyAddress(text);

      // Mark address as shared (for privacy awareness)
      _currentAddressShared = true;
      state = state.copyWith(addressWasShared: true);
      _lastState = state;

      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              successMessage ?? 'Address copied! Will clear in 60 seconds'.tr,
            ),
            duration: const Duration(seconds: 2),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to copy: {error}'.trArgs({'error': e})),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  /// Copy specific address from history
  Future<void> copySpecificAddress(
    BuildContext context,
    AddressInfo info,
  ) async {
    try {
      await ClipboardManager.copyAddress(info.address);

      if (context.mounted) {
        final message = info.label == null
            ? 'Address copied! Will clear in 30 seconds'.tr
            : 'Address ({label}) copied! Will clear in 30 seconds'.trArgs({
                'label': info.label,
              });
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(message),
            duration: const Duration(seconds: 2),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to copy: {error}'.trArgs({'error': e})),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  /// Share address (opens share sheet)
  ///
  /// Also marks the address as shared (for privacy tracking)
  Future<void> shareAddress(
    BuildContext context, {
    String? value,
    String? successMessage,
  }) async {
    final text = value ?? state.currentAddress;
    if (text == null || text.isEmpty) return;

    try {
      await SharePlus.instance.share(ShareParams(text: text));

      // Mark address as shared (for privacy awareness)
      _currentAddressShared = true;
      state = state.copyWith(addressWasShared: true);
      _lastState = state;

      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(successMessage ?? 'Address ready to share'.tr),
            duration: const Duration(seconds: 2),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to share: {error}'.trArgs({'error': e})),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  /// Label an address
  Future<void> labelAddress(String address, String label) async {
    try {
      final walletId = _requireWallet();

      // Call FFI to label address
      await FfiBridge.labelAddress(walletId, address, label);

      // Reload address history
      await _loadAddressHistory(currentAddressOverride: state.currentAddress);
    } catch (e) {
      // Error handling
      state = state.copyWith(error: e.toString());
      _lastState = state;
    }
  }

  /// Update address color tag
  Future<void> setAddressColorTag(
    String address,
    AddressBookColorTag tag,
  ) async {
    try {
      final walletId = _requireWallet();
      await FfiBridge.setAddressColorTag(walletId, address, tag);
      await _loadAddressHistory(currentAddressOverride: state.currentAddress);
    } catch (e) {
      state = state.copyWith(error: e.toString());
      _lastState = state;
    }
  }

  Future<void> setAddressPinned(
    AddressInfo address, {
    required bool isPinned,
  }) async {
    final addressId = address.addressId;
    if (addressId == null) {
      throw StateError('Address preferences are unavailable');
    }
    final walletId = _requireWallet();
    await FfiBridge.setAddressPinned(walletId, addressId, isPinned);
    _replaceAddress(
      address.copyWith(
        isPinned: isPinned,
        isArchived: address.isArchived && !isPinned,
      ),
    );
  }

  Future<void> setAddressArchived(
    AddressInfo address, {
    required bool isArchived,
  }) async {
    final addressId = address.addressId;
    if (addressId == null) {
      throw StateError('Address preferences are unavailable');
    }
    final walletId = _requireWallet();
    await FfiBridge.setAddressArchived(walletId, addressId, isArchived);
    _replaceAddress(
      address.copyWith(
        isArchived: isArchived,
        isPinned: address.isPinned && !isArchived,
      ),
    );
  }

  void _replaceAddress(AddressInfo replacement) {
    final updatedHistory = state.addressHistory
        .map(
          (address) => address.addressId == replacement.addressId
              ? replacement
              : address,
        )
        .toList(growable: false);
    state = state.copyWith(addressHistory: updatedHistory);
    _lastState = state;
  }

  /// Load address history with diversifier indices
  Future<void> _loadAddressHistory({
    String? currentAddressOverride,
    bool forceCurrentAddress = false,
  }) async {
    try {
      final walletId = _walletId;
      if (walletId == null) return;
      final isDecoy = ref.read(decoyModeProvider);

      if (isDecoy) {
        final currentEntry = DecoyData.currentAddress();
        final currentAddress = currentAddressOverride ?? currentEntry.address;
        final history = DecoyData.addressHistory()
            .map(
              (entry) => AddressInfo(
                address: entry.address,
                createdAt: entry.createdAt,
                isActive: entry.address == currentAddress,
                diversifierIndex: entry.index,
                wasShared: true,
                balance: BigInt.zero,
                spendable: BigInt.zero,
                pending: BigInt.zero,
              ),
            )
            .toList();

        state = state.copyWith(
          addressHistory: history,
          currentAddress: currentAddress,
        );
        _lastState = state;
        return;
      }

      var keys = state.keyGroups;
      try {
        keys = await FfiBridge.listKeyGroups(walletId);
      } catch (error) {
        // A failed name lookup must not suppress balances or addresses.
        debugPrint('Failed to refresh receive key groups: $error');
      }
      final keysById = {for (final key in keys) key.id: key};
      if (!forceCurrentAddress) {
        try {
          final receiveKeys = keys
              .where(needsReceiveAddressPreparation)
              .toList();
          if (receiveKeys.isNotEmpty) {
            await _ensureKeyGroupAddresses(walletId, receiveKeys);
          }
        } catch (e) {
          debugPrint('Failed to prepare key group addresses: $e');
        }
      }

      // Get address balances from FFI
      final addresses = await FfiBridge.listAddressBalances(walletId);
      final preferences = await FfiBridge.listAddressDisplayPreferences(
        walletId,
      );
      final preferencesByAddressId = {
        for (final preference in preferences) preference.addressId: preference,
      };
      final currentAddress =
          forceCurrentAddress && currentAddressOverride != null
          ? currentAddressOverride
          : await FfiBridge.currentReceiveAddress(walletId);

      if (!ref.mounted || walletId != _walletId) return;

      // Convert FFI AddressBalanceInfo to local AddressInfo with balance tracking
      final history = addresses.map((ffiAddr) {
        final createdAt = ffiAddr.createdAt > 0
            ? DateTime.fromMillisecondsSinceEpoch(
                ffiAddr.createdAt * 1000,
                isUtc: true,
              ).toLocal()
            : DateTime.fromMillisecondsSinceEpoch(0, isUtc: true).toLocal();
        return AddressInfo(
          addressId: ffiAddr.addressId,
          keyId: ffiAddr.keyId,
          keyLabel: keysById[ffiAddr.keyId] == null
              ? null
              : receiveKeyGroupLabel(keysById[ffiAddr.keyId]!),
          seedAccountIndex: keysById[ffiAddr.keyId]?.seedAccountIndex,
          address: ffiAddr.address,
          label: ffiAddr.label,
          createdAt: createdAt,
          isActive: ffiAddr.address == currentAddress,
          diversifierIndex: ffiAddr.diversifierIndex,
          wasShared: true, // Assume all historical addresses were shared
          colorTag: AddressBookColorTag.fromValue(ffiAddr.colorTag.index),
          balance: ffiAddr.balance,
          spendable: ffiAddr.spendable,
          pending: ffiAddr.pending,
          isPinned:
              preferencesByAddressId[ffiAddr.addressId]?.isPinned ?? false,
          isArchived:
              preferencesByAddressId[ffiAddr.addressId]?.isArchived ?? false,
        );
      }).toList()..sort((a, b) => b.createdAt.compareTo(a.createdAt));

      state = state.copyWith(
        addressHistory: history,
        currentAddress: currentAddress,
        keyGroups: keys,
      );
      _lastState = state;
    } catch (e) {
      // Silently fail for address history (non-critical)
      debugPrint('Failed to load address history: $e');
    }
  }

  /// Refresh address history (balances + pending status) with throttling.
  ///
  /// Use this for live UI updates while the receive page is open so pending
  /// amounts clear automatically once confirmations reach spendable depth.
  Future<void> refreshAddressHistory({
    Duration minInterval = const Duration(seconds: 2),
    bool force = false,
  }) async {
    if (_walletId == null) return;
    if (_addressRefreshInFlight) return;

    final now = DateTime.now();
    if (!force && _lastAddressRefreshAt != null) {
      if (now.difference(_lastAddressRefreshAt!) < minInterval) {
        return;
      }
    }

    _addressRefreshInFlight = true;
    _lastAddressRefreshAt = now;
    try {
      await _loadAddressHistory(currentAddressOverride: state.currentAddress);
    } finally {
      _addressRefreshInFlight = false;
    }
  }

  bool get hasPendingAddressBalances {
    return state.addressHistory.any((address) => address.pending > BigInt.zero);
  }

  /// Check if we should suggest generating a new address
  /// (e.g., if current address was already shared)
  bool get shouldSuggestNewAddress {
    return _currentAddressShared || state.addressWasShared;
  }

  WalletId _requireWallet() {
    final wallet = _walletId ?? ref.read(activeWalletProvider);
    if (wallet == null) {
      throw Exception('No active wallet');
    }
    return wallet;
  }

  Future<void> _ensureKeyGroupAddresses(
    WalletId walletId,
    List<KeyGroupInfo> keys,
  ) async {
    for (final key in keys) {
      try {
        final addresses = await FfiBridge.listAddressesForKey(walletId, key.id);
        if (addresses.isNotEmpty) {
          continue;
        }
        final useIronwood = key.hasIronwood;
        if (!useIronwood && !key.hasSapling) {
          continue;
        }
        await FfiBridge.generateAddressForKey(
          walletId: walletId,
          keyId: key.id,
          useIronwood: useIronwood,
        );
      } catch (e) {
        debugPrint('Failed to prepare key group address: $e');
      }
    }
  }
}

/// Seed accounts restored from older wallets need the same address preparation
/// as imported spending keys. Never rotate an existing group's address here.
bool needsReceiveAddressPreparation(KeyGroupInfo key) =>
    key.spendable && (key.hasSapling || key.hasIronwood);

String receiveKeyGroupLabel(KeyGroupInfo key) {
  final index = key.seedAccountIndex;
  final label = key.label?.trim();
  if (index != null) {
    final account = 'Seed account {index}'.trArgs({'index': index});
    if (label == null ||
        label.isEmpty ||
        label == 'Seed' ||
        label == 'Seed account $index') {
      return account;
    }
    return '$label · $account';
  }
  if (label != null && label.isNotEmpty) return label;
  return key.keyType == KeyTypeInfo.importedViewing
      ? 'Viewing key'.tr
      : 'Imported spending key'.tr;
}

/// Provider for receive screen
final receiveViewModelProvider =
    NotifierProvider.autoDispose<ReceiveViewModel, ReceiveState>(
      ReceiveViewModel.new,
    );
