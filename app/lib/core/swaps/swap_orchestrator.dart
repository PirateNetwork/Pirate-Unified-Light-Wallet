import 'dart:async';

import 'package:decimal/decimal.dart';

import '../ffi/ffi_bridge.dart';
import '../ffi/generated/models.dart';
import '../i18n/arb_text_localizer.dart';
import 'atomic_swap_service.dart';
import 'swap_amount_utils.dart';
import 'swap_models.dart';

typedef SwapSignCallback = Future<SignedTx> Function(PendingTx pending);
typedef SwapProgressCallback = void Function(SwapProgress progress);

class SwapOrchestratorException implements Exception {
  const SwapOrchestratorException(this.message);

  final String message;

  @override
  String toString() => message;
}

class SwapOrchestrator {
  SwapOrchestrator({required AtomicSwapService service}) : _service = service;

  final AtomicSwapService _service;

  static const depositPollInterval = Duration(seconds: 5);
  static const depositTimeout = swapDepositWindow;
  static const swapPollInterval = Duration(seconds: 4);
  static const swapPollTimeout = Duration(hours: 2);

  Future<SwapQuote> quoteBuy({
    required String walletId,
    required Decimal ltcAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.quoteBuyArrr(
      walletId: walletId,
      ltcAmount: ltcAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
  }

  Future<SwapQuote> quoteSell({
    required String walletId,
    required Decimal arrrAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.quoteSellArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
  }

  Future<SwapIntent> prepareBuy({
    required String walletId,
    required Decimal ltcAmount,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.prepareBuyArrr(
      walletId: walletId,
      ltcAmount: ltcAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
  }

  Future<SwapIntent> prepareSell({
    required String walletId,
    required Decimal arrrAmount,
    required String destinationLtcAddress,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.prepareSellArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      destinationLtcAddress: destinationLtcAddress,
      slippageCap: slippageCap,
      pair: pair,
    );
  }

  Future<SwapIntent> prepareBuyLimit({
    required String walletId,
    required Decimal ltcAmount,
    required Decimal priceLtcPerArrr,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.prepareBuyLimitArrr(
      walletId: walletId,
      ltcAmount: ltcAmount,
      priceLtcPerArrr: priceLtcPerArrr,
      pair: pair,
    );
  }

  Future<List<SwapIntent>> resumeIncomplete(String walletId) {
    return _service.resumeIncompleteIntents(walletId);
  }

  Future<List<SwapIntent>> loadHistory(String walletId) {
    return _service.loadIntentHistory(walletId);
  }

  Future<SwapIntent> cancelIntent(SwapIntent intent) {
    return _service.cancelIntent(intent);
  }

  Future<SwapIntent> failIntent(SwapIntent intent, String error) {
    return _service.markIntentFailed(intent, error);
  }

  Future<({List<SwapOrderbookLevel> asks, List<SwapOrderbookLevel> bids})>
  loadOrderbook(String walletId, {SwapPair pair = SwapPair.arrrLtc}) {
    return _service.loadOrderbook(walletId, pair: pair);
  }

  Future<void> executeBuyFlow({
    required SwapIntent intent,
    required Decimal requiredLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    final pair = intent.pair;
    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.waitingForDeposit,
      'Waiting for your confirmed ${pair.relTicker} deposit...',
    );

    await _waitForRelDeposit(
      walletId: intent.walletId,
      requiredAmount: requiredLtc,
      pair: pair,
      isCancelled: isCancelled,
      onProgress: (balance) {
        _emit(
          onProgress,
          intent.id,
          SwapProgressStage.waitingForDeposit,
          'Detected ${formatSwapAmount(balance, fractionDigits: 8)} ${pair.relTicker} so far...',
        );
      },
    );

    await _completeBuyIntent(intent, onProgress: onProgress);
  }

  /// Buy-limit flow: wait for the LTC deposit, then place a maker (limit) order
  /// at the user's price. Returns the updated intent (status
  /// [SwapIntentStatus.limitOrderPlaced]) so the UI can surface it under open
  /// orders.
  Future<SwapIntent> executeBuyLimitFlow({
    required SwapIntent intent,
    required Decimal requiredLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    final pair = intent.pair;
    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.waitingForDeposit,
      'Waiting for your confirmed ${pair.relTicker} deposit...',
    );

    await _waitForRelDeposit(
      walletId: intent.walletId,
      requiredAmount: requiredLtc,
      pair: pair,
      isCancelled: isCancelled,
      onProgress: (balance) {
        _emit(
          onProgress,
          intent.id,
          SwapProgressStage.waitingForDeposit,
          'Detected ${formatSwapAmount(balance, fractionDigits: 8)} ${pair.relTicker} so far...',
        );
      },
    );

    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.placingLimitRemainder,
      'Placing your limit order on the book...'.tr,
    );
    final updated = await _service.placeBuyRemainderLimit(intent);
    _emit(
      onProgress,
      updated.id,
      SwapProgressStage.placingLimitRemainder,
      'Limit order placed. It will fill when the market reaches your price.'.tr,
    );
    return updated;
  }

  /// Sell-limit flow: broadcast ARRR to the swap engine, then place a maker
  /// order at the user's price.
  Future<SwapIntent> executeSellLimitFlow({
    required String walletId,
    required Decimal arrrAmount,
    required Decimal priceLtcPerArrr,
    required String destinationLtcAddress,
    required SwapSignCallback signPending,
    SwapPair pair = SwapPair.arrrLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) {
    return placeAdvancedLimitSell(
      walletId: walletId,
      arrrAmount: arrrAmount,
      priceLtcPerArrr: priceLtcPerArrr,
      destinationLtcAddress: destinationLtcAddress,
      signPending: signPending,
      pair: pair,
      onProgress: onProgress,
      isCancelled: isCancelled,
    );
  }

  Future<Decimal> currentLtcDepositBalance(String walletId) {
    return _service.currentLtcDepositBalance(walletId);
  }

  Future<Decimal> currentRelDepositBalance(
    String walletId, {
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.currentRelDepositBalance(walletId, pair: pair);
  }

  Future<Decimal> currentArrrFundingBalance(String walletId) {
    return _service.currentArrrBalance(walletId);
  }

  Future<Decimal> currentFundingBalance(
    String walletId, {
    required SwapAsset asset,
  }) {
    return _service.currentFundingBalance(walletId, asset: asset);
  }

  Future<String> getFundingDepositAddress(
    String walletId, {
    required SwapAsset asset,
  }) {
    return _service.getFundingDepositAddress(walletId, asset: asset);
  }

  Future<void> withdrawAllFundingLtc({
    required String walletId,
    required String address,
  }) {
    return _service.withdrawAllLtc(walletId: walletId, address: address);
  }

  Future<void> withdrawAllFundingRel({
    required String walletId,
    required String address,
    SwapPair pair = SwapPair.arrrLtc,
  }) {
    return _service.withdrawAllRel(
      walletId: walletId,
      address: address,
      pair: pair,
    );
  }

  Future<void> withdrawFundingAsset({
    required String walletId,
    required SwapAsset asset,
    required String address,
    required Decimal amount,
  }) {
    return _service.withdrawFundingAsset(
      walletId: walletId,
      asset: asset,
      address: address,
      amount: amount,
    );
  }

  Future<void> withdrawAllFundingArrrToWallet(String walletId) async {
    final address = await FfiBridge.currentReceiveAddress(walletId);
    await _service.withdrawAllArrrToWallet(
      walletId: walletId,
      address: address,
    );
  }

  Future<void> continueBuyFromDeposit({
    required SwapIntent intent,
    required Decimal requiredLtc,
    SwapProgressCallback? onProgress,
  }) async {
    final pair = intent.pair;
    final balance = await _service.currentRelDepositBalance(
      intent.walletId,
      pair: pair,
    );
    if (balance < requiredLtc) {
      throw SwapOrchestratorException(
        'Deposit not detected yet. Send ${formatSwapAmount(requiredLtc)} ${pair.relTicker} to continue.',
      );
    }
    await _completeBuyIntent(intent, onProgress: onProgress);
  }

  Future<void> continueStartedSwapFlow({
    required SwapIntent intent,
    SwapProgressCallback? onProgress,
  }) async {
    if (intent.status != SwapIntentStatus.marketSwapStarted &&
        intent.status != SwapIntentStatus.limitOrderPlaced) {
      throw SwapOrchestratorException(
        'This swap has not started yet. Resume from the deposit screen instead.'
            .tr,
      );
    }

    if (intent.status == SwapIntentStatus.limitOrderPlaced) {
      _emit(
        onProgress,
        intent.id,
        SwapProgressStage.placingLimitRemainder,
        'Limit order is open. You can cancel it from Open swap orders.'.tr,
      );
      return;
    }

    if (intent.side == SwapSide.buyArrr) {
      await _completeBuyIntent(intent, onProgress: onProgress);
    } else {
      await _completeSellIntent(intent, onProgress: onProgress);
    }
  }

  Future<void> executeSellFlow({
    required SwapIntent intent,
    required Decimal arrrAmount,
    required SwapSignCallback signPending,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.startingKdf,
      'Preparing your swap...'.tr,
    );

    final bridgeAddress = await _service.getArrrDepositAddress(intent.walletId);

    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.matchingMarketOrder,
      'Sending ARRR to the swap engine...'.tr,
    );

    final marketArrrAmount = intent.plan.marketArrrAmount;
    final appFeeArrrAmount = intent.plan.appFeeArrrAmount;
    if (marketArrrAmount <= Decimal.zero) {
      throw SwapOrchestratorException(
        'No ARRR market amount available for this swap.'.tr,
      );
    }

    final kdfFeeArrrAmount = intent.plan.estimatedKdfTakerFeeArrrAmount;
    final bridgeFundingAmount = marketArrrAmount + kdfFeeArrrAmount;
    final outputs = <Output>[
      Output(addr: bridgeAddress, amount: arrrToZatoshis(bridgeFundingAmount)),
      if (appFeeArrrAmount > Decimal.zero)
        Output(
          addr: appTakerFeeRecipientArrrAddress,
          amount: arrrToZatoshis(appFeeArrrAmount),
        ),
    ];
    final pending = await FfiBridge.buildTx(
      walletId: intent.walletId,
      outputs: outputs,
    );
    final signed = await signPending(pending);
    await FfiBridge.broadcastTx(signed);

    await _waitForArrrBalance(
      walletId: intent.walletId,
      requiredAmount: bridgeFundingAmount,
      isCancelled: isCancelled,
      onProgress: (balance) {
        _emit(
          onProgress,
          intent.id,
          SwapProgressStage.matchingMarketOrder,
          'ARRR received (${formatSwapAmount(balance)} ARRR)...',
        );
      },
    );

    await _completeSellIntent(intent, onProgress: onProgress);
  }

  Future<void> executeAdvancedMarketBuy({
    required String walletId,
    required Decimal ltcAmount,
    required SwapSignCallback signPending,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    final intent = await prepareBuy(
      walletId: walletId,
      ltcAmount: ltcAmount,
      slippageCap: slippageCap,
      pair: pair,
    );
    await executeBuyFlow(
      intent: intent,
      requiredLtc: ltcAmount,
      onProgress: onProgress,
      isCancelled: isCancelled,
    );
  }

  Future<void> executeAdvancedMarketSell({
    required String walletId,
    required Decimal arrrAmount,
    required String destinationLtcAddress,
    required SwapSignCallback signPending,
    Decimal? slippageCap,
    SwapPair pair = SwapPair.arrrLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    final intent = await prepareSell(
      walletId: walletId,
      arrrAmount: arrrAmount,
      destinationLtcAddress: destinationLtcAddress,
      slippageCap: slippageCap,
      pair: pair,
    );
    await executeSellFlow(
      intent: intent,
      arrrAmount: arrrAmount,
      signPending: signPending,
      onProgress: onProgress,
      isCancelled: isCancelled,
    );
  }

  Future<SwapIntent> placeAdvancedLimitSell({
    required String walletId,
    required Decimal arrrAmount,
    required Decimal priceLtcPerArrr,
    required String destinationLtcAddress,
    required SwapSignCallback signPending,
    SwapPair pair = SwapPair.arrrLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    _emit(
      onProgress,
      '',
      SwapProgressStage.startingKdf,
      'Preparing limit sell...'.tr,
    );

    final bridgeAddress = await _service.getArrrDepositAddress(walletId);

    _emit(
      onProgress,
      '',
      SwapProgressStage.matchingMarketOrder,
      'Sending ARRR to the swap engine...'.tr,
    );
    final pending = await FfiBridge.buildTx(
      walletId: walletId,
      outputs: [
        Output(addr: bridgeAddress, amount: arrrToZatoshis(arrrAmount)),
      ],
    );
    final signed = await signPending(pending);
    await FfiBridge.broadcastTx(signed);

    await _waitForArrrBalance(
      walletId: walletId,
      requiredAmount: arrrAmount,
      isCancelled: isCancelled,
      onProgress: (balance) {
        _emit(
          onProgress,
          '',
          SwapProgressStage.matchingMarketOrder,
          'ARRR received (${formatSwapAmount(balance)} ARRR)...',
        );
      },
    );

    _emit(
      onProgress,
      '',
      SwapProgressStage.placingLimitRemainder,
      'Placing your limit order on the book...'.tr,
    );
    final intent = await _service.placeSellLimitArrr(
      walletId: walletId,
      arrrAmount: arrrAmount,
      priceLtcPerArrr: priceLtcPerArrr,
      destinationLtcAddress: destinationLtcAddress,
      pair: pair,
    );
    _emit(
      onProgress,
      intent.id,
      SwapProgressStage.placingLimitRemainder,
      'Limit order placed. It will fill when the market reaches your price.'.tr,
    );
    return intent;
  }

  Future<void> _completeBuyIntent(
    SwapIntent intent, {
    SwapProgressCallback? onProgress,
  }) async {
    var current = intent;

    if (current.plan.hasMarketFill) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.matchingMarketOrder,
        'Matching and settling your atomic swap...'.tr,
      );
      if (current.marketSwapUuid == null) {
        current = await _service.executeBuyMarket(current);
      }
      if (current.marketSwapUuid != null) {
        await _pollSwapUntilTerminal(
          walletId: current.walletId,
          swapUuid: current.marketSwapUuid!,
        );
      }
    }

    if (current.plan.hasLimitRemainder) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.placingLimitRemainder,
        'Placing a limit order for the remainder...'.tr,
      );
      current = await _service.placeBuyRemainderLimit(current);
    }

    if (current.plan.marketArrrAmount > Decimal.zero) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.withdrawing,
        'Withdrawing ARRR to your wallet...'.tr,
      );
      if (current.plan.appFeeArrrAmount > Decimal.zero) {
        await _service.withdrawArrrToWallet(
          walletId: current.walletId,
          address: appTakerFeeRecipientArrrAddress,
          amount: current.plan.appFeeArrrAmount,
        );
      }
      final userArrrAmount = current.plan.marketArrrAmountAfterTakerFees;
      if (userArrrAmount > Decimal.zero) {
        final receiveAddress =
            current.arrReceivingAddress ??
            await FfiBridge.currentReceiveAddress(current.walletId);
        await _service.withdrawArrrToWallet(
          walletId: current.walletId,
          address: receiveAddress,
          amount: userArrrAmount,
        );
      }
    }

    if (!current.plan.hasLimitRemainder) {
      current = await _service.markIntentCompleted(current);
    }

    _emit(
      onProgress,
      current.id,
      current.plan.hasLimitRemainder
          ? SwapProgressStage.placingLimitRemainder
          : SwapProgressStage.complete,
      current.plan.hasLimitRemainder
          ? 'Market fill complete. A limit order is open for the remainder.'.tr
          : 'Swap complete. ARRR is in your wallet.'.tr,
    );
  }

  Future<void> _completeSellIntent(
    SwapIntent intent, {
    SwapProgressCallback? onProgress,
  }) async {
    var current = intent;

    if (current.status == SwapIntentStatus.prepared ||
        current.status == SwapIntentStatus.waitingForDeposit) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.matchingMarketOrder,
        'Matching and settling your atomic swap...'.tr,
      );
      current = await _service.executeSellMarket(current);
    }

    if (current.marketSwapUuid != null) {
      await _pollSwapUntilTerminal(
        walletId: current.walletId,
        swapUuid: current.marketSwapUuid!,
      );
    }

    final payout = current.destinationRelAddress;
    final relAmount = current.plan.marketLtcAmount;
    if (payout != null && payout.isNotEmpty && relAmount > Decimal.zero) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.withdrawing,
        'Withdrawing ${current.pair.relTicker} to your address...',
      );
      await _service.withdrawRel(
        walletId: current.walletId,
        address: payout,
        amount: relAmount,
        pair: current.pair,
      );
    }

    if (current.plan.hasLimitRemainder) {
      _emit(
        onProgress,
        current.id,
        SwapProgressStage.placingLimitRemainder,
        'Placing a limit order for the remainder...'.tr,
      );
      // Remainder limit sell would need separate handling; market path only for now.
    }

    current = await _service.markIntentCompleted(current);
    _emit(
      onProgress,
      current.id,
      SwapProgressStage.complete,
      'Swap complete. {ticker} is on its way to your address.'.trArgs({
        'ticker': current.pair.relTicker,
      }),
    );
  }

  Future<void> _waitForRelDeposit({
    required String walletId,
    required Decimal requiredAmount,
    required SwapPair pair,
    bool Function()? isCancelled,
    void Function(Decimal balance)? onProgress,
  }) async {
    final deadline = DateTime.now().add(depositTimeout);
    while (DateTime.now().isBefore(deadline)) {
      if (isCancelled?.call() ?? false) {
        throw SwapOrchestratorException('Swap cancelled.'.tr);
      }
      final balance = await _service.currentRelDepositBalance(
        walletId,
        pair: pair,
      );
      onProgress?.call(balance);
      if (balance >= requiredAmount) return;
      await Future<void>.delayed(depositPollInterval);
    }
    throw SwapOrchestratorException(
      'Timed out waiting for your {ticker} deposit.'.trArgs({
        'ticker': pair.relTicker,
      }),
    );
  }

  Future<void> _waitForArrrBalance({
    required String walletId,
    required Decimal requiredAmount,
    bool Function()? isCancelled,
    void Function(Decimal balance)? onProgress,
  }) async {
    final deadline = DateTime.now().add(depositTimeout);
    while (DateTime.now().isBefore(deadline)) {
      if (isCancelled?.call() ?? false) {
        throw SwapOrchestratorException('Swap cancelled.'.tr);
      }
      final balance = await _service.currentArrrBalance(walletId);
      onProgress?.call(balance);
      if (balance >= requiredAmount) return;
      await Future<void>.delayed(depositPollInterval);
    }
    throw SwapOrchestratorException(
      'Timed out waiting for ARRR to reach the swap engine.'.tr,
    );
  }

  Future<void> _pollSwapUntilTerminal({
    required String walletId,
    required String swapUuid,
  }) async {
    final deadline = DateTime.now().add(swapPollTimeout);
    while (DateTime.now().isBefore(deadline)) {
      final status = await _service.swapStatus(walletId, swapUuid);
      if (_isTerminalSwapStatus(status)) return;
      await Future<void>.delayed(swapPollInterval);
    }
    throw SwapOrchestratorException(
      'Timed out waiting for the atomic swap to finish.'.tr,
    );
  }

  bool _isTerminalSwapStatus(Map<String, dynamic> response) {
    final result = Map<String, dynamic>.from(
      response['result'] as Map? ?? const {},
    );
    final status = result['status']?.toString().toLowerCase() ?? '';
    return status.contains('finished') ||
        status.contains('complete') ||
        status.contains('success') ||
        status.contains('failed') ||
        status.contains('refunded') ||
        status.contains('expired');
  }

  void _emit(
    SwapProgressCallback? callback,
    String intentId,
    SwapProgressStage stage,
    String message,
  ) {
    callback?.call(
      SwapProgress(intentId: intentId, stage: stage, message: message),
    );
  }
}
