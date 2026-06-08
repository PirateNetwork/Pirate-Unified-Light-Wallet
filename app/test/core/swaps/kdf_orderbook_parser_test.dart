import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/kdf_orderbook_parser.dart';
import 'package:test/test.dart';

void main() {
  test('parses KDF v2 orderbook entries with multi-representation values', () {
    final book = parseKdfOrderbook({
      'result': {
        'asks': [
          {
            'price': {
              'decimal': '0.0012',
              'rational': {'numer': '12', 'denom': '10000'},
              'fraction': {'numer': '12', 'denom': '10000'},
            },
            'base_max_volume': {
              'decimal': '10.5',
              'rational': {'numer': '21', 'denom': '2'},
              'fraction': {'numer': '21', 'denom': '2'},
            },
            'uuid': 'ask-1',
          },
        ],
        'bids': [
          {
            'price': {'decimal': '0.0011'},
            'base_max_volume': {'decimal': '4.25'},
            'uuid': 'bid-1',
          },
        ],
      },
    });

    expect(book.asks, hasLength(1));
    expect(book.asks.single.priceLtcPerArrr, Decimal.parse('0.0012'));
    expect(book.asks.single.arrrAmount, Decimal.parse('10.5'));
    expect(book.asks.single.orderId, 'ask-1');
    expect(book.bids.single.priceLtcPerArrr, Decimal.parse('0.0011'));
    expect(book.bids.single.arrrAmount, Decimal.parse('4.25'));
  });

  test('parses direct orderbook responses and map-based sides', () {
    final book = parseKdfOrderbook({
      'asks': {
        'ask-1': {'price': '0.002', 'base_max_volume': '2', 'uuid': 'ask-1'},
      },
      'bids': {
        'bid-1': {'price': '0.0018', 'base_max_volume': '3', 'uuid': 'bid-1'},
      },
    });

    expect(book.asks.single.priceLtcPerArrr, Decimal.parse('0.002'));
    expect(book.asks.single.arrrAmount, Decimal.parse('2'));
    expect(book.bids.single.priceLtcPerArrr, Decimal.parse('0.0018'));
    expect(book.bids.single.arrrAmount, Decimal.parse('3'));
  });

  test('parses nested entry wrappers and drops malformed levels', () {
    final book = parseKdfOrderbook({
      'result': {
        'orderbook': {
          'asks': [
            {
              'entry': {
                'price': {'decimal': '0.003'},
                'base_max_volume': {'decimal': '5'},
                'uuid': 'ask-2',
              },
            },
            {
              'price': {'decimal': '0'},
              'base_max_volume': {'decimal': '5'},
            },
          ],
          'bids': const <Object?>[],
        },
      },
    });

    expect(book.asks, hasLength(1));
    expect(book.asks.single.priceLtcPerArrr, Decimal.parse('0.003'));
    expect(book.asks.single.arrrAmount, Decimal.parse('5'));
    expect(book.asks.single.orderId, 'ask-2');
    expect(book.bids, isEmpty);
  });
}
