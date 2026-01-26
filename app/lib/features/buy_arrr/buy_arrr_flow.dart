/// Buy ARRR flow state management
library;

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Buy ARRR flow steps
enum BuyArrrStep {
  selectCoin,
  enterAmount,
  quote,
  confirm,
  progress,
  receipt,
}

/// Source coin option
class SourceCoin {
  final String ticker;
  final String name;
  final String iconUrl;
  final double balance;

  const SourceCoin({
    required this.ticker,
    required this.name,
    required this.iconUrl,
    required this.balance,
  });
}

/// Quote for buying ARRR
class BuyQuote {
  final String sourceCoin;
  final String sourceAmount;
  final String arrrAmount;
  final String rate;
  final String fee;
  final String orderUuid;
  final DateTime validUntil;

  const BuyQuote({
    required this.sourceCoin,
    required this.sourceAmount,
    required this.arrrAmount,
    required this.rate,
    required this.fee,
    required this.orderUuid,
    required this.validUntil,
  });

  bool get isValid => DateTime.now().isBefore(validUntil);
  
  int get remainingSeconds => validUntil.difference(DateTime.now()).inSeconds;
}

/// Swap progress
class SwapProgressData {
  final String uuid;
  final SwapStage stage;
  final String sourceCoin;
  final String sourceAmount;
  final String arrrAmount;
  final int progressPercent;
  final DateTime startedAt;
  final String? error;

  const SwapProgressData({
    required this.uuid,
    required this.stage,
    required this.sourceCoin,
    required this.sourceAmount,
    required this.arrrAmount,
    required this.progressPercent,
    required this.startedAt,
    this.error,
  });

  bool get isComplete => stage == SwapStage.completed;
  bool get isFailed => stage == SwapStage.failed;
  bool get isRefunded => stage == SwapStage.refunded;
  bool get isTerminal => isComplete || isFailed || isRefunded;
  
  Duration get elapsed => DateTime.now().difference(startedAt);
}

/// Swap stage
enum SwapStage {
  initiating,
  negotiating,
  sendingFee,
  waitingForMakerPayment,
  validatingMakerPayment,
  sendingTakerPayment,
  waitingForCompletion,
  completed,
  failed,
  refunded,
}

extension SwapStageExt on SwapStage {
  String get displayName {
    switch (this) {
      case SwapStage.initiating:
        return 'Initiating Swap';
      case SwapStage.negotiating:
        return 'Negotiating';
      case SwapStage.sendingFee:
        return 'Sending Fee';
      case SwapStage.waitingForMakerPayment:
        return 'Waiting for Payment';
      case SwapStage.validatingMakerPayment:
        return 'Validating Payment';
      case SwapStage.sendingTakerPayment:
        return 'Sending Payment';
      case SwapStage.waitingForCompletion:
        return 'Completing Swap';
      case SwapStage.completed:
        return 'Completed';
      case SwapStage.failed:
        return 'Failed';
      case SwapStage.refunded:
        return 'Refunded';
    }
  }

  int get progressPercent {
    switch (this) {
      case SwapStage.initiating:
        return 0;
      case SwapStage.negotiating:
        return 10;
      case SwapStage.sendingFee:
        return 20;
      case SwapStage.waitingForMakerPayment:
        return 40;
      case SwapStage.validatingMakerPayment:
        return 60;
      case SwapStage.sendingTakerPayment:
        return 75;
      case SwapStage.waitingForCompletion:
        return 90;
      case SwapStage.completed:
        return 100;
      case SwapStage.failed:
        return 0;
      case SwapStage.refunded:
        return 0;
    }
  }
}

/// Buy ARRR flow state
class BuyArrrState {
  final BuyArrrStep currentStep;
  final SourceCoin? selectedCoin;
  final String? amount;
  final BuyQuote? quote;
  final SwapProgressData? swapProgress;
  final String? error;

  const BuyArrrState({
    this.currentStep = BuyArrrStep.selectCoin,
    this.selectedCoin,
    this.amount,
    this.quote,
    this.swapProgress,
    this.error,
  });

  BuyArrrState copyWith({
    BuyArrrStep? currentStep,
    SourceCoin? selectedCoin,
    String? amount,
    BuyQuote? quote,
    SwapProgressData? swapProgress,
    String? error,
  }) {
    return BuyArrrState(
      currentStep: currentStep ?? this.currentStep,
      selectedCoin: selectedCoin ?? this.selectedCoin,
      amount: amount ?? this.amount,
      quote: quote ?? this.quote,
      swapProgress: swapProgress ?? this.swapProgress,
      error: error ?? this.error,
    );
  }
}

/// Buy ARRR flow controller
class BuyArrrController extends Notifier<BuyArrrState> {
  @override
  BuyArrrState build() {
    return const BuyArrrState();
  }

  void selectCoin(SourceCoin coin) {
    state = state.copyWith(
      selectedCoin: coin,
      currentStep: BuyArrrStep.enterAmount,
    );
  }

  void setAmount(String amount) {
    state = state.copyWith(amount: amount);
  }

  Future<void> getQuote() async {
    if (state.selectedCoin == null || state.amount == null) {
      return;
    }

    // TODO(pirate): Call FFI to get quote from MM2.
    await Future<void>.delayed(const Duration(seconds: 1));

    final quote = BuyQuote(
      sourceCoin: state.selectedCoin!.ticker,
      sourceAmount: state.amount!,
      arrrAmount: '100.0',
      rate: '0.01',
      fee: '0.001',
      orderUuid: 'test-uuid',
      validUntil: DateTime.now().add(const Duration(seconds: 60)),
    );

    state = state.copyWith(
      quote: quote,
      currentStep: BuyArrrStep.quote,
    );
  }

  Future<void> confirmSwap() async {
    if (state.quote == null) {
      return;
    }

    state = state.copyWith(currentStep: BuyArrrStep.confirm);
  }

  Future<void> executeSwap() async {
    if (state.quote == null) {
      return;
    }

    // TODO(pirate): Call FFI to execute swap via MM2.
    await Future<void>.delayed(const Duration(milliseconds: 500));

    final swapProgress = SwapProgressData(
      uuid: 'swap-uuid',
      stage: SwapStage.initiating,
      sourceCoin: state.quote!.sourceCoin,
      sourceAmount: state.quote!.sourceAmount,
      arrrAmount: state.quote!.arrrAmount,
      progressPercent: 0,
      startedAt: DateTime.now(),
    );

    state = state.copyWith(
      swapProgress: swapProgress,
      currentStep: BuyArrrStep.progress,
    );

    // Start monitoring swap progress
    unawaited(_monitorSwapProgress());
  }

  Future<void> _monitorSwapProgress() async {
    // TODO(pirate): Poll MM2 for swap status updates.
    // For now, simulate progress
    await Future<void>.delayed(const Duration(seconds: 2));
    
    if (state.swapProgress != null) {
      state = state.copyWith(
        swapProgress: SwapProgressData(
          uuid: state.swapProgress!.uuid,
          stage: SwapStage.negotiating,
          sourceCoin: state.swapProgress!.sourceCoin,
          sourceAmount: state.swapProgress!.sourceAmount,
          arrrAmount: state.swapProgress!.arrrAmount,
          progressPercent: 10,
          startedAt: state.swapProgress!.startedAt,
        ),
      );
    }
  }

  void reset() {
    state = const BuyArrrState();
  }
}

/// Provider for buy ARRR controller
final buyArrrControllerProvider =
    NotifierProvider<BuyArrrController, BuyArrrState>(BuyArrrController.new);
