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
}
