import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/receive/address_history_selection.dart';
import 'package:pirate_wallet/features/receive/receive_viewmodel.dart';

AddressInfo _address(
  int index, {
  String? label,
  bool active = false,
  bool pinned = false,
  bool archived = false,
  int balance = 0,
}) {
  return AddressInfo(
    addressId: index,
    address: 'zs1address$index',
    label: label,
    createdAt: DateTime.fromMillisecondsSinceEpoch(index * 1000),
    diversifierIndex: index,
    isActive: active,
    isPinned: pinned,
    isArchived: archived,
    balance: BigInt.from(balance),
  );
}

void main() {
  test('keeps current and pinned addresses ahead of the selected sort', () {
    final result = selectAddressHistory(
      addresses: [
        _address(1, balance: 100),
        _address(2, pinned: true, balance: 10),
        _address(3, active: true),
        _address(4, balance: 200),
      ],
      section: AddressHistorySection.visible,
      sort: AddressHistorySort.balanceHigh,
    );

    expect(result.map((address) => address.addressId), [3, 2, 4, 1]);
  });

  test('separates archived addresses and searches labels or indices', () {
    final addresses = [
      _address(7, label: 'Mining'),
      _address(8, label: 'Savings', archived: true),
      _address(9, archived: true),
    ];

    expect(
      selectAddressHistory(
        addresses: addresses,
        section: AddressHistorySection.visible,
        sort: AddressHistorySort.newest,
      ).map((address) => address.addressId),
      [7],
    );
    expect(
      selectAddressHistory(
        addresses: addresses,
        section: AddressHistorySection.archived,
        sort: AddressHistorySort.newest,
        query: 'savings',
      ).single.addressId,
      8,
    );
    expect(
      selectAddressHistory(
        addresses: addresses,
        section: AddressHistorySection.archived,
        sort: AddressHistorySort.newest,
        query: '9',
      ).single.addressId,
      9,
    );
  });

  test('selects a large address set without truncating it', () {
    final addresses = List.generate(
      3001,
      (index) => _address(index, pinned: index == 10),
    );

    final result = selectAddressHistory(
      addresses: addresses,
      section: AddressHistorySection.visible,
      sort: AddressHistorySort.newest,
    );

    expect(result, hasLength(3001));
    expect(result.first.addressId, 10);
    expect(result[1].addressId, 3000);
  });
}
