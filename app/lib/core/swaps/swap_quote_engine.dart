import 'package:decimal/decimal.dart';

import 'swap_models.dart';

class SwapQuoteUnavailableException implements Exception {
  const SwapQuoteUnavailableException(this.message);

  final String message;

  @override
  String toString() => message;
}

class SwapQuoteEngine {
  const SwapQuoteEngine();

  static final Decimal defaultSlippageCap = Decimal.parse('0.03');
  static final Decimal defaultLimitPremium = Decimal.parse('0.03');

  SwapPlan planBuyArrrWithLtc({
    required Decimal ltcAmount,
    required List<SwapOrderbookLevel> asks,
    Decimal? slippageCap,
    Decimal? limitPremium,
  }) {
    final effectiveSlippageCap = slippageCap ?? defaultSlippageCap;
    final effectiveLimitPremium = limitPremium ?? defaultLimitPremium;

    if (ltcAmount.compareTo(Decimal.zero) <= 0) {
      throw const SwapQuoteUnavailableException('LTC amount must be positive.');
    }

    final sortedAsks =
        asks
            .where(
              (level) =>
                  level.priceLtcPerArrr.compareTo(Decimal.zero) > 0 &&
                  level.arrrAmount.compareTo(Decimal.zero) > 0,
            )
            .toList()
          ..sort((a, b) => a.priceLtcPerArrr.compareTo(b.priceLtcPerArrr));

    if (sortedAsks.isEmpty) {
      throw const SwapQuoteUnavailableException('ARRR/LTC orderbook is empty.');
    }

    final referencePrice = sortedAsks.first.priceLtcPerArrr;
    final maxMarketPrice =
        referencePrice * (Decimal.one + effectiveSlippageCap);
    var remainingLtc = ltcAmount;
    var marketLtc = Decimal.zero;
    var marketArrr = Decimal.zero;
    final fills = <SwapMarketFill>[];

    for (final ask in sortedAsks) {
      if (ask.priceLtcPerArrr.compareTo(maxMarketPrice) > 0) break;
      if (remainingLtc.compareTo(Decimal.zero) <= 0) break;

      final levelCost = ask.ltcAmount;
      final takeLtc = _minDecimal(remainingLtc, levelCost);
      final takeArrr = takeLtc == levelCost
          ? ask.arrrAmount
          : _divide(takeLtc, ask.priceLtcPerArrr, scale: 8);

      fills.add(
        SwapMarketFill(
          priceLtcPerArrr: ask.priceLtcPerArrr,
          arrrAmount: takeArrr,
          ltcAmount: takeLtc,
          orderId: ask.orderId,
        ),
      );
      remainingLtc -= takeLtc;
      marketLtc += takeLtc;
      marketArrr += takeArrr;
    }

    final averagePrice = marketArrr.compareTo(Decimal.zero) > 0
        ? _divide(marketLtc, marketArrr, scale: 12)
        : referencePrice;
    final realizedSlippage = referencePrice.compareTo(Decimal.zero) > 0
        ? _divide(averagePrice - referencePrice, referencePrice, scale: 8)
        : Decimal.zero;

    return SwapPlan(
      side: SwapSide.buyArrr,
      referencePriceLtcPerArrr: referencePrice,
      marketArrrAmount: marketArrr,
      marketLtcAmount: marketLtc,
      remainderLtcAmount: remainingLtc,
      remainderArrrAmount: Decimal.zero,
      limitPriceLtcPerArrr: remainingLtc.compareTo(Decimal.zero) > 0
          ? referencePrice * (Decimal.one + effectiveLimitPremium)
          : null,
      slippageCap: effectiveSlippageCap,
      realizedSlippage: realizedSlippage,
      fills: fills,
    );
  }

  SwapPlan planSellArrrForLtc({
    required Decimal arrrAmount,
    required List<SwapOrderbookLevel> bids,
    Decimal? slippageCap,
    Decimal? limitPremium,
  }) {
    final effectiveSlippageCap = slippageCap ?? defaultSlippageCap;
    final effectiveLimitPremium = limitPremium ?? defaultLimitPremium;

    if (arrrAmount.compareTo(Decimal.zero) <= 0) {
      throw const SwapQuoteUnavailableException(
        'ARRR amount must be positive.',
      );
    }

    final sortedBids =
        bids
            .where(
              (level) =>
                  level.priceLtcPerArrr.compareTo(Decimal.zero) > 0 &&
                  level.arrrAmount.compareTo(Decimal.zero) > 0,
            )
            .toList()
          ..sort((a, b) => b.priceLtcPerArrr.compareTo(a.priceLtcPerArrr));

    if (sortedBids.isEmpty) {
      throw const SwapQuoteUnavailableException('ARRR/LTC orderbook is empty.');
    }

    final referencePrice = sortedBids.first.priceLtcPerArrr;
    final minMarketPrice =
        referencePrice * (Decimal.one - effectiveSlippageCap);
    var remainingArrr = arrrAmount;
    var marketLtc = Decimal.zero;
    var marketArrr = Decimal.zero;
    final fills = <SwapMarketFill>[];

    for (final bid in sortedBids) {
      if (bid.priceLtcPerArrr.compareTo(minMarketPrice) < 0) break;
      if (remainingArrr.compareTo(Decimal.zero) <= 0) break;

      final takeArrr = _minDecimal(remainingArrr, bid.arrrAmount);
      final takeLtc = takeArrr * bid.priceLtcPerArrr;
      fills.add(
        SwapMarketFill(
          priceLtcPerArrr: bid.priceLtcPerArrr,
          arrrAmount: takeArrr,
          ltcAmount: takeLtc,
          orderId: bid.orderId,
        ),
      );
      remainingArrr -= takeArrr;
      marketArrr += takeArrr;
      marketLtc += takeLtc;
    }

    final averagePrice = marketArrr.compareTo(Decimal.zero) > 0
        ? _divide(marketLtc, marketArrr, scale: 12)
        : referencePrice;
    final realizedSlippage = referencePrice.compareTo(Decimal.zero) > 0
        ? _divide(referencePrice - averagePrice, referencePrice, scale: 8)
        : Decimal.zero;

    return SwapPlan(
      side: SwapSide.sellArrr,
      referencePriceLtcPerArrr: referencePrice,
      marketArrrAmount: marketArrr,
      marketLtcAmount: marketLtc,
      remainderLtcAmount: Decimal.zero,
      remainderArrrAmount: remainingArrr,
      limitPriceLtcPerArrr: remainingArrr.compareTo(Decimal.zero) > 0
          ? referencePrice * (Decimal.one - effectiveLimitPremium)
          : null,
      slippageCap: effectiveSlippageCap,
      realizedSlippage: realizedSlippage,
      fills: fills,
    );
  }

  Decimal _minDecimal(Decimal a, Decimal b) => a.compareTo(b) <= 0 ? a : b;

  Decimal _divide(Decimal a, Decimal b, {required int scale}) {
    return (a / b).toDecimal(scaleOnInfinitePrecision: scale);
  }
}
