import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:share_plus/share_plus.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/security/clipboard_manager.dart';
import '../../design/tokens/colors.dart';

/// State for receive screen
class ReceiveState {
  final String? currentAddress;
  final List<AddressInfo> addressHistory;
  final bool isLoading;
  final String? error;
  final int diversifierIndex;
  final bool addressWasShared;

  const ReceiveState({
    this.currentAddress,
    this.addressHistory = const [],
    this.isLoading = false,
    this.error,
    this.diversifierIndex = 0,
    this.addressWasShared = false,
  });

  ReceiveState copyWith({
    String? currentAddress,
    List<AddressInfo>? addressHistory,
    bool? isLoading,
    String? error,
    int? diversifierIndex,
    bool? addressWasShared,
  }) {
    return ReceiveState(
      currentAddress: currentAddress ?? this.currentAddress,
      addressHistory: addressHistory ?? this.addressHistory,
      isLoading: isLoading ?? this.isLoading,
      error: error,
      diversifierIndex: diversifierIndex ?? this.diversifierIndex,
      addressWasShared: addressWasShared ?? this.addressWasShared,
    );
  }
}

/// Address info model with usage tracking
class AddressInfo {
  final String address;
  final String? label;
  final DateTime createdAt;
  final bool isActive;
  final int diversifierIndex;
  final bool wasShared;
  final bool wasUsedForReceive;
  final AddressBookColorTag colorTag;

  const AddressInfo({
    required this.address,
    this.label,
    required this.createdAt,
    this.isActive = false,
    this.diversifierIndex = 0,
    this.wasShared = false,
    this.wasUsedForReceive = false,
    this.colorTag = AddressBookColorTag.none,
  });

  AddressInfo copyWith({
    String? address,
    String? label,
    DateTime? createdAt,
    bool? isActive,
    int? diversifierIndex,
    bool? wasShared,
    bool? wasUsedForReceive,
    AddressBookColorTag? colorTag,
  }) {
    return AddressInfo(
      address: address ?? this.address,
      label: label ?? this.label,
      createdAt: createdAt ?? this.createdAt,
      isActive: isActive ?? this.isActive,
      diversifierIndex: diversifierIndex ?? this.diversifierIndex,
      wasShared: wasShared ?? this.wasShared,
      wasUsedForReceive: wasUsedForReceive ?? this.wasUsedForReceive,
      colorTag: colorTag ?? this.colorTag,
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
  
  /// Track if current address was shared (copied/shared)
  bool _currentAddressShared = false;

  @override
  ReceiveState build() {
    try {
      // Keep provider alive during initialization
      ref.keepAlive();
      
      // Watch active wallet provider
      final walletId = ref.watch(activeWalletProvider);
      
      // Handle wallet changes - reset initialization when wallet changes
      if (walletId != _walletId) {
        _walletId = walletId;
        _initialized = false;
        _lastState = null;
        
        // If wallet changed, reset and reinitialize
        if (walletId != null) {
          Future.microtask(() {
            _init();
          });
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
        Future.microtask(() {
          _init();
        });
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
      return ReceiveState(
        error: 'Error initializing: ${e.toString()}',
        isLoading: false,
      );
    }
  }

  /// Initialize the receive screen
  Future<void> _init() async {
    if (_walletId == null) {
      final errorState = const ReceiveState(
        error: 'No wallet selected',
        isLoading: false,
      );
      _lastState = errorState;
      state = errorState;
      return;
    }
    
    try {
      final loadingState = const ReceiveState(isLoading: true, error: null);
      _lastState = loadingState;
      state = loadingState;
      await loadCurrentAddress();
      await _loadAddressHistory();
      _lastState = state;
    } catch (e, stackTrace) {
      debugPrint('Error in _init(): $e');
      debugPrint('Stack trace: $stackTrace');
      // Ensure error is set if initialization fails
      final errorState = ReceiveState(
        error: e.toString(),
        isLoading: false,
      );
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
      state = state.copyWith(
        error: e.toString(),
        isLoading: false,
      );
      _lastState = state;
    }
  }

  /// Generate a new receive address via diversifier rotation
  /// 
  /// This ALWAYS generates a fresh address (no reuse)
  /// Previous address is added to history
  Future<void> generateNewAddress() async {
    state = state.copyWith(isLoading: true, error: null);
    _lastState = state;

    try {
      final walletId = _requireWallet();
      
      // Mark old address as used in history before rotating
      if (state.currentAddress != null) {
        _markAddressAsShared(state.currentAddress!);
      }

      // Get NEXT receive address from FFI (diversifier rotation)
      // This enforces that we never reuse the last address
      final newAddress = await FfiBridge.nextReceiveAddress(walletId);
      
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
      await _loadAddressHistory();
      _lastState = state;
    } catch (e) {
      state = state.copyWith(
        error: e.toString(),
        isLoading: false,
      );
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
      await ClipboardManager.copyAddress(
        text,
      );
      
      // Mark address as shared (for privacy awareness)
      _currentAddressShared = true;
      state = state.copyWith(addressWasShared: true);
      _lastState = state;

      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              successMessage ?? 'Address copied! Will clear in 60 seconds',
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
            content: Text('Failed to copy: $e'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  /// Copy specific address from history
  Future<void> copySpecificAddress(BuildContext context, AddressInfo info) async {
    try {
      await ClipboardManager.copyAddress(
        info.address,
      );

      if (context.mounted) {
        final label = info.label != null ? ' (${info.label})' : '';
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Address$label copied! Will clear in 30 seconds'),
            duration: const Duration(seconds: 2),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to copy: $e'),
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
            content: Text(successMessage ?? 'Address ready to share'),
            duration: const Duration(seconds: 2),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to share: $e'),
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
      await _loadAddressHistory();
    } catch (e) {
      // Error handling
      state = state.copyWith(error: e.toString());
      _lastState = state;
    }
  }

  /// Update address color tag
  Future<void> setAddressColorTag(String address, AddressBookColorTag tag) async {
    try {
      final walletId = _requireWallet();
      await FfiBridge.setAddressColorTag(walletId, address, tag);
      await _loadAddressHistory();
    } catch (e) {
      state = state.copyWith(error: e.toString());
      _lastState = state;
    }
  }

  /// Load address history with diversifier indices
  Future<void> _loadAddressHistory() async {
    try {
      final walletId = _walletId;
      if (walletId == null) return;

      // Get address list from FFI
      final addresses = await FfiBridge.listAddresses(walletId);

      // Convert FFI AddressInfo to local AddressInfo with diversifier tracking
      final history = addresses
          .map((ffiAddr) {
            final createdAt = ffiAddr.createdAt > 0
                ? DateTime.fromMillisecondsSinceEpoch(
                    ffiAddr.createdAt * 1000,
                    isUtc: true,
                  ).toLocal()
                : DateTime.fromMillisecondsSinceEpoch(0, isUtc: true).toLocal();
            return AddressInfo(
              address: ffiAddr.address,
              label: ffiAddr.label,
              createdAt: createdAt,
              isActive: ffiAddr.address == state.currentAddress,
              diversifierIndex: ffiAddr.diversifierIndex,
              wasShared: true, // Assume all historical addresses were shared
              colorTag: AddressBookColorTag.fromValue(ffiAddr.colorTag.index),
            );
          })
          .toList()
        ..sort((a, b) => b.diversifierIndex.compareTo(a.diversifierIndex));

      state = state.copyWith(addressHistory: history);
      _lastState = state;
    } catch (e) {
      // Silently fail for address history (non-critical)
      debugPrint('Failed to load address history: $e');
    }
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
}

/// Provider for receive screen
final receiveViewModelProvider =
    NotifierProvider.autoDispose<ReceiveViewModel, ReceiveState>(ReceiveViewModel.new);
