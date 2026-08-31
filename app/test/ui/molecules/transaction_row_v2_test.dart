import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/ui/molecules/transaction_row_v2.dart';

Widget _testApp({
  required double width,
  required double textScale,
  String amountText = '+77999.9997 ARRR',
  String? addressLabel,
  bool isExpired = false,
  VoidCallback? onTap,
}) {
  return MaterialApp(
    theme: PTheme.dark(),
    home: MediaQuery(
      data: MediaQueryData(
        size: Size(width, 800),
        textScaler: TextScaler.linear(textScale),
      ),
      child: Scaffold(
        body: Align(
          alignment: Alignment.topCenter,
          child: SizedBox(
            width: width,
            child: TransactionRowV2(
              isReceived: true,
              isConfirmed: true,
              isExpired: isExpired,
              amountText: amountText,
              timestamp: DateTime.now().subtract(const Duration(days: 3)),
              memo: 'Thank you',
              addressLabel: addressLabel,
              onTap: onTap,
            ),
          ),
        ),
      ),
    ),
  );
}

void _expectVerticalHierarchy(WidgetTester tester) {
  final direction = tester.getRect(find.byKey(TransactionRowV2.directionKey));
  final amount = tester.getRect(find.byKey(TransactionRowV2.amountKey));
  final metadata = tester.getRect(find.byKey(TransactionRowV2.metadataKey));
  final row = tester.getRect(find.byType(TransactionRowV2));

  expect(amount.top, greaterThanOrEqualTo(direction.bottom));
  expect(metadata.top, greaterThanOrEqualTo(amount.bottom));
  expect(amount.left, greaterThanOrEqualTo(row.left));
  expect(amount.right, lessThanOrEqualTo(row.right));
  expect(metadata.right, lessThanOrEqualTo(row.right));
}

void main() {
  testWidgets('stacks long amounts below transaction identity on phones', (
    tester,
  ) async {
    await tester.pumpWidget(
      _testApp(
        width: 320,
        textScale: 1,
        addressLabel: 'Primary shielded savings account',
      ),
    );

    _expectVerticalHierarchy(tester);
    expect(find.text('+77999.9997 ARRR'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('preserves the mobile hierarchy at large text scales', (
    tester,
  ) async {
    await tester.pumpWidget(
      _testApp(
        width: 320,
        textScale: 2,
        amountText: '+123456789.1234 ARRR',
        addressLabel: 'Long localized account label',
      ),
    );

    _expectVerticalHierarchy(tester);
    expect(find.text('+123456789.1234 ARRR'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('keeps amount and metadata in separate columns when wide', (
    tester,
  ) async {
    await tester.pumpWidget(_testApp(width: 760, textScale: 1));

    final direction = tester.getRect(find.byKey(TransactionRowV2.directionKey));
    final metadata = tester.getRect(find.byKey(TransactionRowV2.metadataKey));
    final amount = tester.getRect(find.byKey(TransactionRowV2.amountKey));

    expect(direction.overlaps(amount), isFalse);
    expect(metadata.overlaps(amount), isFalse);
    expect(amount.left, greaterThan(direction.left));
    expect(tester.takeException(), isNull);
  });

  testWidgets('announces one actionable transaction summary', (tester) async {
    var taps = 0;
    await tester.pumpWidget(
      _testApp(width: 320, textScale: 1, onTap: () => taps += 1),
    );

    final summary = find.byKey(TransactionRowV2.semanticsKey);
    expect(summary, findsOneWidget);
    final semantics = tester.widget<Semantics>(summary);
    expect(
      semantics.properties.label,
      matches(RegExp(r'Received, \+77999\.9997 ARRR, Confirmed, .*Has memo')),
    );
    expect(semantics.container, isTrue);
    expect(semantics.excludeSemantics, isTrue);
    expect(semantics.properties.button, isTrue);
    expect(semantics.properties.onTap, isNotNull);

    semantics.properties.onTap!();
    expect(taps, 1);
  });

  testWidgets('announces an expired transaction as expired', (tester) async {
    await tester.pumpWidget(
      _testApp(width: 320, textScale: 1, isExpired: true),
    );

    expect(find.text('Expired'), findsOneWidget);
    final semantics = tester.widget<Semantics>(
      find.byKey(TransactionRowV2.semanticsKey),
    );
    expect(semantics.properties.label, contains('Expired'));
    expect(semantics.properties.label, isNot(contains('Confirmed')));
    expect(tester.takeException(), isNull);
  });
}
