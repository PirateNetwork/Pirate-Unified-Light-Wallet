import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/activity/activity_screen.dart';

const _walletId = 'activity-pagination-wallet';

class _TestActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  String? build() => _walletId;
}

TxInfo _transaction(int index) {
  return TxInfo(
    txid: 'transaction-$index',
    height: 2_000_000 - index,
    timestamp: 1_710_000_000 - index,
    amount: 100_000_000 + index,
    fee: BigInt.from(1_000),
    memo: null,
    confirmed: true,
    expired: false,
  );
}

TransactionCursor _cursorFor(TxInfo tx) {
  return TransactionCursor(height: tx.height, txid: tx.txid, amount: tx.amount);
}

void main() {
  testWidgets('loads another activity page near the end of the list', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final transactions = List.generate(80, _transaction);
    var requestCount = 0;
    final container = ProviderContainer(
      overrides: [
        activeWalletProvider.overrideWith(_TestActiveWalletNotifier.new),
        transactionPageLoaderProvider.overrideWith((ref) {
          return (
            String walletId, {
            TransactionCursor? cursor,
            required int pageSize,
          }) async {
            requestCount++;
            if (cursor == null) {
              final first = transactions.take(50).toList();
              return TransactionPage(
                transactions: first,
                nextCursor: _cursorFor(first.last),
              );
            }
            return TransactionPage(
              transactions: transactions.skip(50).toList(),
            );
          };
        }),
        syncProgressStreamProvider.overrideWith((ref) => Stream.value(null)),
        syncStatusProvider.overrideWith((ref) async => null),
      ],
    );

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          theme: ThemeData(splashFactory: NoSplash.splashFactory),
          home: const Scaffold(body: ActivityScreen(useScaffold: false)),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(requestCount, 1);
    expect(
      container.read(activityHistoryProvider).requireValue.transactions.length,
      50,
    );

    await tester.fling(find.byType(ListView), const Offset(0, -6000), 6000);
    await tester.pumpAndSettle();

    expect(requestCount, 2);
    expect(
      container.read(activityHistoryProvider).requireValue.transactions.length,
      80,
    );

    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });
}
