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
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    final asks = await _engine.loadArrrLtcAsks();
    final plan = _quoteEngine.planBuyArrrWithLtc(
      ltcAmount: ltcAmount,
      asks: asks,
      slippageCap: slippageCap,
    );
    final preimage = plan.hasMarketFill
        ? await _engine.tradePreimageForBuy(plan)
        : const <String, dynamic>{};

    return SwapQuote(
      walletId: walletId,
      side: SwapSide.buyArrr,
      baseAsset: SwapAsset.arrr,
      relAsset: SwapAsset.ltc,
      requestedLtcAmount: ltcAmount,
      plan: plan,
      fees: _feesFromPreimage(preimage),
      createdAt: DateTime.now().toUtc(),
      preimage: preimage,
    );
  }

  Future<SwapQuote> quoteSellArrr({
    required String walletId,
    required Decimal arrrAmount,
    Decimal? slippageCap,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    final bids = await _engine.loadArrrLtcBids();
    final plan = _quoteEngine.planSellArrrForLtc(
      arrrAmount: arrrAmount,
      bids: bids,
      slippageCap: slippageCap,
    );

    return SwapQuote(
      walletId: walletId,
      side: SwapSide.sellArrr,
      baseAsset: SwapAsset.arrr,
      relAsset: SwapAsset.ltc,
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
  }) async {
    final quote = await quoteBuyArrr(
      walletId: walletId,
      ltcAmount: ltcAmount,
      slippageCap: slippageCap,
    );
    final depositAddress = await _engine.getLtcDepositAddress();
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
      ltcDepositAddress: depositAddress,
      arrReceivingAddress: receiveAddress,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  Future<SwapIntent> executeBuyMarket(SwapIntent intent) async {
    _assertWalletSupportsIntent(intent, SwapSide.buyArrr);
    await _engine.ensureStarted(intent.walletId);
    await _engine.activateArrrLtc();

    final swapUuid = await _engine.startMarketBuy(intent.plan);
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
    await _engine.activateArrrLtc();

    final orderUuid = await _engine.placeRemainderBuyLimit(intent.plan);
    final updated = intent.copyWith(
      status: SwapIntentStatus.limitOrderPlaced,
      limitOrderUuid: orderUuid,
    );
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<SwapIntent> startMarketSellArrr({
    required String walletId,
    required Decimal arrrAmount,
    required String destinationLtcAddress,
    Decimal? slippageCap,
  }) async {
    final quote = await quoteSellArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      slippageCap: slippageCap,
    );
    final swapUuid = await _engine.startMarketSell(
      arrrAmount: quote.plan.marketArrrAmount,
      expectedLtcAmount: quote.plan.marketLtcAmount,
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
      destinationLtcAddress: destinationLtcAddress,
      marketSwapUuid: swapUuid,
    );
    await _intentStore.upsert(intent);
    return intent;
  }

  Future<SwapIntent> placeSellLimitArrr({
    required String walletId,
    required Decimal arrrAmount,
    required Decimal priceLtcPerArrr,
    required String destinationLtcAddress,
  }) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    final orderUuid = await _engine.placeSellLimit(
      arrrAmount: arrrAmount,
      priceLtcPerArrr: priceLtcPerArrr,
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
    );
    final intent = SwapIntent(
      id: _uuid.v4(),
      walletId: walletId,
      side: SwapSide.sellArrr,
      status: SwapIntentStatus.limitOrderPlaced,
      createdAt: now,
      updatedAt: now,
      plan: plan,
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
      ),
      SwapSide.sellArrr => await _engine.modifySellLimitOrder(
        existingUuid: uuid,
        newPriceLtcPerArrr: newPriceLtcPerArrr,
        arrrVolume: intent.plan.remainderArrrAmount,
      ),
    };
    final updated = intent.copyWith(limitOrderUuid: newUuid);
    await _intentStore.upsert(updated);
    return updated;
  }

  Future<List<SwapIntent>> resumeIncompleteIntents(String walletId) async {
    final intents = await _intentStore.load(walletId);
    return intents
        .where(
          (intent) =>
              intent.status != SwapIntentStatus.completed &&
              intent.status != SwapIntentStatus.cancelled &&
              intent.status != SwapIntentStatus.failed,
        )
        .toList();
  }

  Future<Decimal> currentLtcDepositBalance(String walletId) async {
    await _engine.ensureStarted(walletId);
    await _engine.activateArrrLtc();
    return _engine.ltcBalance();
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

  SwapFeeBreakdown _feesFromPreimage(Map<String, dynamic> preimage) {
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
      if (coin == 'ARRR') {
        baseNetworkFee += amount;
      } else if (coin == 'LTC') {
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
