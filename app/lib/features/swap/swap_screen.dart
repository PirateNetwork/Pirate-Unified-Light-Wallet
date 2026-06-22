import 'dart:async';

import 'package:decimal/decimal.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/i18n/arb_text_localizer.dart';
import '../../core/providers/price_providers.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/swaps/ltc_address.dart';
import '../../core/swaps/swap_amount_utils.dart';
import '../../core/swaps/swap_models.dart';
import '../../design/tokens/spacing.dart';
import '../../features/settings/providers/preferences_providers.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/watch_only_banner.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../ui/organisms/primary_action_dock.dart';
import 'swap_viewmodel.dart';
import 'widgets/swap_advanced_panel.dart';
import 'widgets/swap_compose_card.dart';
import 'widgets/swap_deposit_panel.dart';
import 'widgets/swap_funding_balance_card.dart';
import 'widgets/swap_open_orders.dart';
import 'widgets/swap_progress_view.dart';
import 'widgets/swap_review_sheet.dart';

class SwapScreen extends ConsumerStatefulWidget {
  const SwapScreen({super.key});

  @override
  ConsumerState<SwapScreen> createState() => _SwapScreenState();
}

class _SwapScreenState extends ConsumerState<SwapScreen> {
  static const _orderbookRefreshInterval = Duration(seconds: 15);
  static const _fundingBalanceRefreshInterval = Duration(seconds: 30);

  Timer? _orderbookRefreshTimer;
  Timer? _fundingBalanceRefreshTimer;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      unawaited(
        ref.read(swapViewModelProvider.notifier).refreshFundingBalances(),
      );
      _refreshOrderbookIfAdvanced();
      _orderbookRefreshTimer = Timer.periodic(
        _orderbookRefreshInterval,
        (_) => _refreshOrderbookIfAdvanced(),
      );
      _fundingBalanceRefreshTimer = Timer.periodic(
        _fundingBalanceRefreshInterval,
        (_) => unawaited(
          ref.read(swapViewModelProvider.notifier).refreshFundingBalances(),
        ),
      );
    });
  }

  @override
  void dispose() {
    _orderbookRefreshTimer?.cancel();
    _fundingBalanceRefreshTimer?.cancel();
    super.dispose();
  }

  void _refreshOrderbookIfAdvanced() {
    if (!mounted || !ref.read(swapUsesAdvancedUiProvider)) return;
    final state = ref.read(swapViewModelProvider);
    if (state.step != SwapUiStep.compose) return;
    unawaited(ref.read(swapViewModelProvider.notifier).refreshOrderbook());
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(swapViewModelProvider);
    final vm = ref.read(swapViewModelProvider.notifier);
    final isAdvanced = ref.watch(swapUsesAdvancedUiProvider);
    final walletBalance = ref.watch(swapWalletBalanceProvider);
    final walletMeta = ref.watch(activeWalletMetaProvider);
    final arrrUsdQuote = ref.watch(arrrUsdPriceQuoteProvider).asData?.value;
    final ltcUsdQuote = ref.watch(ltcUsdPriceQuoteProvider).asData?.value;
    final isWatchOnly = walletMeta?.watchOnly ?? false;
    final screenWidth = MediaQuery.of(context).size.width;
    final isMobile = PSpacing.isMobile(screenWidth);
    final relUsdPrice = _relUsdPrice(
      pair: state.pair,
      arrrUsdPrice: arrrUsdQuote?.pricePerArrr,
      ltcUsdPrice: ltcUsdQuote?.pricePerUnit,
    );

    final quote = state.quote;
    final rateLabel = quote == null
        ? null
        : '1 ARRR ≈ ${formatSwapAmount(quote.plan.referencePriceLtcPerArrr, fractionDigits: 6)} ${state.pair.relTicker}';
    final feeLabel = quote == null ? null : _feeLabel(quote, state);
    final averageArrrUsdPriceLabel = quote == null
        ? null
        : _averageArrrUsdPriceLabel(quote, relUsdPrice);

    return PScaffold(
      title: 'Swap'.tr,
      appBar: PAppBar(
        title: 'Swap'.tr,
        subtitle: isAdvanced
            ? 'Advanced ARRR Atomic Swaps'.tr
            : 'Simple ARRR Atomic Swaps'.tr,
        showBackButton: true,
        onBack: () {
          if (state.step == SwapUiStep.compose) {
            if (context.canPop()) context.pop();
            return;
          }
          vm.backToCompose();
        },
        actions: [
          ConnectionStatusIndicator(
            full: !isMobile,
            onTap: () => context.push('/settings/privacy-shield'),
          ),
          if (!isMobile) const WalletSwitcherButton(compact: true),
          PopupMenuButton<String>(
            onSelected: (value) async {
              if (value == 'toggle_mode') {
                final next = isAdvanced
                    ? SwapInterfacePreference.simple
                    : SwapInterfacePreference.advanced;
                await ref
                    .read(swapInterfacePreferenceProvider.notifier)
                    .setPreference(next);
                ref
                    .read(swapViewModelProvider.notifier)
                    .setAdvancedOverride(
                      value: next == SwapInterfacePreference.advanced,
                    );
              } else if (value == 'settings') {
                if (context.mounted) {
                  unawaited(context.push('/settings/swap-interface'));
                }
              }
            },
            itemBuilder: (context) => [
              PopupMenuItem(
                value: 'toggle_mode',
                child: Text(
                  isAdvanced
                      ? 'Switch to simple swap'.tr
                      : 'Switch to advanced trading'.tr,
                ),
              ),
              PopupMenuItem(
                value: 'settings',
                child: Text('Swap interface settings'.tr),
              ),
            ],
          ),
        ],
      ),
      body: isWatchOnly
          ? Center(
              child: Padding(
                padding: PSpacing.screenPadding(screenWidth),
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    const WatchOnlyBanner(),
                    const SizedBox(height: PSpacing.lg),
                    Text(
                      'Swaps require a spendable wallet.'.tr,
                      style: Theme.of(context).textTheme.bodyLarge,
                      textAlign: TextAlign.center,
                    ),
                  ],
                ),
              ),
            )
          : Column(
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    padding: EdgeInsets.symmetric(
                      horizontal: PSpacing.screenPadding(screenWidth).left,
                      vertical: PSpacing.xl,
                    ),
                    child: _buildBody(
                      state: state,
                      vm: vm,
                      isAdvanced: isAdvanced,
                      walletBalance: walletBalance,
                      rateLabel: rateLabel,
                      feeLabel: feeLabel,
                      relUsdPrice: relUsdPrice,
                      averageArrrUsdPriceLabel: averageArrrUsdPriceLabel,
                    ),
                  ),
                ),
                if (_showDock(state)) _buildDock(state, vm, isAdvanced),
              ],
            ),
    );
  }

  String _feeLabel(SwapQuote quote, SwapViewModelState state) {
    final parts = <String>[];
    final totalTakerFee = quote.plan.totalTakerFeeArrrAmount;
    if (totalTakerFee > Decimal.zero) {
      parts.add(
        '${formatSwapAmount(totalTakerFee, fractionDigits: 8)} '
        'ARRR ${'taker fee (0.5%)'.tr}',
      );
    }
    if (quote.fees.baseNetworkFee > Decimal.zero) {
      parts.add(
        '~${formatSwapAmount(quote.fees.baseNetworkFee, fractionDigits: 8)} '
        '${state.pair.baseTicker} ${'network fees'.tr}',
      );
    }
    if (quote.fees.relNetworkFee > Decimal.zero) {
      parts.add(
        '~${formatSwapAmount(quote.fees.relNetworkFee, fractionDigits: 8)} '
        '${state.pair.relTicker} ${'network fees'.tr}',
      );
    }
    if (quote.fees.withdrawalFee > Decimal.zero) {
      parts.add(
        '~${formatSwapAmount(quote.fees.withdrawalFee, fractionDigits: 8)} '
        '${'withdrawal fees'.tr}',
      );
    }
    return parts.isEmpty ? '0 fees'.tr : parts.join(' + ');
  }

  String? _averageArrrUsdPriceLabel(SwapQuote quote, double? relUsdPrice) {
    if (relUsdPrice == null || relUsdPrice <= 0) return null;
    final plan = quote.plan;
    final averageRelPerArrr = plan.effectiveMarketPriceRelPerArrr;
    if (averageRelPerArrr == null || averageRelPerArrr <= Decimal.zero) {
      return null;
    }
    final relPrice = double.tryParse(averageRelPerArrr.toString());
    if (relPrice == null || relPrice <= 0) return null;

    final averageUsd = relPrice * relUsdPrice;
    return '${ArrrPriceFormatter.formatCurrency(CurrencyPreference.usd, averageUsd, includeCode: false)} per ARRR';
  }

  double? _relUsdPrice({
    required SwapPair pair,
    required double? arrrUsdPrice,
    required double? ltcUsdPrice,
  }) {
    return switch (pair.relAsset) {
      SwapAsset.ltc => ltcUsdPrice,
      SwapAsset.varrr => arrrUsdPrice,
      SwapAsset.arrr => arrrUsdPrice,
    };
  }

  Widget _buildBody({
    required SwapViewModelState state,
    required SwapViewModel vm,
    required bool isAdvanced,
    required String? walletBalance,
    required String? rateLabel,
    required String? feeLabel,
    required double? relUsdPrice,
    required String? averageArrrUsdPriceLabel,
  }) {
    switch (state.step) {
      case SwapUiStep.deposit:
        return SwapDepositPanel(
          ltcAmount: state.payAmountText,
          assetTicker: state.pair.relTicker,
          depositAddress: state.activeIntent?.ltcDepositAddress,
          depositExpiresAt: state.activeIntent?.depositExpiresAt,
          progressMessage: state.progressMessage,
          isExecuting: state.isExecuting,
          depositDetected: state.depositDetected,
          onContinue: vm.checkDepositOrStartSwap,
          onCancel: () => _confirmCancelSwap(vm),
          onExpired: vm.expireActiveDeposit,
          onRecheck: vm.recheckDeposit,
        );
      case SwapUiStep.executing:
      case SwapUiStep.complete:
      case SwapUiStep.error:
        return SwapProgressView(
          stage: state.progress,
          message: state.executionError ?? state.progressMessage,
          isError: state.step == SwapUiStep.error,
          isLimitOrder: state.completionKind == SwapCompletionKind.limitOrder,
          side: state.side,
          pair: state.pair,
          payAmount: state.payAmountText,
          receiveAmount: state.receiveAmountText,
          onDone: vm.resetAfterComplete,
          onRetry: state.step == SwapUiStep.error ? vm.retryAfterError : null,
        );
      case SwapUiStep.review:
        return SwapReviewSheet(
          side: state.side,
          pair: state.pair,
          payAmount: state.payAmountText,
          receiveAmount: state.receiveAmountText,
          rateLabel: rateLabel,
          feeLabel: feeLabel,
        );
      case SwapUiStep.compose:
        final fundingBalanceCard = SwapFundingBalanceCard(
          ltcBalance: state.fundingLtcBalance,
          arrrBalance: state.fundingArrrBalance,
          varrrBalance: state.fundingVarrrBalance,
          isLoading: state.isLoadingFundingBalance,
          isActionPending: state.isFundingActionPending,
          error: state.fundingBalanceError,
          message: state.fundingActionMessage,
          onRefresh: () => unawaited(vm.refreshFundingBalances()),
          onGetDepositAddress: vm.getFundingDepositAddress,
          onWithdrawAsset: vm.withdrawFundingAsset,
          onSweepArrrToWallet: vm.withdrawFundingArrrToWallet,
        );
        if (isAdvanced) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _PairDropdown(state: state, onChanged: vm.setPair),
              const SizedBox(height: PSpacing.lg),
              fundingBalanceCard,
              const SizedBox(height: PSpacing.lg),
              SwapAdvancedPanel(
                state: state,
                walletBalance: walletBalance,
                relUsdPrice: relUsdPrice,
                averageArrrUsdPriceLabel: averageArrrUsdPriceLabel,
                onPayAmountChanged: vm.updatePayAmount,
                onLtcAddressChanged: vm.updateLtcPayoutAddress,
                onSlippageChanged: vm.updateSlippage,
                onLimitPriceChanged: vm.updateLimitPrice,
                onOrderTypeChanged: vm.setAdvancedOrderType,
                onSideChanged: vm.setSide,
                onOrderbookDepthSelected: vm.selectAdvancedOrderbookDepth,
                onMax: vm.useMaxSellAmount,
              ),
              const SizedBox(height: PSpacing.lg),
              SwapOpenOrdersPanel(
                intents: state.incompleteIntents,
                onCancel: (intent) => cancelSwapIntent(ref, intent),
                onResume: vm.resumeIntent,
              ),
              SwapHistoryPanel(
                intents: state.history,
                onResume: vm.resumeIntent,
              ),
            ],
          );
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            fundingBalanceCard,
            const SizedBox(height: PSpacing.lg),
            SwapComposeCard(
              pair: state.pair,
              side: state.side,
              payAmount: state.payAmountText,
              receiveAmount: state.receiveAmountText,
              isQuoting: state.isQuoting,
              quoteError: state.quoteError,
              capNotice: state.capNotice,
              walletBalance: walletBalance,
              ltcPayoutAddress: state.ltcPayoutAddress,
              slippagePercent: state.slippagePercent,
              onPayAmountChanged: vm.updatePayAmount,
              onReceiveAmountChanged: vm.updateReceiveAmount,
              onFlip: vm.flipSide,
              onLtcAddressChanged: vm.updateLtcPayoutAddress,
              onSlippageChanged: vm.updateSlippage,
              onMax: vm.useMaxSellAmount,
              onAssetSelected: vm.selectSimpleAsset,
              rateLabel: rateLabel,
              feeLabel: feeLabel,
              averageArrrUsdPriceLabel: averageArrrUsdPriceLabel,
            ),
            if (state.incompleteIntents.isNotEmpty) ...[
              const SizedBox(height: PSpacing.lg),
              SwapOpenOrdersPanel(
                intents: state.incompleteIntents,
                onCancel: (intent) => cancelSwapIntent(ref, intent),
                onResume: vm.resumeIntent,
              ),
            ],
            SwapHistoryPanel(intents: state.history, onResume: vm.resumeIntent),
          ],
        );
    }
  }

  bool _showDock(SwapViewModelState state) {
    return state.step == SwapUiStep.compose || state.step == SwapUiStep.review;
  }

  Widget _buildDock(
    SwapViewModelState state,
    SwapViewModel vm,
    bool isAdvanced,
  ) {
    final isLimit =
        isAdvanced && state.advancedOrderType == SwapAdvancedOrderType.limit;
    final addressOk = _destinationAddressOk(state);
    final canContinue = isLimit
        ? vm.hasValidLimitInputs && addressOk
        : (state.quote != null &&
              state.payAmountText.trim().isNotEmpty &&
              (!state.isBuy || state.receiveAmountText.isNotEmpty) &&
              addressOk);

    return PrimaryActionDock(
      child: state.step == SwapUiStep.review
          ? Row(
              children: [
                Expanded(
                  child: PButton(
                    text: 'Back'.tr,
                    variant: PButtonVariant.secondary,
                    fullWidth: true,
                    onPressed: vm.backToCompose,
                  ),
                ),
                const SizedBox(width: PSpacing.md),
                Expanded(
                  child: PButton(
                    text: 'Confirm swap'.tr,
                    fullWidth: true,
                    loading: state.isExecuting,
                    onPressed: state.isExecuting ? null : vm.confirmSwap,
                  ),
                ),
              ],
            )
          : PButton(
              text: isAdvanced
                  ? (isLimit ? 'Place limit order'.tr : 'Place order'.tr)
                  : 'Review swap'.tr,
              fullWidth: true,
              onPressed: canContinue
                  ? (isAdvanced
                        ? () => _confirmAndPlaceAdvanced(state, vm, isLimit)
                        : vm.goToReview)
                  : null,
            ),
    );
  }

  /// Advanced mode skips the review step, so we surface an explicit
  /// confirmation before an irreversible on-chain action (especially sells and
  /// resting limit orders).
  Future<void> _confirmAndPlaceAdvanced(
    SwapViewModelState state,
    SwapViewModel vm,
    bool isLimit,
  ) async {
    final isBuy = state.isBuy;
    final payAsset = state.payAsset.ticker;
    final receiveAsset = state.receiveAsset.ticker;

    final lines = <String>[
      '${'You pay'.tr}: ${state.payAmountText} $payAsset',
      if (state.receiveAmountText.isNotEmpty)
        '${isLimit ? 'You receive (at your price)'.tr : 'You receive (estimated)'.tr}: ${formatSwapAmountTextRounded(state.receiveAmountText)} $receiveAsset',
      if (isLimit)
        '${'Limit price'.tr}: ${state.limitPriceText} ${state.pair.relTicker}/ARRR',
      if (!isBuy) '${'To'.tr}: ${state.ltcPayoutAddress}',
    ];

    final detail = isLimit
        ? (isBuy
              ? "You'll deposit {ticker}. Once enough confirmed balance is detected, a limit order is placed on the book at your price."
                    .trArgs({'ticker': state.pair.relTicker})
              : 'Your ARRR will be sent to the swap engine and a limit order placed at your price. This is on-chain and cannot be undone.'
                    .tr)
        : (isBuy
              ? "You'll deposit {ticker}. Once enough confirmed balance is detected, the app starts matching and settling the market swap."
                    .trArgs({'ticker': state.pair.relTicker})
              : 'Your ARRR will be sent on-chain to the swap engine, then matched and settled at market. This cannot be undone.'
                    .tr);

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(
          isLimit
              ? 'Place limit order?'.tr
              : (isBuy ? 'Start buy?'.tr : 'Confirm sell?'.tr),
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final line in lines) ...[
              Text(line),
              const SizedBox(height: PSpacing.xs),
            ],
            const SizedBox(height: PSpacing.sm),
            Text(detail, style: Theme.of(context).textTheme.bodySmall),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text('Cancel'.tr),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(isLimit ? 'Place order'.tr : 'Confirm'.tr),
          ),
        ],
      ),
    );

    if (confirmed ?? false) {
      await vm.confirmSwap();
    }
  }

  bool _destinationAddressOk(SwapViewModelState state) {
    if (state.isBuy) return true;
    if (state.pair.relAsset == SwapAsset.ltc) {
      return isLikelyValidLtcAddress(state.ltcPayoutAddress);
    }
    return state.ltcPayoutAddress.trim().isNotEmpty;
  }

  /// Cancelling after funding may already have been sent: warn the user and point
  /// them to the funding balance for recovery instead of silently abandoning.
  Future<void> _confirmCancelSwap(SwapViewModel vm) async {
    final ticker = ref.read(swapViewModelProvider).pair.relTicker;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text('Cancel this swap?'.tr),
        content: Text(
          "If you already sent {ticker}, it won't be lost. It stays in your "
                  'swap funding balance. You can use it for another swap or '
                  'refund it from the funding card.'
              .trArgs({'ticker': ticker}),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text('Keep waiting'.tr),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text('Cancel swap'.tr),
          ),
        ],
      ),
    );
    if (confirmed ?? false) {
      vm.cancelSwap();
    }
  }
}

class _PairDropdown extends StatelessWidget {
  const _PairDropdown({required this.state, required this.onChanged});

  final SwapViewModelState state;
  final ValueChanged<SwapPair> onChanged;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.centerLeft,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(PSpacing.radiusLG),
          border: Border.all(
            color: Theme.of(context).colorScheme.outlineVariant,
          ),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: PSpacing.md),
          child: DropdownButtonHideUnderline(
            child: DropdownButton<SwapPair>(
              value: state.pair,
              borderRadius: BorderRadius.circular(PSpacing.radiusLG),
              items: [
                for (final pair in SwapPair.values)
                  DropdownMenuItem(value: pair, child: Text(pair.displayName)),
              ],
              onChanged: state.step == SwapUiStep.compose
                  ? (pair) {
                      if (pair != null) onChanged(pair);
                    }
                  : null,
            ),
          ),
        ),
      ),
    );
  }
}
