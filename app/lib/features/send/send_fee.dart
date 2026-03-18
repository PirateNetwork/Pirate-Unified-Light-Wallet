import 'package:flutter/foundation.dart';

/// Additional fee per extra output in arrrtoshis.
const int kAdditionalOutputFeeArrrtoshis = 5000;

enum FeePreset { low, standard, high, custom }

extension FeePresetLabel on FeePreset {
  String get label {
    switch (this) {
      case FeePreset.low:
        return 'Low';
      case FeePreset.standard:
        return 'Standard';
      case FeePreset.high:
        return 'High';
      case FeePreset.custom:
        return 'Custom';
    }
  }
}

@immutable
class SendFeeSelection {
  const SendFeeSelection({
    required this.preset,
    required this.feeArrrtoshis,
    required this.customFeeArrrtoshis,
  });

  final FeePreset preset;
  final int feeArrrtoshis;
  final int? customFeeArrrtoshis;
}

@immutable
class SendFeeState {
  const SendFeeState({
    this.selectedFeeArrrtoshis = 10000,
    this.defaultFeeArrrtoshis = 10000,
    this.baseMinFeeArrrtoshis = 10000,
    this.minFeeArrrtoshis = 10000,
    this.baseFeeArrrtoshis = 10000,
    this.maxFeeArrrtoshis = 1000000,
    this.preset = FeePreset.standard,
    this.customFeeArrrtoshis,
  });

  final int selectedFeeArrrtoshis;
  final int defaultFeeArrrtoshis;
  final int baseMinFeeArrrtoshis;
  final int minFeeArrrtoshis;
  final int baseFeeArrrtoshis;
  final int maxFeeArrrtoshis;
  final FeePreset preset;
  final int? customFeeArrrtoshis;

  double get selectedFeeArrr => feeArrrFromArrrtoshis(selectedFeeArrrtoshis);

  int feeForPreset(FeePreset preset) {
    switch (preset) {
      case FeePreset.low:
        return (baseFeeArrrtoshis * 0.5).round();
      case FeePreset.standard:
        return baseFeeArrrtoshis;
      case FeePreset.high:
        return baseFeeArrrtoshis * 2;
      case FeePreset.custom:
        return customFeeArrrtoshis ?? baseFeeArrrtoshis;
    }
  }

  int clampFee(int fee, {int? minFee, int? maxFee}) {
    final minValue = minFee ?? minFeeArrrtoshis;
    final maxValue = maxFee ?? maxFeeArrrtoshis;
    return fee.clamp(minValue, maxValue);
  }

  SendFeeState withFeeInfo({
    required int defaultFeeArrrtoshis,
    required int baseMinFeeArrrtoshis,
    required int maxFeeArrrtoshis,
  }) {
    return SendFeeState(
      selectedFeeArrrtoshis: selectedFeeArrrtoshis,
      defaultFeeArrrtoshis: defaultFeeArrrtoshis,
      baseMinFeeArrrtoshis: baseMinFeeArrrtoshis,
      minFeeArrrtoshis: minFeeArrrtoshis,
      baseFeeArrrtoshis: baseFeeArrrtoshis,
      maxFeeArrrtoshis: maxFeeArrrtoshis,
      preset: preset,
      customFeeArrrtoshis: customFeeArrrtoshis,
    );
  }

  SendFeeState applySelection(SendFeeSelection selection) {
    return SendFeeState(
      selectedFeeArrrtoshis: selection.feeArrrtoshis,
      defaultFeeArrrtoshis: defaultFeeArrrtoshis,
      baseMinFeeArrrtoshis: baseMinFeeArrrtoshis,
      minFeeArrrtoshis: minFeeArrrtoshis,
      baseFeeArrrtoshis: baseFeeArrrtoshis,
      maxFeeArrrtoshis: maxFeeArrrtoshis,
      preset: selection.preset,
      customFeeArrrtoshis: selection.customFeeArrrtoshis,
    );
  }

  SendFeeState withSelectedFeeArrrtoshis(int feeArrrtoshis) {
    return SendFeeState(
      selectedFeeArrrtoshis: feeArrrtoshis,
      defaultFeeArrrtoshis: defaultFeeArrrtoshis,
      baseMinFeeArrrtoshis: baseMinFeeArrrtoshis,
      minFeeArrrtoshis: minFeeArrrtoshis,
      baseFeeArrrtoshis: baseFeeArrrtoshis,
      maxFeeArrrtoshis: maxFeeArrrtoshis,
      preset: preset,
      customFeeArrrtoshis: customFeeArrrtoshis,
    );
  }

  SendFeeState recalculate({required int recipientCount}) {
    final minFee = _currentMinFeeArrrtoshis(recipientCount: recipientCount);
    final baseFee = _currentBaseFeeArrrtoshis(
      recipientCount: recipientCount,
      minFee: minFee,
    );
    var selectedFee = preset == FeePreset.custom
        ? (customFeeArrrtoshis ?? selectedFeeArrrtoshis)
        : feeForPresetWithBase(preset, baseFee);
    selectedFee = clampFee(
      selectedFee,
      minFee: minFee,
      maxFee: maxFeeArrrtoshis,
    );

    return SendFeeState(
      selectedFeeArrrtoshis: selectedFee,
      defaultFeeArrrtoshis: defaultFeeArrrtoshis,
      baseMinFeeArrrtoshis: baseMinFeeArrrtoshis,
      minFeeArrrtoshis: minFee,
      baseFeeArrrtoshis: baseFee,
      maxFeeArrrtoshis: maxFeeArrrtoshis,
      preset: preset,
      customFeeArrrtoshis: preset == FeePreset.custom
          ? selectedFee
          : customFeeArrrtoshis,
    );
  }

  int feeForPresetWithBase(FeePreset preset, int baseFee) {
    switch (preset) {
      case FeePreset.low:
        return (baseFee * 0.5).round();
      case FeePreset.standard:
        return baseFee;
      case FeePreset.high:
        return baseFee * 2;
      case FeePreset.custom:
        return customFeeArrrtoshis ?? baseFee;
    }
  }

  int _currentMinFeeArrrtoshis({required int recipientCount}) {
    final fee =
        baseMinFeeArrrtoshis +
        (_extraOutputCount(recipientCount) * kAdditionalOutputFeeArrrtoshis);
    return fee.clamp(baseMinFeeArrrtoshis, maxFeeArrrtoshis);
  }

  int _currentBaseFeeArrrtoshis({
    required int recipientCount,
    required int minFee,
  }) {
    final fee =
        defaultFeeArrrtoshis +
        (_extraOutputCount(recipientCount) * kAdditionalOutputFeeArrrtoshis);
    return fee.clamp(minFee, maxFeeArrrtoshis);
  }

  int _extraOutputCount(int recipientCount) {
    if (recipientCount <= 1) return 0;
    return recipientCount - 1;
  }
}

double feeArrrFromArrrtoshis(int feeArrrtoshis) {
  return feeArrrtoshis / 100000000.0;
}

String formatFeeArrrtoshis(int feeArrrtoshis) {
  return '${feeArrrFromArrrtoshis(feeArrrtoshis).toStringAsFixed(8)} ARRR';
}
