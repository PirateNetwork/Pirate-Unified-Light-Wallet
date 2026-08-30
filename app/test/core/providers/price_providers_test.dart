import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/providers/price_providers.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';

void main() {
  group('CoinPaprika ARRR quotes', () {
    test('parses a positive quote for the requested currency', () {
      final price = parseCoinPaprikaPrice({
        'id': 'arrr-pirate',
        'symbol': 'ARRR',
        'quotes': {
          'USD': {'price': 0.185138009858889},
        },
      }, 'USD');

      expect(price, closeTo(0.185138009858889, 1e-15));
    });

    test('rejects another asset and invalid prices', () {
      expect(
        parseCoinPaprikaPrice({
          'id': 'btc-bitcoin',
          'symbol': 'BTC',
          'quotes': {
            'USD': {'price': 100000},
          },
        }, 'USD'),
        isNull,
      );
      expect(
        parseCoinPaprikaPrice({
          'id': 'arrr-pirate',
          'symbol': 'ARRR',
          'quotes': {
            'USD': {'price': 0},
          },
        }, 'USD'),
        isNull,
      );
      expect(
        parseCoinPaprikaPrice({
          'id': 'arrr-pirate',
          'symbol': 'ARRR',
          'quotes': <String, Object?>{},
        }, 'USD'),
        isNull,
      );
    });

    test('maps only currencies supported by the ticker endpoint', () {
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.usd), 'USD');
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.eur), 'EUR');
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.btc), 'BTC');
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.tryCurrency), 'TRY');
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.aed), isNull);
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.bhd), isNull);
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.kwd), isNull);
      expect(coinPaprikaQuoteCodeFor(CurrencyPreference.sar), isNull);
    });
  });

  group('PriceQuotePoller', () {
    testWidgets('retries quickly after startup transport failure', (
      tester,
    ) async {
      var attempts = 0;
      final poller = PriceQuotePoller<int>(
        fetch: () async {
          attempts += 1;
          return attempts == 1 ? null : 42;
        },
        refreshInterval: const Duration(minutes: 5),
        retryDelays: const [Duration(seconds: 3)],
      );
      final values = <int?>[];
      final subscription = poller.stream.listen(values.add);

      await tester.pump();
      expect(attempts, 1);
      expect(values, [null]);

      await tester.pump(const Duration(seconds: 2));
      expect(attempts, 1);

      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(attempts, 2);
      expect(values, [null, 42]);

      unawaited(subscription.cancel());
      poller.dispose();
      await tester.pump();
    });

    testWidgets('coalesces refresh requests while a fetch is running', (
      tester,
    ) async {
      var attempts = 0;
      final firstFetch = Completer<int?>();
      final poller = PriceQuotePoller<int>(
        fetch: () {
          attempts += 1;
          return attempts == 1 ? firstFetch.future : Future.value(43);
        },
        refreshInterval: const Duration(minutes: 5),
      );
      final values = <int?>[];
      final subscription = poller.stream.listen(values.add);

      await tester.pump();
      poller
        ..refreshNow()
        ..refreshNow();
      expect(attempts, 1);

      firstFetch.complete(42);
      await tester.pump();
      await tester.pump();
      expect(attempts, 2);
      expect(values, [42, 43]);

      unawaited(subscription.cancel());
      poller.dispose();
      await tester.pump();
    });

    testWidgets('keeps the last quote during a temporary outage', (
      tester,
    ) async {
      var attempts = 0;
      final poller = PriceQuotePoller<int>(
        fetch: () async {
          attempts += 1;
          return switch (attempts) {
            1 => 42,
            2 => null,
            _ => 43,
          };
        },
        refreshInterval: const Duration(seconds: 5),
        retryDelays: const [Duration(seconds: 3)],
      );
      final values = <int?>[];
      final subscription = poller.stream.listen(values.add);

      await tester.pump();
      expect(values, [42]);

      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      expect(attempts, 2);
      expect(values, [42]);

      await tester.pump(const Duration(seconds: 3));
      await tester.pump();
      expect(values, [42, 43]);

      unawaited(subscription.cancel());
      poller.dispose();
      await tester.pump();
    });
  });
}
