/// Address book state management with FFI integration
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../models/address_entry.dart';
import '../../../core/ffi/ffi_bridge.dart';

/// Address book state
class AddressBookState {
  final List<AddressEntry> entries;
  final bool isLoading;
  final String? error;
  final String searchQuery;
  final ColorTag? filterColor;
  final bool showFavoritesOnly;

  const AddressBookState({
    this.entries = const [],
    this.isLoading = false,
    this.error,
    this.searchQuery = '',
    this.filterColor,
    this.showFavoritesOnly = false,
  });

  AddressBookState copyWith({
    List<AddressEntry>? entries,
    bool? isLoading,
    String? error,
    String? searchQuery,
    ColorTag? filterColor,
    bool? showFavoritesOnly,
  }) {
    return AddressBookState(
      entries: entries ?? this.entries,
      isLoading: isLoading ?? this.isLoading,
      error: error,
      searchQuery: searchQuery ?? this.searchQuery,
      filterColor: filterColor ?? this.filterColor,
      showFavoritesOnly: showFavoritesOnly ?? this.showFavoritesOnly,
    );
  }

  /// Get filtered entries
  List<AddressEntry> get filteredEntries {
    var result = entries;

    // Filter by favorites
    if (showFavoritesOnly) {
      result = result.where((e) => e.isFavorite).toList();
    }

    // Filter by color
    if (filterColor != null && filterColor != ColorTag.none) {
      result = result.where((e) => e.colorTag == filterColor).toList();
    }

    // Filter by search
    if (searchQuery.isNotEmpty) {
      final query = searchQuery.toLowerCase();
      result = result.where((e) {
        return e.label.toLowerCase().contains(query) ||
            e.address.toLowerCase().contains(query) ||
            (e.notes?.toLowerCase().contains(query) ?? false);
      }).toList();
    }

    // Sort: favorites first, then alphabetically
    result.sort((a, b) {
      if (a.isFavorite != b.isFavorite) {
        return a.isFavorite ? -1 : 1;
      }
      return a.label.compareTo(b.label);
    });

    return result;
  }

  /// Get entries by color
  Map<ColorTag, int> get colorCounts {
    final counts = <ColorTag, int>{};
    for (final entry in entries) {
      counts[entry.colorTag] = (counts[entry.colorTag] ?? 0) + 1;
    }
    return counts;
  }

  /// Get favorite count
  int get favoriteCount => entries.where((e) => e.isFavorite).length;
}

/// Convert FFI color tag to model color tag
ColorTag _ffiColorTagToModel(AddressBookColorTag ffiTag) {
  switch (ffiTag) {
    case AddressBookColorTag.none:
      return ColorTag.none;
    case AddressBookColorTag.red:
      return ColorTag.red;
    case AddressBookColorTag.orange:
      return ColorTag.orange;
    case AddressBookColorTag.yellow:
      return ColorTag.yellow;
    case AddressBookColorTag.green:
      return ColorTag.green;
    case AddressBookColorTag.blue:
      return ColorTag.blue;
    case AddressBookColorTag.purple:
      return ColorTag.purple;
    case AddressBookColorTag.pink:
      return ColorTag.pink;
    case AddressBookColorTag.gray:
      return ColorTag.gray;
  }
}

/// Convert model color tag to FFI color tag
AddressBookColorTag _modelColorTagToFfi(ColorTag modelTag) {
  switch (modelTag) {
    case ColorTag.none:
      return AddressBookColorTag.none;
    case ColorTag.red:
      return AddressBookColorTag.red;
    case ColorTag.orange:
      return AddressBookColorTag.orange;
    case ColorTag.yellow:
      return AddressBookColorTag.yellow;
    case ColorTag.green:
      return AddressBookColorTag.green;
    case ColorTag.blue:
      return AddressBookColorTag.blue;
    case ColorTag.purple:
      return AddressBookColorTag.purple;
    case ColorTag.pink:
      return AddressBookColorTag.pink;
    case ColorTag.gray:
      return AddressBookColorTag.gray;
  }
}

/// Convert FFI entry to model entry
AddressEntry _ffiEntryToModel(AddressBookEntryFfi ffi) {
  return AddressEntry(
    id: ffi.id,
    walletId: ffi.walletId,
    address: ffi.address,
    label: ffi.label,
    notes: ffi.notes,
    colorTag: _ffiColorTagToModel(ffi.colorTag),
    isFavorite: ffi.isFavorite,
    createdAt: ffi.createdAt,
    updatedAt: ffi.updatedAt,
    lastUsedAt: ffi.lastUsedAt,
    useCount: ffi.useCount,
  );
}

/// Address book notifier with FFI integration
class AddressBookNotifier extends Notifier<AddressBookState> {
  final String? _walletId;

  AddressBookNotifier([this._walletId]);

  String get walletId => _walletId ?? '';

  @override
  AddressBookState build() {
    _loadEntries();
    return const AddressBookState();
  }

  /// Load entries from FFI storage
  Future<void> _loadEntries() async {
    state = state.copyWith(isLoading: true, error: null);

    try {
      final ffiEntries = await AddressBookEntryFfi.listAddressBook(walletId);
      final entries = ffiEntries.map(_ffiEntryToModel).toList();

      state = state.copyWith(
        entries: entries,
        isLoading: false,
      );
    } catch (e) {
      state = state.copyWith(
        isLoading: false,
        error: e.toString(),
      );
    }
  }

  /// Refresh entries
  Future<void> refresh() => _loadEntries();

  /// Add new entry via FFI
  Future<AddressEntry?> addEntry({
    required String address,
    required String label,
    String? notes,
    ColorTag colorTag = ColorTag.none,
  }) async {
    try {
      // Validate locally first
      final tempEntry = AddressEntry.create(
        walletId: walletId,
        address: address,
        label: label,
        notes: notes,
        colorTag: colorTag,
      );

      final errors = tempEntry.validate();
      if (errors.isNotEmpty) {
        state = state.copyWith(error: errors.first);
        return null;
      }

      // Add via FFI
      final ffiEntry = await AddressBookEntryFfi.addAddressBookEntry(
        walletId: walletId,
        address: address,
        label: label,
        notes: notes,
        colorTag: _modelColorTagToFfi(colorTag),
      );

      final newEntry = _ffiEntryToModel(ffiEntry);

      state = state.copyWith(
        entries: [...state.entries, newEntry],
        error: null,
      );

      return newEntry;
    } catch (e) {
      state = state.copyWith(error: e.toString());
      return null;
    }
  }

  /// Update entry via FFI
  Future<bool> updateEntry(AddressEntry entry) async {
    try {
      final errors = entry.validate();
      if (errors.isNotEmpty) {
        state = state.copyWith(error: errors.first);
        return false;
      }

      // Update via FFI
      final ffiEntry = await AddressBookEntryFfi.updateAddressBookEntry(
        walletId: entry.walletId,
        id: entry.id,
        label: entry.label,
        notes: entry.notes,
        colorTag: _modelColorTagToFfi(entry.colorTag),
        isFavorite: entry.isFavorite,
      );

      final updated = _ffiEntryToModel(ffiEntry);

      // Update local state
      final entries = state.entries.map((e) {
        return e.id == entry.id ? updated : e;
      }).toList();

      state = state.copyWith(entries: entries, error: null);
      return true;
    } catch (e) {
      state = state.copyWith(error: e.toString());
      return false;
    }
  }

  /// Delete entry via FFI
  Future<bool> deleteEntry(int id) async {
    try {
      final entry = state.entries.firstWhere(
        (e) => e.id == id,
        orElse: () => AddressEntry(
          id: id,
          walletId: walletId,
          address: '',
          label: '',
          createdAt: DateTime.now(),
          updatedAt: DateTime.now(),
        ),
      );

      // Delete via FFI
      await AddressBookEntryFfi.deleteAddressBookEntry(entry.walletId, id);

      // Remove from local state
      final entries = state.entries.where((e) => e.id != id).toList();
      state = state.copyWith(entries: entries, error: null);
      return true;
    } catch (e) {
      state = state.copyWith(error: e.toString());
      return false;
    }
  }

  /// Toggle favorite via FFI
  Future<bool> toggleFavorite(int id) async {
    try {
      final entry = state.entries.firstWhere(
        (e) => e.id == id,
        orElse: () => AddressEntry(
          id: id,
          walletId: walletId,
          address: '',
          label: '',
          createdAt: DateTime.now(),
          updatedAt: DateTime.now(),
        ),
      );

      // Toggle via FFI
      final newFavoriteState = await AddressBookEntryFfi.toggleAddressBookFavorite(
        entry.walletId,
        id,
      );

      // Update local state
      final entries = state.entries.map((e) {
        if (e.id == id) {
          return e.copyWith(
            isFavorite: newFavoriteState,
            updatedAt: DateTime.now(),
          );
        }
        return e;
      }).toList();

      state = state.copyWith(entries: entries, error: null);
      return true;
    } catch (e) {
      state = state.copyWith(error: e.toString());
      return false;
    }
  }

  /// Mark address as used via FFI
  Future<void> markUsed(String address) async {
    try {
      final entry = state.entries.firstWhere(
        (e) => e.address == address,
        orElse: () => AddressEntry(
          id: 0,
          walletId: walletId,
          address: address,
          label: '',
          createdAt: DateTime.now(),
          updatedAt: DateTime.now(),
        ),
      );

      // Mark via FFI
      await AddressBookEntryFfi.markAddressUsed(entry.walletId, address);

      // Update local state
      final entries = state.entries.map((e) {
        if (e.address == address) {
          return e.copyWith(
            lastUsedAt: DateTime.now(),
            useCount: e.useCount + 1,
            updatedAt: DateTime.now(),
          );
        }
        return e;
      }).toList();

      state = state.copyWith(entries: entries);
    } catch (e) {
      // Silent fail for usage tracking
    }
  }

  /// Set search query
  void setSearchQuery(String query) {
    state = state.copyWith(searchQuery: query);
  }

  /// Set color filter
  void setColorFilter(ColorTag? color) {
    state = state.copyWith(filterColor: color);
  }

  /// Toggle favorites filter
  void toggleFavoritesFilter() {
    state = state.copyWith(showFavoritesOnly: !state.showFavoritesOnly);
  }

  /// Clear filters
  void clearFilters() {
    state = state.copyWith(
      searchQuery: '',
      filterColor: null,
      showFavoritesOnly: false,
    );
  }

  /// Get label for address (for transaction history)
  String? getLabelForAddress(String address) {
    final entry = state.entries.firstWhere(
      (e) => e.address == address,
      orElse: () => AddressEntry(
        id: 0,
        walletId: '',
        address: '',
        label: '',
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
      ),
    );
    return entry.label.isEmpty ? null : entry.label;
  }

  /// Get entry by address
  AddressEntry? getByAddress(String address) {
    try {
      return state.entries.firstWhere((e) => e.address == address);
    } catch (_) {
      return null;
    }
  }
}

/// Provider for address book
AddressBookNotifier _createAddressBookNotifier(String walletId) {
  return AddressBookNotifier(walletId);
}

final addressBookProvider =
    NotifierProvider.family<AddressBookNotifier, AddressBookState, String>(
  _createAddressBookNotifier,
);

/// Provider for recently used addresses
final recentAddressesProvider =
    Provider.family<List<AddressEntry>, String>((ref, walletId) {
  final state = ref.watch(addressBookProvider(walletId));
  return state.entries
      .where((e) => e.lastUsedAt != null)
      .toList()
    ..sort((a, b) => (b.lastUsedAt ?? DateTime(0))
        .compareTo(a.lastUsedAt ?? DateTime(0)));
});

/// Provider for favorites
final favoriteAddressesProvider =
    Provider.family<List<AddressEntry>, String>((ref, walletId) {
  final state = ref.watch(addressBookProvider(walletId));
  return state.entries.where((e) => e.isFavorite).toList();
});

/// Provider for label lookup (for transaction history)
final addressLabelProvider =
    FutureProvider.family<String?, (String, String)>((ref, params) async {
  final (walletId, address) = params;
  return AddressBookEntryFfi.getLabelForAddress(walletId, address);
});
