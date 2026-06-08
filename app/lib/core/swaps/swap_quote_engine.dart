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
    Decimal? appFeeRate,
  }) {
    return planBuyArrr(
      relAmount: ltcAmount,
      asks: asks,
      pair: SwapPair.arrrLtc,
      slippageCap: slippageCap,
      limitPremium: limitPremium,
      appFeeRate: appFeeRate,
    );
  }

  SwapPlan planBuyArrr({
    required Decimal relAmount,
    required List<SwapOrderbookLevel> asks,
    required SwapPair pair,
    Decimal? slippageCap,
    Decimal? limitPremium,
    Decimal? appFeeRate,
  }) {
    final effectiveSlippageCap = slippageCap ?? defaultSlippageCap;
    final effectiveLimitPremium = limitPremium ?? defaultLimitPremium;

    if (relAmount.compareTo(Decimal.zero) <= 0) {
      throw SwapQuoteUnavailableException(
        '${pair.relTicker} amount must be positive.',
      );
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
      throw SwapQuoteUnavailableException(
        '${pair.displayName} orderbook is empty.',
      );
    }

    final referencePrice = sortedAsks.first.priceLtcPerArrr;
    final maxMarketPrice =
        referencePrice * (Decimal.one + effectiveSlippageCap);
    var remainingLtc = relAmount;
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

    final appFeeArrr = marketArrr.compareTo(Decimal.zero) > 0
        ? _scale(marketArrr * (appFeeRate ?? appTakerFeeRate), scale: 8)
        : Decimal.zero;

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
      appFeeArrrAmount: appFeeArrr,
    );
  }

  SwapPlan planSellArrrForLtc({
    required Decimal arrrAmount,
    required List<SwapOrderbookLevel> bids,
    Decimal? slippageCap,
    Decimal? limitPremium,
    Decimal? appFeeRate,
  }) {
    return planSellArrr(
      arrrAmount: arrrAmount,
      bids: bids,
      pair: SwapPair.arrrLtc,
      slippageCap: slippageCap,
      limitPremium: limitPremium,
      appFeeRate: appFeeRate,
    );
  }

  SwapPlan planSellArrr({
    required Decimal arrrAmount,
    required List<SwapOrderbookLevel> bids,
    required SwapPair pair,
    Decimal? slippageCap,
    Decimal? limitPremium,
    Decimal? appFeeRate,
  }) {
    final effectiveSlippageCap = slippageCap ?? defaultSlippageCap;
    final effectiveLimitPremium = limitPremium ?? defaultLimitPremium;
    final effectiveAppFeeRate = appFeeRate ?? appTakerFeeRate;

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
      throw SwapQuoteUnavailableException(
        '${pair.displayName} orderbook is empty.',
      );
    }

    final referencePrice = sortedBids.first.priceLtcPerArrr;
    final minMarketPrice =
        referencePrice * (Decimal.one - effectiveSlippageCap);
    final feeMultiplier = Decimal.one - totalTakerFeeRate;
    if (feeMultiplier <= Decimal.zero) {
      throw const SwapQuoteUnavailableException('Invalid ARRR swap fee.');
    }

    var remainingGrossArrr = arrrAmount;
    var marketGrossArrr = Decimal.zero;
    var marketLtc = Decimal.zero;
    var marketArrr = Decimal.zero;
    final fills = <SwapMarketFill>[];

    for (final bid in sortedBids) {
      if (bid.priceLtcPerArrr.compareTo(minMarketPrice) < 0) break;
      if (remainingGrossArrr.compareTo(Decimal.zero) <= 0) break;

      final grossNeededForLevel = _divide(
        bid.arrrAmount,
        feeMultiplier,
        scale: 12,
      );
      final takeGrossArrr = _minDecimal(
        remainingGrossArrr,
        grossNeededForLevel,
      );
      final takeArrr = takeGrossArrr == grossNeededForLevel
          ? bid.arrrAmount
          : _scale(takeGrossArrr * feeMultiplier, scale: 8);
      final takeLtc = takeArrr * bid.priceLtcPerArrr;
      fills.add(
        SwapMarketFill(
          priceLtcPerArrr: bid.priceLtcPerArrr,
          arrrAmount: takeArrr,
          ltcAmount: takeLtc,
          orderId: bid.orderId,
        ),
      );
      remainingGrossArrr -= takeGrossArrr;
      marketGrossArrr += takeGrossArrr;
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
      remainderArrrAmount: _scale(remainingGrossArrr, scale: 8),
      limitPriceLtcPerArrr: remainingGrossArrr.compareTo(Decimal.zero) > 0
          ? referencePrice * (Decimal.one - effectiveLimitPremium)
          : null,
      slippageCap: effectiveSlippageCap,
      realizedSlippage: realizedSlippage,
      fills: fills,
      appFeeArrrAmount: marketGrossArrr.compareTo(Decimal.zero) > 0
          ? _scale(marketGrossArrr * effectiveAppFeeRate, scale: 8)
          : Decimal.zero,
    );
  }

  Decimal _minDecimal(Decimal a, Decimal b) => a.compareTo(b) <= 0 ? a : b;

  Decimal _divide(Decimal a, Decimal b, {required int scale}) {
    return (a / b).toDecimal(scaleOnInfinitePrecision: scale);
  }

  Decimal _scale(Decimal value, {required int scale}) {
    return Decimal.parse(value.toStringAsFixed(scale));
  }
}
