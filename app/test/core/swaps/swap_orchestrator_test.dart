import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/swap_amount_utils.dart';
import 'package:test/test.dart';

void main() {
  group('swap amount utils', () {
    test('converts ARRR to zatoshis', () {
      expect(
        arrrToZatoshis(Decimal.parse('1.23456789')),
        BigInt.parse('123456789'),
      );
    });

    test('converts zatoshis back to ARRR', () {
      expect(
        arrrFromZatoshis(BigInt.parse('123456789')),
        Decimal.parse('1.23456789'),
      );
    });

    test('formats trailing zeros away', () {
      expect(formatSwapAmount(Decimal.parse('1.50000000')), '1.5');
    });

    test('formats fixed precision for order book prices', () {
      expect(
        formatSwapAmountFixed(Decimal.parse('0.01'), fractionDigits: 6),
        '0.010000',
      );
    });

    test('rounds display-only amount text to fixed precision', () {
      expect(formatSwapAmountTextRounded('12.345'), '12.35');
      expect(formatSwapAmountTextRounded('12'), '12.00');
      expect(formatSwapAmountTextRounded('not a number'), 'not a number');
    });
  });
}
