import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/i18n/arb_text_localizer.dart';
import '../../design/deep_space_theme.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import 'send_fee.dart';

Future<SendFeeSelection?> showSendFeeSelectorSheet({
  required BuildContext context,
  required SendFeeState feeState,
}) async {
  final inputFormatter = TextInputFormatter.withFunction((oldValue, newValue) {
    final text = newValue.text;
    if (text.isEmpty) return newValue;
    if (!RegExp(r'^\d+(\.\d{0,8})?$').hasMatch(text)) {
      return oldValue;
    }
    return newValue;
  });

  var pendingPreset = feeState.preset;
  var pendingFee = feeState.selectedFeeArrrtoshis;
  var pendingCustomFee = feeState.customFeeArrrtoshis;
  String? feeError;
  final controller = TextEditingController(
    text: feeArrrFromArrrtoshis(pendingFee).toStringAsFixed(8),
  );

  void syncController() {
    final text = feeArrrFromArrrtoshis(pendingFee).toStringAsFixed(8);
    controller.value = controller.value.copyWith(
      text: text,
      selection: TextSelection.collapsed(offset: text.length),
    );
  }

  void setPendingFee(int fee) {
    pendingFee = feeState.clampFee(fee);
    feeError = null;
    syncController();
  }

  String? validateCustomFee(String raw) {
    final parsed = double.tryParse(raw.trim());
    if (parsed == null) {
      return 'Enter a valid fee amount.'.tr;
    }

    final feeArrrtoshis = (parsed * 100000000).round();
    if (feeArrrtoshis < feeState.minFeeArrrtoshis ||
        feeArrrtoshis > feeState.maxFeeArrrtoshis) {
      return 'Fee must be between ${formatFeeArrrtoshis(feeState.minFeeArrrtoshis)} and ${formatFeeArrrtoshis(feeState.maxFeeArrrtoshis)}.'
          .tr;
    }

    return null;
  }

  void applyPreset(FeePreset preset, StateSetter setModalState) {
    pendingPreset = preset;
    if (preset == FeePreset.custom) {
      if (pendingCustomFee != null) {
        setPendingFee(pendingCustomFee!);
      }
    } else {
      setPendingFee(feeState.feeForPreset(preset));
    }
    setModalState(() {});
  }

  try {
    return await PBottomSheet.show<SendFeeSelection>(
      context: context,
      title: 'Network fee'.tr,
      content: StatefulBuilder(
        builder: (context, setModalState) {
          void onSliderChanged(double value) {
            pendingPreset = FeePreset.custom;
            setPendingFee(value.round());
            setModalState(() {});
          }

          void onCustomChanged(String value) {
            pendingPreset = FeePreset.custom;
            final error = validateCustomFee(value);
            if (error != null) {
              feeError = error;
              setModalState(() {});
              return;
            }
            pendingFee = (double.parse(value.trim()) * 100000000).round();
            feeError = null;
            setModalState(() {});
          }

          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Choose a fee speed'.tr, style: AppTypography.bodyMedium),
              const SizedBox(height: AppSpacing.sm),
              Wrap(
                spacing: AppSpacing.xs,
                runSpacing: AppSpacing.xs,
                children: FeePreset.values.map((preset) {
                  return _FeePresetChip(
                    label: preset.label,
                    isSelected: pendingPreset == preset,
                    onTap: () => applyPreset(preset, setModalState),
                  );
                }).toList(),
              ),
              const SizedBox(height: AppSpacing.md),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Selected'.tr,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: AppTypography.caption,
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Text(
                    formatFeeArrrtoshis(pendingFee),
                    style: AppTypography.mono.copyWith(fontSize: 12),
                  ),
                ],
              ),
              Slider(
                value: pendingFee.toDouble(),
                min: feeState.minFeeArrrtoshis.toDouble(),
                max: feeState.maxFeeArrrtoshis.toDouble(),
                divisions: 100,
                onChanged: onSliderChanged,
                activeColor: AppColors.accentPrimary,
                inactiveColor: AppColors.borderSubtle,
              ),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Low'.tr,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: AppTypography.caption,
                    ),
                  ),
                  Text('High'.tr, style: AppTypography.caption),
                ],
              ),
              const SizedBox(height: AppSpacing.md),
              PInput(
                label: 'Custom fee (ARRR)'.tr,
                controller: controller,
                hint: feeArrrFromArrrtoshis(
                  feeState.minFeeArrrtoshis,
                ).toStringAsFixed(8),
                errorText: feeError,
                keyboardType: const TextInputType.numberWithOptions(
                  decimal: true,
                ),
                inputFormatters: [inputFormatter],
                onChanged: onCustomChanged,
              ),
              const SizedBox(height: AppSpacing.xs),
              Text(
                'Min ${formatFeeArrrtoshis(feeState.minFeeArrrtoshis)} - Max ${formatFeeArrrtoshis(feeState.maxFeeArrrtoshis)}',
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
              const SizedBox(height: AppSpacing.sm),
              Text(
                'Pirate uses a fixed minimum fee. Higher fees may not speed up confirmations.'
                    .tr,
                style: AppTypography.bodySmall.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
              const SizedBox(height: AppSpacing.lg),
              Row(
                children: [
                  Expanded(
                    child: PButton(
                      text: 'Cancel',
                      variant: PButtonVariant.secondary,
                      onPressed: () => Navigator.of(context).pop(),
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: PButton(
                      text: 'Apply',
                      onPressed: () {
                        if (pendingPreset == FeePreset.custom) {
                          final error = validateCustomFee(controller.text);
                          if (error != null) {
                            setModalState(() => feeError = error);
                            return;
                          }
                          pendingFee =
                              (double.parse(controller.text.trim()) * 100000000)
                                  .round();
                          pendingCustomFee = pendingFee;
                        }

                        Navigator.of(context).pop(
                          SendFeeSelection(
                            preset: pendingPreset,
                            feeArrrtoshis: pendingFee,
                            customFeeArrrtoshis: pendingCustomFee,
                          ),
                        );
                      },
                    ),
                  ),
                ],
              ),
            ],
          );
        },
      ),
    );
  } finally {
    controller.dispose();
  }
}

class _FeePresetChip extends StatelessWidget {
  const _FeePresetChip({
    required this.label,
    required this.isSelected,
    required this.onTap,
  });

  final String label;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final background = isSelected
        ? AppColors.selectedBackground
        : AppColors.backgroundSurface;
    final border = isSelected
        ? AppColors.selectedBorder
        : AppColors.borderSubtle;
    final textColor = isSelected
        ? AppColors.textPrimary
        : AppColors.textSecondary;

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      child: Container(
        constraints: const BoxConstraints(minHeight: 44),
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpacing.md,
          vertical: AppSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(PSpacing.radiusFull),
          border: Border.all(color: border),
        ),
        child: Text(label, style: PTypography.labelSmall(color: textColor)),
      ),
    );
  }
}
