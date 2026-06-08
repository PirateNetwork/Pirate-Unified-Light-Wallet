import 'dart:async';

import 'package:decimal/decimal.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/swaps/ltc_address.dart';
import '../../core/swaps/swap_amount_utils.dart';
import '../../core/swaps/swap_error_messages.dart';
import '../../core/swaps/swap_models.dart';
import '../../core/swaps/swap_orchestrator.dart';
import '../../core/swaps/swap_providers.dart';
import '../../features/settings/providers/preferences_providers.dart';

enum SwapUiStep { compose, review, deposit, executing, complete, error }

enum SwapAdvancedOrderType { market, limit }

enum SwapAmountInputMode { pay, receive }

/// What kind of action just finished, so the completion screen can use the
/// right copy (an instant swap vs. a resting limit order that was placed).
enum SwapCompletionKind { swap, limitOrder }

Duration _swapBootstrapRetryDelay(int attempt) {
  if (attempt < 2) return const Duration(seconds: 2);
  if (attempt < 5) return const Duration(seconds: 5);
  return const Duration(seconds: 15);
}

class SwapViewModelState {
  const SwapViewModelState({
    this.step = SwapUiStep.compose,
    this.pair = SwapPair.arrrLtc,
    this.side = SwapSide.buyArrr,
    this.payAmountText = '',
    this.receiveAmountText = '',
    this.amountInputMode = SwapAmountInputMode.pay,
    this.ltcPayoutAddress = '',
    this.slippagePercent = 5.0,
    this.advancedOrderType = SwapAdvancedOrderType.market,
    this.limitPriceText = '',
    this.isQuoting = false,
    this.isExecuting = false,
    this.quoteError,
    this.executionError,
    this.quote,
    this.activeIntent,
    this.progress,
    this.progressMessage,
    this.orderbookAsks = const [],
    this.orderbookBids = const [],
    this.isLoadingOrderbook = false,
    this.fundingLtcBalance,
    this.fundingArrrBalance,
    this.fundingVarrrBalance,
    this.isLoadingFundingBalance = false,
    this.isFundingActionPending = false,
    this.fundingBalanceError,
    this.fundingActionMessage,
    this.incompleteIntents = const [],
    this.history = const [],
    this.useAdvancedOverride,
    this.capNotice,
    this.limitPriceError,
    this.completionKind,
    this.depositDetected = false,
  });

  final SwapUiStep step;
  final SwapPair pair;
  final SwapSide side;
  final String payAmountText;
  final String receiveAmountText;
  final SwapAmountInputMode amountInputMode;
  final String ltcPayoutAddress;
  final double slippagePercent;
  final SwapAdvancedOrderType advancedOrderType;
  final String limitPriceText;
  final bool isQuoting;
  final bool isExecuting;
  final String? quoteError;
  final String? executionError;
  final SwapQuote? quote;
  final SwapIntent? activeIntent;
  final SwapProgressStage? progress;
  final String? progressMessage;
  final List<SwapOrderbookLevel> orderbookAsks;
  final List<SwapOrderbookLevel> orderbookBids;
  final bool isLoadingOrderbook;
  final Decimal? fundingLtcBalance;
  final Decimal? fundingArrrBalance;
  final Decimal? fundingVarrrBalance;
  final bool isLoadingFundingBalance;
  final bool isFundingActionPending;
  final String? fundingBalanceError;
  final String? fundingActionMessage;
  final List<SwapIntent> incompleteIntents;

  /// All persisted intents (most recent first) used for the history list.
  final List<SwapIntent> history;
  final bool? useAdvancedOverride;

  /// Informational notice (not an error) - e.g. amount auto-capped by slippage.
  final String? capNotice;

  /// Validation message for the limit-price field (advanced limit orders).
  final String? limitPriceError;

  /// Set when [SwapUiStep.complete] is reached so the UI can show the right
  /// copy for a finished swap vs. a placed limit order.
  final SwapCompletionKind? completionKind;

  /// True once the awaited LTC deposit has been seen on the deposit screen.
  final bool depositDetected;

  bool get isBuy => side == SwapSide.buyArrr;
  SwapAsset get payAsset => isBuy ? pair.relAsset : SwapAsset.arrr;
  SwapAsset get receiveAsset => isBuy ? SwapAsset.arrr : pair.relAsset;
  String get relTicker => pair.relTicker;
  bool get isAdvanced =>
      useAdvancedOverride ??
      false; // resolved in provider with preference fallback
  bool get isLimit =>
      isAdvanced && advancedOrderType == SwapAdvancedOrderType.limit;

  SwapViewModelState copyWith({
    SwapUiStep? step,
    SwapPair? pair,
    SwapSide? side,
    String? payAmountText,
    String? receiveAmountText,
    SwapAmountInputMode? amountInputMode,
    String? ltcPayoutAddress,
    double? slippagePercent,
    SwapAdvancedOrderType? advancedOrderType,
    String? limitPriceText,
    bool? isQuoting,
    bool? isExecuting,
    String? quoteError,
    String? executionError,
    SwapQuote? quote,
    SwapIntent? activeIntent,
    SwapProgressStage? progress,
    String? progressMessage,
    List<SwapOrderbookLevel>? orderbookAsks,
    List<SwapOrderbookLevel>? orderbookBids,
    bool? isLoadingOrderbook,
    Decimal? fundingLtcBalance,
    Decimal? fundingArrrBalance,
    Decimal? fundingVarrrBalance,
    bool? isLoadingFundingBalance,
    bool? isFundingActionPending,
    String? fundingBalanceError,
    String? fundingActionMessage,
    List<SwapIntent>? incompleteIntents,
    List<SwapIntent>? history,
    bool? useAdvancedOverride,
    String? capNotice,
    String? limitPriceError,
    SwapCompletionKind? completionKind,
    bool? depositDetected,
    bool clearQuote = false,
    bool clearIntent = false,
    bool clearErrors = false,
    bool clearCapNotice = false,
    bool clearFundingMessages = false,
    bool clearLimitPriceError = false,
    bool clearCompletionKind = false,
  }) {
    return SwapViewModelState(
      step: step ?? this.step,
      pair: pair ?? this.pair,
      side: side ?? this.side,
      payAmountText: payAmountText ?? this.payAmountText,
      receiveAmountText: receiveAmountText ?? this.receiveAmountText,
      amountInputMode: amountInputMode ?? this.amountInputMode,
      ltcPayoutAddress: ltcPayoutAddress ?? this.ltcPayoutAddress,
      slippagePercent: slippagePercent ?? this.slippagePercent,
      advancedOrderType: advancedOrderType ?? this.advancedOrderType,
      limitPriceText: limitPriceText ?? this.limitPriceText,
      isQuoting: isQuoting ?? this.isQuoting,
      isExecuting: isExecuting ?? this.isExecuting,
      quoteError: clearErrors ? null : (quoteError ?? this.quoteError),
      executionError: clearErrors
          ? null
          : (executionError ?? this.executionError),
      quote: clearQuote ? null : (quote ?? this.quote),
      activeIntent: clearIntent ? null : (activeIntent ?? this.activeIntent),
      progress: progress ?? this.progress,
      progressMessage: progressMessage ?? this.progressMessage,
      orderbookAsks: orderbookAsks ?? this.orderbookAsks,
      orderbookBids: orderbookBids ?? this.orderbookBids,
      isLoadingOrderbook: isLoadingOrderbook ?? this.isLoadingOrderbook,
      fundingLtcBalance: fundingLtcBalance ?? this.fundingLtcBalance,
      fundingArrrBalance: fundingArrrBalance ?? this.fundingArrrBalance,
      fundingVarrrBalance: fundingVarrrBalance ?? this.fundingVarrrBalance,
      isLoadingFundingBalance:
          isLoadingFundingBalance ?? this.isLoadingFundingBalance,
      isFundingActionPending:
          isFundingActionPending ?? this.isFundingActionPending,
      fundingBalanceError: clearFundingMessages
          ? null
          : (fundingBalanceError ?? this.fundingBalanceError),
      fundingActionMessage: clearFundingMessages
          ? null
          : (fundingActionMessage ?? this.fundingActionMessage),
      incompleteIntents: incompleteIntents ?? this.incompleteIntents,
      history: history ?? this.history,
      useAdvancedOverride: useAdvancedOverride ?? this.useAdvancedOverride,
      capNotice: clearCapNotice ? null : (capNotice ?? this.capNotice),
      limitPriceError: clearLimitPriceError
          ? null
          : (limitPriceError ?? this.limitPriceError),
      completionKind: clearCompletionKind
          ? null
          : (completionKind ?? this.completionKind),
      depositDetected: depositDetected ?? this.depositDetected,
    );
  }
}

class SwapViewModel extends Notifier<SwapViewModelState> {
  static const _depositPollInterval = Duration(seconds: 8);

  Timer? _quoteDebounce;
  Timer? _depositPollTimer;
  Timer? _bootstrapRetryTimer;
  bool _cancelRequested = false;
  String? _lastBootstrapWalletId;
  String? _bootstrapInFlightWalletId;

  SwapOrchestrator get _orchestrator => ref.read(swapOrchestratorProvider);
  KdfSwapWarmupService get _warmup => ref.read(kdfSwapWarmupServiceProvider);

  /// The effective advanced flag, resolving the in-session override against the
  /// saved preference. [SwapViewModelState.isAdvanced] only knows about the
  /// override, so business logic must use this.
  bool get _advanced =>
      state.useAdvancedOverride ??
      (ref.read(swapInterfacePreferenceProvider) ==
          SwapInterfacePreference.advanced);

  bool get _limit =>
      _advanced && state.advancedOrderType == SwapAdvancedOrderType.limit;

  @override
  SwapViewModelState build() {
    ref
      ..onDispose(() {
        _quoteDebounce?.cancel();
        _depositPollTimer?.cancel();
        _bootstrapRetryTimer?.cancel();
      })
      ..listen<WalletId?>(activeWalletProvider, (previous, next) {
        if (next == null || next == previous) return;
        _lastBootstrapWalletId = null;
        _bootstrapInFlightWalletId = null;
        _bootstrapRetryTimer?.cancel();
        unawaited(_bootstrapForWallet(next));
      });
    Future.microtask(_bootstrap);
    return const SwapViewModelState();
  }

  Future<void> _bootstrap() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;
    await _bootstrapForWallet(walletId);
  }

  Future<void> _bootstrapForWallet(String walletId, {int attempt = 0}) async {
    if (_lastBootstrapWalletId == walletId ||
        _bootstrapInFlightWalletId == walletId) {
      return;
    }
    _bootstrapInFlightWalletId = walletId;
    _bootstrapRetryTimer?.cancel();

    try {
      await _warmup.warm(walletId, pair: state.pair);
    } catch (_) {
      _scheduleBootstrapRetry(walletId, attempt);
      return;
    } finally {
      if (_bootstrapInFlightWalletId == walletId) {
        _bootstrapInFlightWalletId = null;
      }
    }

    _lastBootstrapWalletId = walletId;
    try {
      final incomplete = await _orchestrator.resumeIncomplete(walletId);
      state = state.copyWith(incompleteIntents: incomplete);
      unawaited(_loadHistory(walletId));
    } catch (_) {
      // Resume/history are best-effort on first load.
    } finally {
      unawaited(refreshFundingBalances());
    }
  }

  void _scheduleBootstrapRetry(String walletId, int attempt) {
    _bootstrapRetryTimer?.cancel();
    _bootstrapRetryTimer = Timer(_swapBootstrapRetryDelay(attempt), () {
      unawaited(_bootstrapForWallet(walletId, attempt: attempt + 1));
    });
  }

  Future<void> _loadHistory(String walletId) async {
    try {
      final history = await _orchestrator.loadHistory(walletId);
      state = state.copyWith(history: history);
    } catch (_) {
      // History is best-effort.
    }
  }

  void setAdvancedOverride({required bool? value}) {
    state = state.copyWith(
      useAdvancedOverride: value,
      amountInputMode: SwapAmountInputMode.pay,
      clearQuote: true,
    );
    if (value ?? false) {
      unawaited(refreshOrderbook());
    }
  }

  void setSide(SwapSide side) {
    if (state.side == side) return;
    state = state.copyWith(
      side: side,
      clearQuote: true,
      clearErrors: true,
      payAmountText: '',
      receiveAmountText: '',
      amountInputMode: SwapAmountInputMode.pay,
    );
  }

  void setPair(SwapPair pair) {
    if (state.pair == pair) return;
    state = state.copyWith(
      pair: pair,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
      clearFundingMessages: true,
      payAmountText: '',
      receiveAmountText: '',
      limitPriceText: '',
      amountInputMode: SwapAmountInputMode.pay,
    );
    unawaited(refreshOrderbook());
    unawaited(refreshFundingBalances());
  }

  void selectAdvancedOrderbookDepth({
    required SwapSide side,
    required Decimal priceRelPerArrr,
    required Decimal payAmount,
  }) {
    if (priceRelPerArrr <= Decimal.zero || payAmount <= Decimal.zero) return;
    final receive = side == SwapSide.buyArrr
        ? (payAmount / priceRelPerArrr).toDecimal(scaleOnInfinitePrecision: 8)
        : (payAmount * priceRelPerArrr);
    state = state.copyWith(
      side: side,
      advancedOrderType: SwapAdvancedOrderType.limit,
      limitPriceText: formatSwapAmountFixed(priceRelPerArrr, fractionDigits: 8),
      payAmountText: formatSwapAmount(payAmount, fractionDigits: 8),
      receiveAmountText: formatSwapAmount(receive, fractionDigits: 8),
      amountInputMode: SwapAmountInputMode.pay,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
      clearLimitPriceError: true,
    );
  }

  void selectSimpleAsset({required bool isPayRow, required SwapAsset asset}) {
    final currentPay = state.payAsset;
    final currentReceive = state.receiveAsset;
    var nextPay = isPayRow ? asset : currentPay;
    var nextReceive = isPayRow ? currentReceive : asset;

    // If the user selects the coin currently on the opposite row, treat that
    // as a flip. This avoids invalid duplicate assets such as ARRR/ARRR.
    if (nextPay == nextReceive) {
      if (isPayRow) {
        nextReceive = currentPay;
      } else {
        nextPay = currentReceive;
      }
    }

    final next = _swapStateForAssets(pay: nextPay, receive: nextReceive);
    if (next == null || (next.pair == state.pair && next.side == state.side)) {
      return;
    }

    state = state.copyWith(
      pair: next.pair,
      side: next.side,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
      clearFundingMessages: true,
      payAmountText: '',
      receiveAmountText: '',
      limitPriceText: '',
      amountInputMode: SwapAmountInputMode.pay,
    );
    unawaited(refreshOrderbook());
    unawaited(refreshFundingBalances());
  }

  ({SwapPair pair, SwapSide side})? _swapStateForAssets({
    required SwapAsset pay,
    required SwapAsset receive,
  }) {
    if (pay == receive) return null;
    if (receive == SwapAsset.arrr && pay != SwapAsset.arrr) {
      final pair = _pairForRelAsset(pay);
      return pair == null ? null : (pair: pair, side: SwapSide.buyArrr);
    }
    if (pay == SwapAsset.arrr && receive != SwapAsset.arrr) {
      final pair = _pairForRelAsset(receive);
      return pair == null ? null : (pair: pair, side: SwapSide.sellArrr);
    }
    return null;
  }

  SwapPair? _pairForRelAsset(SwapAsset asset) {
    for (final pair in SwapPair.values) {
      if (pair.relAsset == asset) return pair;
    }
    return null;
  }

  void flipSide() {
    state = state.copyWith(
      side: state.isBuy ? SwapSide.sellArrr : SwapSide.buyArrr,
      clearQuote: true,
      clearErrors: true,
      payAmountText: '',
      receiveAmountText: '',
      amountInputMode: SwapAmountInputMode.pay,
    );
  }

  void updatePayAmount(String value) {
    state = state.copyWith(
      payAmountText: value,
      amountInputMode: SwapAmountInputMode.pay,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
    );
    _scheduleQuote();
  }

  void updateReceiveAmount(String value) {
    state = state.copyWith(
      receiveAmountText: value,
      amountInputMode: SwapAmountInputMode.receive,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
    );
    _scheduleQuote();
  }

  void updateLtcPayoutAddress(String value) {
    state = state.copyWith(ltcPayoutAddress: value.trim());
  }

  void updateSlippage(double value) {
    state = state.copyWith(
      slippagePercent: value,
      clearErrors: true,
      clearCapNotice: true,
    );
    _scheduleQuote();
  }

  void updateLimitPrice(String value) {
    state = state.copyWith(
      limitPriceText: value,
      clearQuote: true,
      clearLimitPriceError: true,
    );
    _scheduleQuote();
  }

  void setAdvancedOrderType(SwapAdvancedOrderType type) {
    state = state.copyWith(advancedOrderType: type, clearQuote: true);
    _scheduleQuote();
  }

  void goToReview() {
    if (state.quote == null) return;
    if (!_validateSellAddressOrReport()) return;
    state = state.copyWith(step: SwapUiStep.review, clearErrors: true);
  }

  void backToCompose() {
    state = state.copyWith(step: SwapUiStep.compose, clearErrors: true);
  }

  Future<void> refreshOrderbook() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;
    if (state.isLoadingOrderbook) return;
    state = state.copyWith(isLoadingOrderbook: true);
    try {
      await _warmup.warm(walletId, pair: state.pair);
      final book = await _orchestrator.loadOrderbook(
        walletId,
        pair: state.pair,
      );
      state = state.copyWith(
        orderbookAsks: book.asks,
        orderbookBids: book.bids,
        isLoadingOrderbook: false,
        clearErrors: true,
      );
    } catch (error) {
      state = state.copyWith(
        isLoadingOrderbook: false,
        quoteError: friendlySwapError(error),
      );
    }
  }

  Future<void> refreshFundingBalances({bool clearMessages = true}) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null || state.isLoadingFundingBalance) return;

    state = state.copyWith(
      isLoadingFundingBalance: true,
      clearFundingMessages: clearMessages,
    );
    try {
      await _warmup.warm(walletId);
      final arrr = await _orchestrator.currentFundingBalance(
        walletId,
        asset: SwapAsset.arrr,
      );
      final varrr = await _orchestrator.currentFundingBalance(
        walletId,
        asset: SwapAsset.varrr,
      );
      final ltc = await _orchestrator.currentFundingBalance(
        walletId,
        asset: SwapAsset.ltc,
      );
      state = state.copyWith(
        fundingLtcBalance: ltc,
        fundingArrrBalance: arrr,
        fundingVarrrBalance: varrr,
        isLoadingFundingBalance: false,
        clearFundingMessages: true,
      );
    } catch (error) {
      state = state.copyWith(
        isLoadingFundingBalance: false,
        fundingBalanceError: friendlySwapError(error),
      );
    }
  }

  void useFundingLtcBalance() {
    final balance = state.fundingLtcBalance;
    if (balance == null || balance <= Decimal.zero) return;
    state = state.copyWith(
      step: SwapUiStep.compose,
      side: SwapSide.buyArrr,
      payAmountText: formatSwapAmount(balance, fractionDigits: 8),
      amountInputMode: SwapAmountInputMode.pay,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
    );
    _scheduleQuote();
  }

  Future<String> getFundingDepositAddress(SwapAsset asset) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      throw const SwapOrchestratorException('No wallet is currently unlocked.');
    }

    state = state.copyWith(
      isFundingActionPending: true,
      clearFundingMessages: true,
    );
    try {
      final address = await _orchestrator.getFundingDepositAddress(
        walletId,
        asset: asset,
      );
      state = state.copyWith(isFundingActionPending: false);
      return address;
    } catch (error) {
      state = state.copyWith(
        isFundingActionPending: false,
        fundingBalanceError: friendlySwapError(error),
      );
      rethrow;
    }
  }

  Future<void> withdrawFundingAsset({
    required SwapAsset asset,
    required String address,
    required Decimal amount,
  }) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null || amount <= Decimal.zero) return;

    state = state.copyWith(
      isFundingActionPending: true,
      clearFundingMessages: true,
    );
    try {
      await _orchestrator.withdrawFundingAsset(
        walletId: walletId,
        asset: asset,
        address: address,
        amount: amount,
      );
      state = state.copyWith(
        isFundingActionPending: false,
        fundingActionMessage:
            'Sending ${formatSwapAmount(amount, fractionDigits: 8)} ${asset.ticker} from the swap funding wallet. It will appear after the network confirms it.',
      );
      await refreshFundingBalances(clearMessages: false);
    } catch (error) {
      state = state.copyWith(
        isFundingActionPending: false,
        fundingBalanceError: friendlySwapError(error),
      );
    }
  }

  Future<void> withdrawFundingLtc(String address) async {
    final walletId = ref.read(activeWalletProvider);
    final balance = state.fundingLtcBalance;
    if (walletId == null || balance == null || balance <= Decimal.zero) return;

    state = state.copyWith(
      isFundingActionPending: true,
      clearFundingMessages: true,
    );
    try {
      await _orchestrator.withdrawAllFundingRel(
        walletId: walletId,
        address: address,
        pair: state.pair,
      );
      final ticker = state.pair.relTicker;
      state = state.copyWith(
        isFundingActionPending: false,
        fundingActionMessage:
            'Refunding ${formatSwapAmount(balance, fractionDigits: 8)} $ticker to your address. It will appear after the network confirms it.',
      );
      await refreshFundingBalances(clearMessages: false);
    } catch (error) {
      state = state.copyWith(
        isFundingActionPending: false,
        fundingBalanceError: friendlySwapError(error),
      );
    }
  }

  Future<void> withdrawFundingArrrToWallet() async {
    final walletId = ref.read(activeWalletProvider);
    final balance = state.fundingArrrBalance;
    if (walletId == null || balance == null || balance <= Decimal.zero) return;

    state = state.copyWith(
      isFundingActionPending: true,
      clearFundingMessages: true,
    );
    try {
      await _orchestrator.withdrawAllFundingArrrToWallet(walletId);
      state = state.copyWith(
        isFundingActionPending: false,
        fundingActionMessage:
            'Sending ${formatSwapAmount(balance, fractionDigits: 8)} ARRR back to your wallet. It will appear after the network confirms it.',
      );
      await refreshFundingBalances(clearMessages: false);
    } catch (error) {
      state = state.copyWith(
        isFundingActionPending: false,
        fundingBalanceError: friendlySwapError(error),
      );
    }
  }

  /// Validates that a sell has a usable payout address. Returns true when
  /// OK; otherwise sets [SwapViewModelState.quoteError] and returns false.
  bool _validateSellAddressOrReport() {
    if (state.isBuy) return true;
    if (state.pair.relAsset == SwapAsset.ltc) {
      final check = checkLtcAddress(state.ltcPayoutAddress);
      if (!check.isValid) {
        state = state.copyWith(quoteError: check.error);
        return false;
      }
    } else if (state.ltcPayoutAddress.trim().isEmpty) {
      state = state.copyWith(
        quoteError: 'Enter a ${state.pair.relTicker} receiving address.',
      );
      return false;
    }
    return true;
  }

  Future<void> confirmSwap() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;
    if (_limit) {
      if (!hasValidLimitInputs) return;
    } else if (state.quote == null) {
      return;
    }
    if (!_validateSellAddressOrReport()) return;

    state = state.copyWith(clearErrors: true, clearCompletionKind: true);
    _cancelRequested = false;

    try {
      if (_limit) {
        await _confirmLimit(walletId);
      } else if (state.isBuy) {
        final ltcAmount = _parseDecimal(state.payAmountText);
        final intent = await _orchestrator.prepareBuy(
          walletId: walletId,
          ltcAmount: ltcAmount,
          slippageCap: _slippageCap(),
          pair: state.pair,
        );
        state = state.copyWith(
          activeIntent: intent,
          step: SwapUiStep.deposit,
          isExecuting: false,
          depositDetected: false,
        );
        _startDepositPolling();
      } else {
        state = state.copyWith(isExecuting: true, step: SwapUiStep.executing);
        final arrrAmount = _parseDecimal(state.payAmountText);
        final intent = await _orchestrator.prepareSell(
          walletId: walletId,
          arrrAmount: arrrAmount,
          destinationLtcAddress: state.ltcPayoutAddress,
          slippageCap: _slippageCap(),
          pair: state.pair,
        );
        await _orchestrator.executeSellFlow(
          intent: intent,
          arrrAmount: arrrAmount,
          signPending: _signPending,
          isCancelled: () => _cancelRequested,
          onProgress: _onProgress,
        );
        state = state.copyWith(
          step: SwapUiStep.complete,
          isExecuting: false,
          progress: SwapProgressStage.complete,
          completionKind: SwapCompletionKind.swap,
        );
        _refreshAfterTerminal(walletId);
      }
    } catch (error) {
      _failWith(error);
    }
  }

  Future<void> _confirmLimit(String walletId) async {
    final amount = _parseDecimal(state.payAmountText);
    final price = _parseDecimal(state.limitPriceText);
    if (state.isBuy) {
      final intent = await _orchestrator.prepareBuyLimit(
        walletId: walletId,
        ltcAmount: amount,
        priceLtcPerArrr: price,
        pair: state.pair,
      );
      state = state.copyWith(
        activeIntent: intent,
        step: SwapUiStep.deposit,
        isExecuting: false,
        depositDetected: false,
      );
      _startDepositPolling();
    } else {
      state = state.copyWith(isExecuting: true, step: SwapUiStep.executing);
      await _orchestrator.executeSellLimitFlow(
        walletId: walletId,
        arrrAmount: amount,
        priceLtcPerArrr: price,
        destinationLtcAddress: state.ltcPayoutAddress,
        signPending: _signPending,
        pair: state.pair,
        isCancelled: () => _cancelRequested,
        onProgress: _onProgress,
      );
      state = state.copyWith(
        step: SwapUiStep.complete,
        isExecuting: false,
        progress: SwapProgressStage.placingLimitRemainder,
        completionKind: SwapCompletionKind.limitOrder,
      );
      _refreshAfterTerminal(walletId);
    }
  }

  Future<void> confirmDepositAndExecute() async {
    final intent = state.activeIntent;
    if (intent == null) return;
    _stopDepositPolling();
    final requiredLtc = intent.requestedPayAmount;
    final isLimitBuy =
        intent.plan.limitPriceLtcPerArrr != null && !intent.plan.hasMarketFill;
    state = state.copyWith(step: SwapUiStep.executing, isExecuting: true);
    try {
      if (isLimitBuy) {
        await _orchestrator.executeBuyLimitFlow(
          intent: intent,
          requiredLtc: requiredLtc,
          isCancelled: () => _cancelRequested,
          onProgress: _onProgress,
        );
        state = state.copyWith(
          step: SwapUiStep.complete,
          isExecuting: false,
          progress: SwapProgressStage.placingLimitRemainder,
          completionKind: SwapCompletionKind.limitOrder,
        );
      } else {
        await _orchestrator.executeBuyFlow(
          intent: intent,
          requiredLtc: requiredLtc,
          isCancelled: () => _cancelRequested,
          onProgress: _onProgress,
        );
        state = state.copyWith(
          step: SwapUiStep.complete,
          isExecuting: false,
          progress: SwapProgressStage.complete,
          completionKind: SwapCompletionKind.swap,
        );
      }
      _refreshAfterTerminal(intent.walletId);
    } catch (error) {
      _failWith(error);
    }
  }

  Future<void> checkDepositOrStartSwap() async {
    if (state.depositDetected) {
      await confirmDepositAndExecute();
      return;
    }
    await recheckDeposit();
  }

  void _failWith(Object error) {
    state = state.copyWith(
      step: SwapUiStep.error,
      isExecuting: false,
      executionError: friendlySwapError(error),
      progress: SwapProgressStage.failed,
    );
    final walletId = ref.read(activeWalletProvider);
    if (walletId != null) _refreshAfterTerminal(walletId);
  }

  void _refreshAfterTerminal(String walletId) {
    unawaited(_loadHistory(walletId));
    unawaited(() async {
      try {
        final incomplete = await _orchestrator.resumeIncomplete(walletId);
        state = state.copyWith(incompleteIntents: incomplete);
      } catch (_) {
        // best-effort
      }
    }());
  }

  void cancelSwap() {
    _stopDepositPolling();
    final intent = state.activeIntent;
    _cancelRequested = true;
    if (intent != null) {
      unawaited(_orchestrator.cancelIntent(intent));
    }
    state = state.copyWith(
      step: SwapUiStep.compose,
      isExecuting: false,
      clearIntent: true,
      clearQuote: true,
    );
    final walletId = ref.read(activeWalletProvider);
    if (walletId != null) _refreshAfterTerminal(walletId);
  }

  /// Returns to compose after a failure, preserving the user's inputs so they
  /// can adjust and try again (rather than starting from scratch).
  void retryAfterError() {
    _cancelRequested = false;
    state = state.copyWith(
      step: SwapUiStep.compose,
      isExecuting: false,
      clearIntent: true,
      clearQuote: true,
      clearErrors: true,
      clearCompletionKind: true,
      progress: null,
    );
    unawaited(_warmThenRefresh());
  }

  Future<void> _warmThenRefresh() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;

    try {
      await _warmup.warm(walletId, pair: state.pair);
    } catch (error) {
      final message = friendlySwapError(error);
      state = state.copyWith(quoteError: message, fundingBalanceError: message);
      return;
    }

    _scheduleQuote();
    unawaited(refreshFundingBalances(clearMessages: false));
    if (_advanced) {
      unawaited(refreshOrderbook());
    }
  }

  void resetAfterComplete() {
    _stopDepositPolling();
    state = state.copyWith(
      step: SwapUiStep.compose,
      clearIntent: true,
      clearQuote: true,
      clearErrors: true,
      clearCompletionKind: true,
      payAmountText: '',
      receiveAmountText: '',
      limitPriceText: '',
      amountInputMode: SwapAmountInputMode.pay,
    );
    unawaited(_bootstrap());
  }

  void resumeIntent(SwapIntent intent) {
    _stopDepositPolling();

    if (intent.status == SwapIntentStatus.waitingForDeposit) {
      if (intent.isDepositExpired(DateTime.now().toUtc())) {
        unawaited(_orchestrator.failIntent(intent, 'Deposit window expired.'));
        state = state.copyWith(
          incompleteIntents: [
            for (final entry in state.incompleteIntents)
              if (entry.id != intent.id) entry,
          ],
        );
        return;
      }
      _showDepositIntent(intent);
      return;
    }

    if (intent.status == SwapIntentStatus.marketSwapStarted) {
      unawaited(_continueStartedSwap(intent));
      return;
    }

    if (intent.status == SwapIntentStatus.limitOrderPlaced) {
      state = state.copyWith(
        activeIntent: intent,
        pair: intent.pair,
        side: intent.side,
        step: SwapUiStep.compose,
        payAmountText: formatSwapAmount(intent.requestedPayAmount),
        receiveAmountText: formatSwapAmount(intent.expectedReceiveAmount),
        amountInputMode: SwapAmountInputMode.pay,
        progress: SwapProgressStage.placingLimitRemainder,
        progressMessage:
            'Limit order is open. You can cancel it from Open swap orders.',
        clearErrors: true,
        clearQuote: true,
      );
      unawaited(refreshOrderbook());
      return;
    }

    if (intent.status == SwapIntentStatus.completed) {
      state = state.copyWith(
        activeIntent: intent,
        pair: intent.pair,
        side: intent.side,
        step: SwapUiStep.complete,
        payAmountText: formatSwapAmount(intent.requestedPayAmount),
        receiveAmountText: formatSwapAmount(intent.expectedReceiveAmount),
        amountInputMode: SwapAmountInputMode.pay,
        progress: SwapProgressStage.complete,
        progressMessage: 'Swap completed successfully.',
        completionKind: SwapCompletionKind.swap,
        clearErrors: true,
        clearQuote: true,
      );
      return;
    }

    if (intent.status == SwapIntentStatus.prepared) {
      state = state.copyWith(
        activeIntent: intent,
        pair: intent.pair,
        side: intent.side,
        step: SwapUiStep.compose,
        payAmountText: formatSwapAmount(intent.requestedPayAmount),
        receiveAmountText: formatSwapAmount(intent.expectedReceiveAmount),
        amountInputMode: SwapAmountInputMode.pay,
        clearErrors: true,
        clearQuote: true,
      );
    }
  }

  Future<void> _continueStartedSwap(SwapIntent intent) async {
    state = state.copyWith(
      activeIntent: intent,
      pair: intent.pair,
      side: intent.side,
      step: SwapUiStep.executing,
      isExecuting: true,
      payAmountText: formatSwapAmount(intent.requestedPayAmount),
      receiveAmountText: formatSwapAmount(intent.expectedReceiveAmount),
      progress: SwapProgressStage.matchingMarketOrder,
      progressMessage: 'Swap is already in progress. Checking status...',
      depositDetected: true,
      clearErrors: true,
    );

    try {
      await _orchestrator.continueStartedSwapFlow(
        intent: intent,
        onProgress: _onProgress,
      );
      state = state.copyWith(
        step: SwapUiStep.complete,
        isExecuting: false,
        progress: intent.status == SwapIntentStatus.limitOrderPlaced
            ? SwapProgressStage.placingLimitRemainder
            : SwapProgressStage.complete,
        completionKind: intent.status == SwapIntentStatus.limitOrderPlaced
            ? SwapCompletionKind.limitOrder
            : SwapCompletionKind.swap,
      );
      _refreshAfterTerminal(intent.walletId);
    } catch (error) {
      _failWith(error);
    }
  }

  Future<void> expireActiveDeposit() async {
    final intent = state.activeIntent;
    if (intent == null ||
        intent.status != SwapIntentStatus.waitingForDeposit ||
        !intent.isDepositExpired(DateTime.now().toUtc())) {
      return;
    }
    _stopDepositPolling();

    await _orchestrator.failIntent(
      intent,
      'Deposit window expired before ${intent.pair.relTicker} was detected.',
    );
    state = state.copyWith(
      step: SwapUiStep.error,
      isExecuting: false,
      progress: SwapProgressStage.failed,
      executionError:
          'Deposit window expired. Start a fresh quote before sending more ${intent.pair.relTicker}. If you already sent funds, check the funding balance once it confirms.',
      clearIntent: true,
      clearQuote: true,
    );
  }

  void _showDepositIntent(SwapIntent intent) {
    state = state.copyWith(
      activeIntent: intent,
      pair: intent.pair,
      side: intent.side,
      step: SwapUiStep.deposit,
      payAmountText: formatSwapAmount(intent.requestedPayAmount),
      receiveAmountText: formatSwapAmount(intent.expectedReceiveAmount),
      amountInputMode: SwapAmountInputMode.pay,
      progressMessage: 'Checking for your ${intent.pair.relTicker} deposit...',
      depositDetected: false,
      clearErrors: true,
    );
    _startDepositPolling();
  }

  void _startDepositPolling() {
    _depositPollTimer?.cancel();
    final intent = state.activeIntent;
    if (intent == null) return;
    unawaited(_pollDeposit(intent, autoAdvance: true));
    _depositPollTimer = Timer.periodic(_depositPollInterval, (_) {
      final current = state.activeIntent;
      if (current == null || state.step != SwapUiStep.deposit) {
        _stopDepositPolling();
        return;
      }
      unawaited(_pollDeposit(current, autoAdvance: true));
    });
  }

  void _stopDepositPolling() {
    _depositPollTimer?.cancel();
    _depositPollTimer = null;
  }

  /// Manual "Check now" action from the deposit screen.
  Future<void> recheckDeposit() async {
    final intent = state.activeIntent;
    if (intent == null || state.step != SwapUiStep.deposit) return;
    await _pollDeposit(intent, autoAdvance: true);
  }

  Future<void> _pollDeposit(
    SwapIntent intent, {
    bool autoAdvance = false,
  }) async {
    if (state.isExecuting) return;
    try {
      final balance = await _orchestrator.currentRelDepositBalance(
        intent.walletId,
        pair: intent.pair,
      );
      if (state.activeIntent?.id != intent.id ||
          state.step != SwapUiStep.deposit) {
        return;
      }

      final requiredAmount = intent.requestedPayAmount;
      final detected = balance >= requiredAmount;
      final message = detected
          ? 'Deposit detected. Starting the atomic swap...'
          : balance > Decimal.zero
          ? 'Detected ${formatSwapAmount(balance, fractionDigits: 8)} ${intent.pair.relTicker} so far. Waiting for ${formatSwapAmount(requiredAmount - balance, fractionDigits: 8)} ${intent.pair.relTicker} more...'
          : 'No confirmed ${intent.pair.relTicker} detected yet. After you send it, the swap starts automatically once enough balance is confirmed.';

      state = state.copyWith(
        progressMessage: message,
        depositDetected: detected,
      );

      if (detected && autoAdvance && !state.isExecuting) {
        _stopDepositPolling();
        await confirmDepositAndExecute();
      }
    } catch (_) {
      if (state.activeIntent?.id == intent.id &&
          state.step == SwapUiStep.deposit) {
        state = state.copyWith(
          progressMessage:
              "Couldn't check the ${intent.pair.relTicker} funding balance right now. Retrying shortly...",
        );
      }
    }
  }

  /// Fills the pay field with the wallet's spendable ARRR (sell side only).
  void useMaxSellAmount() {
    if (state.isBuy) return;
    final balance = ref.read(balanceStreamProvider).asData?.value;
    if (balance == null) return;
    final spendable = arrrFromZatoshis(balance.spendable);
    if (spendable <= Decimal.zero) return;
    state = state.copyWith(
      payAmountText: formatSwapAmount(spendable, fractionDigits: 8),
      amountInputMode: SwapAmountInputMode.pay,
      clearQuote: true,
      clearErrors: true,
      clearCapNotice: true,
    );
    _scheduleQuote();
  }

  void _scheduleQuote() {
    _quoteDebounce?.cancel();
    _quoteDebounce = Timer(const Duration(milliseconds: 450), () {
      unawaited(_refreshQuote());
    });
  }

  Future<void> _refreshQuote() async {
    // Limit orders don't take the market: the receive amount is derived
    // directly from the user's price, so we compute it locally without hitting
    // the order book.
    if (_limit) {
      _recomputeLimitEstimate();
      return;
    }

    final walletId = ref.read(activeWalletProvider);
    final mode = state.amountInputMode;
    final text = switch (mode) {
      SwapAmountInputMode.pay => state.payAmountText.trim(),
      SwapAmountInputMode.receive => state.receiveAmountText.trim(),
    };
    if (walletId == null || text.isEmpty) {
      state = state.copyWith(clearQuote: true, isQuoting: false);
      return;
    }

    Decimal amount;
    try {
      amount = _parseDecimal(text);
      if (amount <= Decimal.zero) {
        state = state.copyWith(clearQuote: true, isQuoting: false);
        return;
      }
    } catch (_) {
      state = state.copyWith(
        isQuoting: false,
        quoteError: 'Enter a valid amount.',
        clearQuote: true,
      );
      return;
    }

    state = state.copyWith(isQuoting: true, clearErrors: true);
    try {
      await _warmup.warm(walletId, pair: state.pair);
      final quote = mode == SwapAmountInputMode.pay
          ? await _quoteFromPayAmount(walletId, amount)
          : await _quoteFromReceiveAmount(walletId, amount);

      // Auto-cap: When the engine returns a remainder due to slippage limits,
      // we hard-lock the user's input to the maximum amount that can actually
      // be filled at the current slippage tolerance. Limit orders return early
      // above, so by here we are always on the market path.
      const isMarket = true;
      Decimal effectivePayAmount = quote.plan.requestedPayAmount;
      String? capNotice;

      if (isMarket && mode == SwapAmountInputMode.pay) {
        effectivePayAmount = amount;
        if (state.isBuy && quote.plan.remainderLtcAmount > Decimal.zero) {
          // For BUY: cap LTC payment to filled LTC only.
          effectivePayAmount = quote.plan.marketLtcAmount;
          capNotice =
              'Capped at ${formatSwapAmount(effectivePayAmount)} ${state.pair.relTicker} due to ${state.slippagePercent.toStringAsFixed(1)}% slippage limit. Increase slippage to fill more.';
        } else if (!state.isBuy &&
            quote.plan.remainderArrrAmount > Decimal.zero) {
          // For SELL: cap ARRR sell to the market-filled gross amount,
          // including the app/KDF taker-fee envelope but excluding the
          // unfilled maker remainder.
          effectivePayAmount =
              quote.plan.marketArrrAmount + quote.plan.totalTakerFeeArrrAmount;
          capNotice =
              'Capped at ${formatSwapAmount(effectivePayAmount)} ARRR due to ${state.slippagePercent.toStringAsFixed(1)}% slippage limit. Increase slippage to fill more.';
        }
      }

      // If we capped the amount, re-quote against the capped amount so the UI
      // reflects clean numbers (no remainder) and we don't double-bill the user.
      if (capNotice != null && effectivePayAmount > Decimal.zero) {
        final cappedQuote = state.isBuy
            ? await _orchestrator.quoteBuy(
                walletId: walletId,
                ltcAmount: effectivePayAmount,
                slippageCap: _slippageCap(),
                pair: state.pair,
              )
            : await _orchestrator.quoteSell(
                walletId: walletId,
                arrrAmount: effectivePayAmount,
                slippageCap: _slippageCap(),
                pair: state.pair,
              );

        final cappedReceive = cappedQuote.plan.expectedReceiveAmount;

        state = state.copyWith(
          quote: cappedQuote,
          payAmountText: formatSwapAmount(effectivePayAmount),
          receiveAmountText: formatSwapAmount(cappedReceive),
          capNotice: capNotice,
          isQuoting: false,
        );
        return;
      }

      final receiveAmount = quote.plan.expectedReceiveAmount;

      state = state.copyWith(
        quote: quote,
        payAmountText: mode == SwapAmountInputMode.receive
            ? formatSwapAmount(quote.plan.requestedPayAmount)
            : state.payAmountText,
        receiveAmountText: formatSwapAmount(receiveAmount),
        isQuoting: false,
        clearCapNotice: true,
      );
    } catch (error) {
      state = state.copyWith(
        isQuoting: false,
        quoteError: friendlySwapError(error),
        clearQuote: true,
      );
    }
  }

  Future<SwapQuote> _quoteFromPayAmount(String walletId, Decimal amount) {
    return state.isBuy
        ? _orchestrator.quoteBuy(
            walletId: walletId,
            ltcAmount: amount,
            slippageCap: _slippageCap(),
            pair: state.pair,
          )
        : _orchestrator.quoteSell(
            walletId: walletId,
            arrrAmount: amount,
            slippageCap: _slippageCap(),
            pair: state.pair,
          );
  }

  Future<SwapQuote> _quoteFromReceiveAmount(
    String walletId,
    Decimal receiveAmount,
  ) async {
    final payAmount = state.isBuy
        ? await _relAmountNeededForArrrReceive(walletId, receiveAmount)
        : await _arrrAmountNeededForRelReceive(walletId, receiveAmount);
    return _quoteFromPayAmount(walletId, payAmount);
  }

  Future<Decimal> _relAmountNeededForArrrReceive(
    String walletId,
    Decimal targetArrrAfterFee,
  ) async {
    final feeMultiplier = Decimal.one - totalTakerFeeRate;
    if (feeMultiplier <= Decimal.zero) {
      throw const SwapOrchestratorException('Invalid ARRR swap fee.');
    }

    final grossArrrTarget = (targetArrrAfterFee / feeMultiplier).toDecimal(
      scaleOnInfinitePrecision: 12,
    );
    final book = await _orchestrator.loadOrderbook(walletId, pair: state.pair);
    final asks =
        book.asks
            .where(
              (level) =>
                  level.priceRelPerArrr > Decimal.zero &&
                  level.arrrAmount > Decimal.zero,
            )
            .toList()
          ..sort((a, b) => a.priceRelPerArrr.compareTo(b.priceRelPerArrr));

    if (asks.isEmpty) {
      throw SwapOrchestratorException(
        '${state.pair.displayName} orderbook is empty.',
      );
    }

    final maxPrice =
        asks.first.priceRelPerArrr * (Decimal.one + _slippageCap());
    var remainingArrr = grossArrrTarget;
    var requiredRel = Decimal.zero;

    for (final ask in asks) {
      if (ask.priceRelPerArrr > maxPrice) break;
      if (remainingArrr <= Decimal.zero) break;
      final takeArrr = _minDecimal(remainingArrr, ask.arrrAmount);
      requiredRel += takeArrr * ask.priceRelPerArrr;
      remainingArrr -= takeArrr;
    }

    if (_hasMeaningfulRemainder(remainingArrr)) {
      throw SwapOrchestratorException(
        'Not enough ARRR liquidity is available within ${state.slippagePercent.toStringAsFixed(1)}% slippage for that receive amount.',
      );
    }

    return requiredRel;
  }

  Future<Decimal> _arrrAmountNeededForRelReceive(
    String walletId,
    Decimal targetRelAmount,
  ) async {
    final book = await _orchestrator.loadOrderbook(walletId, pair: state.pair);
    final bids =
        book.bids
            .where(
              (level) =>
                  level.priceRelPerArrr > Decimal.zero &&
                  level.arrrAmount > Decimal.zero,
            )
            .toList()
          ..sort((a, b) => b.priceRelPerArrr.compareTo(a.priceRelPerArrr));

    if (bids.isEmpty) {
      throw SwapOrchestratorException(
        '${state.pair.displayName} orderbook is empty.',
      );
    }

    final minPrice =
        bids.first.priceRelPerArrr * (Decimal.one - _slippageCap());
    var remainingRel = targetRelAmount;
    var requiredNetArrr = Decimal.zero;

    for (final bid in bids) {
      if (bid.priceRelPerArrr < minPrice) break;
      if (remainingRel <= Decimal.zero) break;
      final levelRel = bid.arrrAmount * bid.priceRelPerArrr;
      final takeRel = _minDecimal(remainingRel, levelRel);
      final takeArrr = takeRel == levelRel
          ? bid.arrrAmount
          : (takeRel / bid.priceRelPerArrr).toDecimal(
              scaleOnInfinitePrecision: 12,
            );
      requiredNetArrr += takeArrr;
      remainingRel -= takeRel;
    }

    if (_hasMeaningfulRemainder(remainingRel)) {
      throw SwapOrchestratorException(
        'Not enough ${state.pair.relTicker} liquidity is available within ${state.slippagePercent.toStringAsFixed(1)}% slippage for that receive amount.',
      );
    }

    final feeMultiplier = Decimal.one - totalTakerFeeRate;
    if (feeMultiplier <= Decimal.zero) {
      throw const SwapOrchestratorException('Invalid ARRR swap fee.');
    }
    return (requiredNetArrr / feeMultiplier).toDecimal(
      scaleOnInfinitePrecision: 12,
    );
  }

  Decimal _minDecimal(Decimal a, Decimal b) => a <= b ? a : b;

  bool _hasMeaningfulRemainder(Decimal value) {
    return value > Decimal.parse('0.00000001');
  }

  void _recomputeLimitEstimate() {
    final amountText = state.payAmountText.trim();
    final priceText = state.limitPriceText.trim();

    Decimal? amount;
    Decimal? price;
    try {
      if (amountText.isNotEmpty) amount = _parseDecimal(amountText);
    } catch (_) {
      amount = null;
    }
    try {
      if (priceText.isNotEmpty) price = _parseDecimal(priceText);
    } catch (_) {
      price = null;
    }

    final priceError =
        priceText.isNotEmpty && (price == null || price <= Decimal.zero)
        ? 'Enter a valid price.'
        : null;

    if (amount == null ||
        amount <= Decimal.zero ||
        price == null ||
        price <= Decimal.zero) {
      state = state.copyWith(
        isQuoting: false,
        clearQuote: true,
        clearCapNotice: true,
        receiveAmountText: '',
        limitPriceError: priceError,
        clearLimitPriceError: priceError == null,
      );
      return;
    }

    final receive = state.isBuy
        ? (amount / price).toDecimal(scaleOnInfinitePrecision: 8)
        : (amount * price);
    state = state.copyWith(
      isQuoting: false,
      clearQuote: true,
      clearCapNotice: true,
      clearErrors: true,
      clearLimitPriceError: true,
      receiveAmountText: formatSwapAmount(receive),
    );
  }

  /// Whether the current limit inputs are complete enough to place an order.
  bool get hasValidLimitInputs {
    if (!_limit) return false;
    try {
      final amount = _parseDecimal(state.payAmountText);
      final price = _parseDecimal(state.limitPriceText);
      return amount > Decimal.zero && price > Decimal.zero;
    } catch (_) {
      return false;
    }
  }

  Decimal _slippageCap() =>
      Decimal.parse((state.slippagePercent / 100).toStringAsFixed(4));

  Decimal _parseDecimal(String value) => Decimal.parse(value.trim());

  Future<SignedTx> _signPending(PendingTx pending) async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      throw const SwapOrchestratorException('No active wallet.');
    }
    return FfiBridge.signTx(walletId, pending);
  }

  void _onProgress(SwapProgress progress) {
    state = state.copyWith(
      progress: progress.stage,
      progressMessage: progress.message,
    );
  }
}

final swapViewModelProvider =
    NotifierProvider.autoDispose<SwapViewModel, SwapViewModelState>(
      SwapViewModel.new,
    );

final swapUsesAdvancedUiProvider = Provider<bool>((ref) {
  final override = ref.watch(
    swapViewModelProvider.select((state) => state.useAdvancedOverride),
  );
  if (override != null) return override;
  return ref.watch(swapInterfacePreferenceProvider) ==
      SwapInterfacePreference.advanced;
});

final swapWalletBalanceProvider = Provider<String?>((ref) {
  final balance = ref.watch(balanceStreamProvider).asData?.value;
  if (balance == null) return null;
  return formatSwapAmount(
    arrrFromZatoshis(balance.spendable),
    fractionDigits: 4,
  );
});
