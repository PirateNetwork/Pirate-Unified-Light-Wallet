import 'receive_viewmodel.dart';

enum AddressHistorySection { visible, archived }

enum AddressHistorySort { newest, oldest, balanceHigh, balanceLow }

List<AddressInfo> selectAddressHistory({
  required List<AddressInfo> addresses,
  required AddressHistorySection section,
  required AddressHistorySort sort,
  String query = '',
  int? keyId,
}) {
  final normalizedQuery = query.trim().toLowerCase();
  final showArchived = section == AddressHistorySection.archived;
  final selected = addresses
      .where((address) {
        if (address.isArchived != showArchived) return false;
        if (keyId != null && address.keyId != keyId) return false;
        if (normalizedQuery.isEmpty) return true;
        return address.address.toLowerCase().contains(normalizedQuery) ||
            (address.label?.toLowerCase().contains(normalizedQuery) ?? false) ||
            (address.keyLabel?.toLowerCase().contains(normalizedQuery) ??
                false) ||
            (address.seedAccountIndex != null &&
                'seed account ${address.seedAccountIndex}'.contains(
                  normalizedQuery,
                )) ||
            address.diversifierIndex.toString().contains(normalizedQuery);
      })
      .toList(growable: false);

  final sorted = List<AddressInfo>.from(selected)
    ..sort((left, right) {
      final priorityOrder = _priority(right).compareTo(_priority(left));
      if (priorityOrder != 0) return priorityOrder;

      final requestedOrder = switch (sort) {
        AddressHistorySort.newest => right.createdAt.compareTo(left.createdAt),
        AddressHistorySort.oldest => left.createdAt.compareTo(right.createdAt),
        AddressHistorySort.balanceHigh => right.balance.compareTo(left.balance),
        AddressHistorySort.balanceLow => left.balance.compareTo(right.balance),
      };
      if (requestedOrder != 0) return requestedOrder;
      return right.diversifierIndex.compareTo(left.diversifierIndex);
    });
  return List.unmodifiable(sorted);
}

int _priority(AddressInfo address) {
  if (address.isActive) return 2;
  if (address.isPinned) return 1;
  return 0;
}
