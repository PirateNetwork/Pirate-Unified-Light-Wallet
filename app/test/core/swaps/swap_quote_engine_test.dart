import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/swap_models.dart';
import 'package:pirate_wallet/core/swaps/swap_quote_engine.dart';
import 'package:test/test.dart';

void main() {
  const engine = SwapQuoteEngine();

  SwapOrderbookLevel ask(String price, String amount) => SwapOrderbookLevel(
    priceLtcPerArrr: Decimal.parse(price),
    arrrAmount: Decimal.parse(amount),
  );

  SwapOrderbookLevel bid(String price, String amount) => SwapOrderbookLevel(
    priceLtcPerArrr: Decimal.parse(price),
    arrrAmount: Decimal.parse(amount),
  );

  test('buy quote fully fills from market asks', () {
    final plan = engine.planBuyArrrWithLtc(
      ltcAmount: Decimal.parse('1.0'),
      asks: [ask('0.01', '50'), ask('0.02', '50')],
      slippageCap: Decimal.one,
    );

    expect(plan.hasMarketFill, isTrue);
    expect(plan.hasLimitRemainder, isFalse);
    expect(plan.marketArrrAmount, Decimal.parse('75'));
    expect(plan.marketLtcAmount, Decimal.parse('1.0'));
    expect(plan.limitPriceLtcPerArrr, isNull);
    expect(plan.fills, hasLength(2));
  });

  test('buy quote places unfilled LTC as a 3 percent premium limit order', () {
    final plan = engine.planBuyArrrWithLtc(
      ltcAmount: Decimal.parse('2.0'),
      asks: [ask('0.01', '50')],
    );

    expect(plan.marketArrrAmount, Decimal.parse('50'));
    expect(plan.marketLtcAmount, Decimal.parse('0.50'));
    expect(plan.remainderLtcAmount, Decimal.parse('1.50'));
    expect(plan.hasLimitRemainder, isTrue);
    expect(plan.limitPriceLtcPerArrr, Decimal.parse('0.0103'));
  });

  test('buy quote rejects empty orderbooks', () {
    expect(
      () => engine.planBuyArrrWithLtc(ltcAmount: Decimal.one, asks: const []),
      throwsA(isA<SwapQuoteUnavailableException>()),
    );
  });

  test('buy quote respects slippage cap', () {
    final plan = engine.planBuyArrrWithLtc(
      ltcAmount: Decimal.one,
      asks: [ask('0.01', '10'), ask('0.012', '10')],
      slippageCap: Decimal.parse('0.05'),
    );

    expect(plan.fills, hasLength(1));
    expect(plan.marketArrrAmount, Decimal.parse('10'));
    expect(plan.marketLtcAmount, Decimal.parse('0.10'));
    expect(plan.remainderLtcAmount, Decimal.parse('0.90'));
  });

  test('sell quote fully fills from market bids', () {
    final plan = engine.planSellArrrForLtc(
      arrrAmount: Decimal.parse('60'),
      bids: [bid('0.02', '50'), bid('0.019', '50')],
      slippageCap: Decimal.one,
    );

    expect(plan.hasMarketFill, isTrue);
    expect(plan.hasLimitRemainder, isFalse);
    expect(plan.marketArrrAmount, Decimal.parse('60'));
    expect(plan.marketLtcAmount, Decimal.parse('1.19'));
    expect(plan.fills, hasLength(2));
  });

  test(
    'sell quote places unfilled ARRR as a 3 percent discount limit order',
    () {
      final plan = engine.planSellArrrForLtc(
        arrrAmount: Decimal.parse('75'),
        bids: [bid('0.02', '50')],
      );

      expect(plan.marketArrrAmount, Decimal.parse('50'));
      expect(plan.marketLtcAmount, Decimal.parse('1.00'));
      expect(plan.remainderArrrAmount, Decimal.parse('25'));
      expect(plan.hasLimitRemainder, isTrue);
      expect(plan.limitPriceLtcPerArrr, Decimal.parse('0.0194'));
    },
  );
}
