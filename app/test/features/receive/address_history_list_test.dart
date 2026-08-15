import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/receive/receive_viewmodel.dart';
import 'package:pirate_wallet/features/receive/widgets/address_history_list.dart';
import 'package:pirate_wallet/ui/molecules/p_card.dart';

AddressInfo _address(int index, {bool archived = false}) {
  return AddressInfo(
    addressId: index,
    address: 'zs1address$index',
    label: 'Address $index',
    createdAt: DateTime.fromMillisecondsSinceEpoch(index * 1000),
    diversifierIndex: index,
    isArchived: archived,
  );
}

Widget _history(List<AddressInfo> addresses, {bool showArchived = false}) {
  return MaterialApp(
    theme: ThemeData(splashFactory: InkRipple.splashFactory),
    home: Scaffold(
      body: CustomScrollView(
        slivers: [
          AddressHistorySliver(
            addresses: addresses,
            showArchived: showArchived,
            onCopy: (_) {},
            onLabel: (_) {},
            onColorTag: (_) {},
            onTogglePin: (_) {},
            onArchive: (_) {},
            onOpen: (_) {},
          ),
        ],
      ),
    ),
  );
}

void main() {
  testWidgets('builds a large address history lazily', (tester) async {
    final addresses = List.generate(3001, _address);

    await tester.pumpWidget(_history(addresses));
    await tester.pump();

    expect(find.byKey(const ValueKey('address-history-0')), findsOneWidget);
    expect(find.byKey(const ValueKey('address-history-3000')), findsNothing);
    expect(find.byType(PCard).evaluate().length, lessThan(30));
  });

  testWidgets('exposes pinning as a direct address action', (tester) async {
    AddressInfo? toggled;
    final address = _address(4);

    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: InkRipple.splashFactory),
        home: Scaffold(
          body: CustomScrollView(
            slivers: [
              AddressHistorySliver(
                addresses: [address],
                onCopy: (_) {},
                onLabel: (_) {},
                onColorTag: (_) {},
                onTogglePin: (value) => toggled = value,
                onArchive: (_) {},
                onOpen: (_) {},
              ),
            ],
          ),
        ),
      ),
    );

    await tester.tap(find.byIcon(Icons.push_pin_outlined));
    expect(toggled, same(address));
  });

  testWidgets('keeps address actions large enough for touch input', (
    tester,
  ) async {
    await tester.pumpWidget(_history([_address(7)]));
    await tester.pump();

    for (final icon in [Icons.push_pin_outlined, Icons.copy]) {
      final button = find.widgetWithIcon(IconButton, icon);
      final size = tester.getSize(button);
      expect(size.width, greaterThanOrEqualTo(44));
      expect(size.height, greaterThanOrEqualTo(44));
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets('explains an empty archived view', (tester) async {
    await tester.pumpWidget(_history(const [], showArchived: true));

    expect(find.text('No archived addresses.'), findsOneWidget);
    expect(
      find.text('Addresses you archive will appear here.'),
      findsOneWidget,
    );
  });
}
