import 'package:decimal/decimal.dart';
import 'package:uuid/uuid.dart';

import '../ffi/ffi_bridge.dart';
import 'kdf_swap_engine.dart';
import 'swap_intent_store.dart';
import 'swap_models.dart';
import 'swap_quote_engine.dart';

class AtomicSwapService {
  AtomicSwapService({
    required KdfSwapEngine engine,
    required SwapIntentStore intentStore,
    SwapQuoteEngine quoteEngine = const SwapQuoteEngine(),
    Uuid uuid = const Uuid(),
  }) : _engine = engine,
       _intentStore = intentStore,
       _quoteEngine = quoteEngine,
       _uuid = uuid;

  final KdfSwapEngine _engine;
  final SwapIntentStore _intentStore;
  final SwapQuoteEngine _quoteEngine;
  final Uuid _uuid;

  Future<SwapQuote> quoteBuyArrr({
    required String walletId,
    required Decimal ltcAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    final asks = await _engine.loadAsks(pair);
    final plan = _quoteEngine.planBuyArrr(
      relAmount: ltcAmount,
      asks: asks,
      pair: pair,
      slippageCap: slippageCap,
    );
    final preimage = plan.hasMarketFill
        ? await _tradePreimageForBuyQuote(plan, pair)
        : const <String, dynamic>{};

    return SwapQuote(
      walletId: walletId,
      side: SwapSide.buyArrr,
      pair: pair,
      baseAsset: pair.baseAsset,
      relAsset: pair.relAsset,
      requestedLtcAmount: ltcAmount,
      plan: plan,
      fees: _feesFromPreimage(preimage, pair: pair),
      createdAt: DateTime.now().toUtc(),
      preimage: preimage,
    );
  }

  Future<SwapQuote> quoteSellArrr({
    required String walletId,
    required Decimal arrrAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    final bids = await _engine.loadBids(pair);
    final plan = _quoteEngine.planSellArrr(
      arrrAmount: arrrAmount,
      bids: bids,
      pair: pair,
      slippageCap: slippageCap,
    );

    return SwapQuote(
      walletId: walletId,
      side: SwapSide.sellArrr,
      pair: pair,
      baseAsset: pair.baseAsset,
      relAsset: pair.relAsset,
      requestedLtcAmount: plan.marketLtcAmount,
      plan: plan,
      fees: SwapFeeBreakdown(),
      createdAt: DateTime.now().toUtc(),
    );
  }

  Future<SwapIntent> prepareBuyArrr({
    required String walletId,
    required Decimal ltcAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    final quote = await quoteBuyArrr(
      walletId: walletId,
      ltcAmount: ltcAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
    final depositAddress = await _engine.getDepositAddress(pair.relAsset);
    final receiveAddress = await FfiBridge.currentReceiveAddress(walletId);
    final now = DateTime.now().toUtc();
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.buyArrr,
      status: SwapIntentStatus.waitingForDeposit,
      createdAt: now,
      updatedAt: now,
      plan: quote.plan,
      pair: pair,
      ltcDepositAddress: depositAddress,
      arrReceivingAddress: receiveAddress,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  Future<SwapIntent> executeBuyMarket(SwapIntent intent) async {
    _assertWalletSupportsIntent(intent, SwapSide.buyArrr);
    await _engine.ensureStarted(intent.walletId);
    await _engine.activatePair(intent.pair);

    final swapUuid = await _engine.startMarketBuyPair(
      intent.plan,
      pair: intent.pair,
    );
    final updated = intent.copyWith(
      status: SwapIntentStatus.marketSwapStarted,
      marketSwapUuid: swapUuid,
    );
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> placeBuyRemainderLimit(SwapIntent intent) async {
    _assertWalletSupportsIntent(intent, SwapSide.buyArrr);
    await _engine.ensureStarted(intent.walletId);
    await _engine.activatePair(intent.pair);

    final orderUuid = await _engine.placeRemainderBuyLimitPair(
      intent.plan,
      pair: intent.pair,
    );
    final updated = intent.copyWith(
      status: SwapIntentStatus.limitOrderPlaced,
      limitOrderUuid: orderUuid,
    );
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> executeSellMarket(SwapIntent intent) async {
    _assertWalletSupportsIntent(intent, SwapSide.sellArrr);
    await _engine.ensureStarted(intent.walletId);
    await _engine.activatePair(intent.pair);

    final swapUuid = await _engine.startMarketSell(
      arrrAmount: intent.plan.marketArrrAmount,
      expectedLtcAmount: intent.plan.marketLtcAmount,
      pair: intent.pair,
    );
    final updated = intent.copyWith(
      status: SwapIntentStatus.marketSwapStarted,
      marketSwapUuid: swapUuid,
    );
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> startMarketSellArrr({
    required String walletId,
    required Decimal arrrAmount,
    required String destinationLtcAddress,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    final quote = await quoteSellArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
    final swapUuid = await _engine.startMarketSell(
      arrrAmount: quote.plan.marketArrrAmount,
      expectedLtcAmount: quote.plan.marketLtcAmount,
      pair: pair,
    );
    final now = DateTime.now().toUtc();
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.sellArrr,
      status: SwapIntentStatus.marketSwapStarted,
      createdAt: now,
      updatedAt: now,
      plan: quote.plan,
      pair: pair,
      destinationLtcAddress: destinationLtcAddress,
      marketSwapUuid: swapUuid,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  /// Prepares a *limit* buy: the user will deposit LTC and we then place a
  /// maker order on the book at their chosen price (rather than taking the
  /// current market). The returned intent is [SwapIntentStatus.waitingForDeposit]
  /// and carries a deposit address, exactly like a market buy.
  Future<SwapIntent> prepareBuyLimitArrr({
    required String walletId,
    required Decimal ltcAmount,
    required Decimal priceLtcPerArrr,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    final depositAddress = await _engine.getDepositAddress(pair.relAsset);
    final receiveAddress = await FfiBridge.currentReceiveAddress(walletId);
    final now = DateTime.now().toUtc();
    final expectedArrr = priceLtcPerArrr > Decimal.zero
        ? (ltcAmount / priceLtcPerArrr).toDecimal(scaleOnInfinitePrecision: 8)
        : Decimal.zero;
    final plan = SwapPlan(
      side: SwapSide.buyArrr,
      referencePriceLtcPerArrr: priceLtcPerArrr,
      marketArrrAmount: Decimal.zero,
      marketLtcAmount: Decimal.zero,
      remainderLtcAmount: ltcAmount,
      remainderArrrAmount: expectedArrr,
      limitPriceLtcPerArrr: priceLtcPerArrr,
      slippageCap: Decimal.zero,
      realizedSlippage: Decimal.zero,
      fills: const [],
      appFeeArrrAmount: Decimal.zero,
    );
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.buyArrr,
      status: SwapIntentStatus.waitingForDeposit,
      createdAt: now,
      updatedAt: now,
      plan: plan,
      pair: pair,
      ltcDepositAddress: depositAddress,
      arrReceivingAddress: receiveAddress,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  /// All persisted intents for a wallet (completed/cancelled/failed included),
  /// most recent first. Used to render swap history.
  Future<List<SwapIntent>> loadIntentHistory(String walletId) async {
    final intents = await _intentStore.load(walletId);
    intents.sort((a, b) => b.createdAt.compareTo(a.createdAt));
    return intents;
  }

  Future<SwapIntent> placeSellLimitArrr({
    required String walletId,
    required Decimal arrrAmount,
    required Decimal priceLtcPerArrr,
    required String destinationLtcAddress,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    final orderUuid = await _engine.placeSellLimit(
      arrrAmount: arrrAmount,
      priceLtcPerArrr: priceLtcPerArrr,
      pair: pair,
    );
    final now = DateTime.now().toUtc();
    final plan = SwapPlan(
      side: SwapSide.sellArrr,
      referencePriceLtcPerArrr: priceLtcPerArrr,
      marketArrrAmount: Decimal.zero,
      marketLtcAmount: Decimal.zero,
      remainderLtcAmount: Decimal.zero,
      remainderArrrAmount: arrrAmount,
      limitPriceLtcPerArrr: priceLtcPerArrr,
      slippageCap: Decimal.zero,
      realizedSlippage: Decimal.zero,
      fills: const [],
      appFeeArrrAmount: Decimal.zero,
    );
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.sellArrr,
      status: SwapIntentStatus.limitOrderPlaced,
      createdAt: now,
      updatedAt: now,
      plan: plan,
      pair: pair,
      destinationLtcAddress: destinationLtcAddress,
      limitOrderUuid: orderUuid,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  Future<SwapIntent> cancelLimitOrder(SwapIntent intent) async {
    final uuid = intent.limitOrderUuid;
    if (uuid == null || uuid.isEmpty) return intent;
    await _engine.ensureStarted(intent.walletId);
    await _engine.cancelOrder(uuid);
    final updated = intent.copyWith(status: SwapIntentStatus.cancelled);
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> cancelIntent(SwapIntent intent) async {
    if (intent.status == SwapIntentStatus.limitOrderPlaced &&
        (intent.limitOrderUuid?.isNotEmpty ?? false)) {
      return cancelLimitOrder(intent);
    }

    final updated = intent.copyWith(status: SwapIntentStatus.cancelled);
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> modifyBuyLimitOrder({
    required SwapIntent intent,
    required Decimal newPriceLtcPerArrr,
  }) async {
    return modifyLimitOrder(
      intent: intent,
      newPriceLtcPerArrr: newPriceLtcPerArrr,
    );
  }

  Future<SwapIntent> modifyLimitOrder({
    required SwapIntent intent,
    required Decimal newPriceLtcPerArrr,
  }) async {
    final uuid = intent.limitOrderUuid;
    if (uuid == null || uuid.isEmpty) {
      throw const KdfSwapEngineException(
        'Cannot modify an intent without a limit order.',
      );
    }
    await _engine.ensureStarted(intent.walletId);
    final newUuid = switch (intent.side) {
      SwapSide.buyArrr => await _engine.modifyBuyLimitOrder(
        existingUuid: uuid,
        newPriceLtcPerArrr: newPriceLtcPerArrr,
        ltcVolume: intent.plan.remainderLtcAmount,
        pair: intent.pair,
      ),
      SwapSide.sellArrr => await _engine.modifySellLimitOrder(
        existingUuid: uuid,
        newPriceLtcPerArrr: newPriceLtcPerArrr,
        arrrVolume: intent.plan.remainderArrrAmount,
        pair: intent.pair,
      ),
    };
    final updated = intent.copyWith(limitOrderUuid: newUuid);
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<List<SwapIntent>> resumeIncompleteIntents(String walletId) async {
    final intents = await _intentStore.load(walletId);
    final now = DateTime.now().toUtc();
    final active = <SwapIntent>[];

    for (final intent in intents) {
      if (intent.status == SwapIntentStatus.completed ||
          intent.status == SwapIntentStatus.cancelled ||
          intent.status == SwapIntentStatus.failed) {
        continue;
      }

      if (intent.isDepositExpired(now)) {
        await _intentStore.upsert(
          intent.copyWith(
            status: SwapIntentStatus.failed,
            lastError:
                'Deposit window expired before ${intent.pair.relTicker} was detected.',
          ),
        );
        continue;
      }

      active.add(intent);
    }

    return active;
  }

  Future<Decimal> currentLtcDepositBalance(String walletId) async {
    return currentRelDepositBalance(walletId);
  }

  Future<Decimal> currentRelDepositBalance(
    String walletId, {
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    return _engine.relBalance(pair);
  }

  Future<Decimal> currentFundingBalance(
    String walletId, {
    required SwapAsset asset,
  }) async {
    await _engine.ensureStarted(walletId);
    await _activateAssetSupport(asset);
    return _engine.coinBalance(asset);
  }

  Future<Decimal> currentArrrBalance(String walletId) async {
    return currentFundingBalance(walletId, asset: SwapAsset.arrr);
  }

  Future<String> getFundingDepositAddress(
    String walletId, {
    required SwapAsset asset,
  }) async {
    await _engine.ensureStarted(walletId);
    await _activateAssetSupport(asset);
    return _engine.getDepositAddress(asset);
  }

  Future<String> getArrrDepositAddress(String walletId) async {
    return getFundingDepositAddress(walletId, asset: SwapAsset.arrr);
  }

  Future<SwapIntent> prepareSellArrr({
    required String walletId,
    required Decimal arrrAmount,
    required String destinationLtcAddress,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    final quote = await quoteSellArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
    final now = DateTime.now().toUtc();
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.sellArrr,
      status: SwapIntentStatus.prepared,
      createdAt: now,
      updatedAt: now,
      plan: quote.plan,
      pair: pair,
      destinationLtcAddress: destinationLtcAddress,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  Future<void> withdrawArrrToWallet({
    required String walletId,
    required String address,
    required Decimal amount,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    await _engine.withdrawArrrToWallet(address: address, amount: amount);
  }

  Future<void> withdrawAllArrrToWallet({
    required String walletId,
    required String address,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    await _engine.withdrawAllArrrToWallet(address: address);
  }

  Future<void> withdrawLtc({
    required String walletId,
    required String address,
    required Decimal amount,
  }) async {
    await withdrawRel(
      walletId: walletId,
      address: address,
      amount: amount,
      pair: SwapPair.arrrLtc,
    );
  }

  Future<void> withdrawRel({
    required String walletId,
    required String address,
    required Decimal amount,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    await _engine.withdrawAsset(
      asset: pair.relAsset,
      address: address,
      amount: amount,
    );
  }

  Future<void> withdrawFundingAsset({
    required String walletId,
    required SwapAsset asset,
    required String address,
    required Decimal amount,
  }) async {
    await _engine.ensureStarted(walletId);
    await _activateAssetSupport(asset);
    await _engine.withdrawAsset(asset: asset, address: address, amount: amount);
  }

  Future<void> withdrawAllLtc({
    required String walletId,
    required String address,
  }) async {
    await withdrawAllRel(
      walletId: walletId,
      address: address,
      pair: SwapPair.arrrLtc,
    );
  }

  Future<void> withdrawAllRel({
    required String walletId,
    required String address,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    await _engine.withdrawAllAsset(asset: pair.relAsset, address: address);
  }

  Future<void> _activateAssetSupport(SwapAsset asset) async {
    final pair = switch (asset) {
      SwapAsset.ltc => SwapPair.arrrLtc,
      SwapAsset.varrr => SwapPair.arrrVarrr,
      SwapAsset.arrr => SwapPair.arrrLtc,
    };
    await _engine.activatePair(pair);
  }

  Future<({List<SwapOrderbookLevel> asks, List<SwapOrderbookLevel> bids})>
  loadOrderbook(String walletId, {SwapPair pair = SwapPair.arrrLtc}) async {
    await _engine.ensureStarted(walletId);
    await _engine.activatePair(pair);
    final book = await _engine.loadOrderbook(pair);
    return (asks: book.asks, bids: book.bids);
  }

  Future<SwapIntent> markIntentCompleted(SwapIntent intent) async {
    final updated = intent.copyWith(status: SwapIntentStatus.completed);
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> markIntentFailed(SwapIntent intent, String error) async {
    final updated = intent.copyWith(
      status: SwapIntentStatus.failed,
      lastError: error,
    );
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<Map<String, dynamic>> swapStatus(String walletId, String uuid) async {
    await _engine.ensureStarted(walletId);
    return _engine.swapStatus(uuid);
  }

  Future<Map<String, dynamic>> activeOrders(String walletId) async {
    await _engine.ensureStarted(walletId);
    return _engine.myOrders();
  }

  Future<void> shutdown() => _engine.dispose();

  Future<Map<String, dynamic>> _tradePreimageForBuyQuote(
    SwapPlan plan,
    SwapPair pair,
  ) async {
    try {
      return await _engine.tradePreimageForBuyPair(plan, pair: pair);
    } on KdfSwapEngineException catch (error) {
      if (isKdfInsufficientBalanceError(error, coin: pair.relTicker)) {
        return const <String, dynamic>{};
      }
      rethrow;
    }
  }

  SwapFeeBreakdown _feesFromPreimage(
    Map<String, dynamic> preimage, {
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    final result = Map<String, dynamic>.from(
      preimage['result'] as Map? ?? const {},
    );
    final totalFees = result['total_fees'] as List? ?? const [];
    var baseNetworkFee = Decimal.zero;
    var relNetworkFee = Decimal.zero;
    var kdfFee = Decimal.zero;

    for (final entry in totalFees.whereType<Map<Object?, Object?>>()) {
      final json = Map<String, dynamic>.from(entry);
      final coin = json['coin']?.toString().toUpperCase();
      final amount = _decimal(
        json['amount'] ?? json['fee'] ?? json['required_amount'],
      );
      if (coin == pair.baseTicker.toUpperCase()) {
        baseNetworkFee += amount;
      } else if (coin == pair.relTicker.toUpperCase()) {
        relNetworkFee += amount;
      } else {
        kdfFee += amount;
      }
    }

    return SwapFeeBreakdown(
      kdfFee: kdfFee,
      baseNetworkFee: baseNetworkFee,
      relNetworkFee: relNetworkFee,
      raw: preimage,
    );
  }

  Decimal _decimal(Object? value) {
    if (value == null) return Decimal.zero;
    if (value is Decimal) return value;
    if (value is num || value is String) return Decimal.parse(value.toString());
    if (value is Map && value['decimal'] != null) {
      return Decimal.parse(value['decimal'].toString());
    }
    return Decimal.zero;
  }

  void _assertWalletSupportsIntent(SwapIntent intent, SwapSide side) {
    if (intent.side != side) {
      throw KdfSwapEngineException(
        'Swap intent ${intent.id} has the wrong side.',
      );
    }
  }
}
