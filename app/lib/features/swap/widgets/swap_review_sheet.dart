import 'package:decimal/decimal.dart';
import 'package:flutter/material.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/swap_amount_utils.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';

class SwapReviewSheet extends StatelessWidget {
  const SwapReviewSheet({
    required this.side,
    required this.pair,
    required this.payAmount,
    required this.receiveAmount,
    required this.rateLabel,
    required this.feeLabel,
    super.key,
  });

  final SwapSide side;
  final SwapPair pair;
  final String payAmount;
  final String receiveAmount;
  final String? rateLabel;
  final String? feeLabel;

  bool get isBuy => side == SwapSide.buyArrr;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.all(PSpacing.sm),
                decoration: BoxDecoration(
                  color: AppColors.accentPrimary.withValues(alpha: 0.1),
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  Icons.receipt_long_outlined,
                  color: AppColors.accentPrimary,
                ),
              ),
              const SizedBox(width: PSpacing.md),
              Text(
                'Review swap'.tr,
                style: PTypography.heading5(color: AppColors.textPrimary),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.xl),
          Container(
            padding: const EdgeInsets.all(PSpacing.md),
            decoration: BoxDecoration(
              color: AppColors.backgroundElevated,
              borderRadius: BorderRadius.circular(PSpacing.radiusLG),
              border: Border.all(color: AppColors.borderSubtle),
            ),
            child: Column(
              children: [
                _ReviewRow(
                  label: 'You pay'.tr,
                  value:
                      '${formatSwapAmount(_parse(payAmount))} ${isBuy ? pair.relTicker : 'ARRR'}',
                  valueColor: AppColors.textPrimary,
                  isBold: true,
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: PSpacing.sm),
                  child: Divider(color: AppColors.borderSubtle, height: 1),
                ),
                _ReviewRow(
                  label: 'You receive'.tr,
                  value:
                      '${formatSwapAmount(_parse(receiveAmount))} ${isBuy ? 'ARRR' : pair.relTicker}',
                  valueColor: AppColors.success,
                  isBold: true,
                ),
              ],
            ),
          ),
          if (rateLabel != null || feeLabel != null) ...[
            const SizedBox(height: PSpacing.lg),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.backgroundSurface,
                borderRadius: BorderRadius.circular(PSpacing.radiusLG),
                border: Border.all(color: AppColors.borderSubtle),
              ),
              child: Column(
                children: [
                  if (rateLabel != null)
                    _ReviewRow(label: 'Rate'.tr, value: rateLabel!),
                  if (rateLabel != null && feeLabel != null)
                    const SizedBox(height: PSpacing.sm),
                  if (feeLabel != null)
                    _ReviewRow(label: 'Estimated fees'.tr, value: feeLabel!),
                ],
              ),
            ),
          ],
          const SizedBox(height: PSpacing.xl),
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(
                Icons.shield_outlined,
                color: AppColors.textSecondary,
                size: 20,
              ),
              const SizedBox(width: PSpacing.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Non-custodial swap. Your keys stay on this device.'.tr,
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                    const SizedBox(height: PSpacing.xs),
                    Text(
                      (isBuy
                              ? 'Next: deposit ${pair.relTicker}. Once enough confirmed balance is detected, the swap starts automatically.'
                              : 'Next: ARRR is sent to the swap engine, then matched and settled through KDF.')
                          .tr,
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                    const SizedBox(height: PSpacing.xs),
                    Text(
                      'No fixed ETA: completion depends on blockchain confirmations, maker response, and network conditions.'
                          .tr,
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  static Decimal _parse(String value) {
    if (value.trim().isEmpty) return Decimal.zero;
    return Decimal.parse(value.trim());
  }
}

class _ReviewRow extends StatelessWidget {
  const _ReviewRow({
    required this.label,
    required this.value,
    this.valueColor,
    this.isBold = false,
  });

  final String label;
  final String value;
  final Color? valueColor;
  final bool isBold;

  @override
  Widget build(BuildContext context) {
    final resolvedValueColor = valueColor ?? AppColors.textPrimary;

    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Text(
            label,
            style: PTypography.bodyMedium(color: AppColors.textSecondary),
          ),
        ),
        Text(
          value,
          style: isBold
              ? PTypography.titleMedium(color: resolvedValueColor)
              : PTypography.bodyMedium(color: resolvedValueColor),
          textAlign: TextAlign.right,
        ),
      ],
    );
  }
}
