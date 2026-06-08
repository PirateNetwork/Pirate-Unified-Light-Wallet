import 'package:flutter/material.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_card.dart';

class SwapProgressView extends StatelessWidget {
  const SwapProgressView({
    required this.stage,
    required this.message,
    required this.isError,
    required this.onDone,
    this.onRetry,
    this.isLimitOrder = false,
    this.side = SwapSide.buyArrr,
    this.pair = SwapPair.arrrLtc,
    this.payAmount,
    this.receiveAmount,
    super.key,
  });

  final SwapProgressStage? stage;
  final String? message;
  final bool isError;
  final VoidCallback onDone;
  final VoidCallback? onRetry;
  final bool isLimitOrder;
  final SwapSide side;
  final SwapPair pair;
  final String? payAmount;
  final String? receiveAmount;

  bool get _isBuy => side == SwapSide.buyArrr;

  @override
  Widget build(BuildContext context) {
    final isComplete =
        stage == SwapProgressStage.complete ||
        (isLimitOrder && stage == SwapProgressStage.placingLimitRemainder);
    final steps = _stepsForStage(stage, isError, isComplete);

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.all(PSpacing.sm),
                decoration: BoxDecoration(
                  color: isError
                      ? AppColors.error.withValues(alpha: 0.1)
                      : isComplete
                      ? AppColors.success.withValues(alpha: 0.1)
                      : AppColors.accentPrimary.withValues(alpha: 0.1),
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  isError
                      ? Icons.error_outline
                      : isComplete
                      ? Icons.check_circle_outline
                      : Icons.sync_rounded,
                  color: isError
                      ? AppColors.error
                      : isComplete
                      ? AppColors.success
                      : AppColors.accentPrimary,
                ),
              ),
              const SizedBox(width: PSpacing.md),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      isError
                          ? 'Swap failed'.tr
                          : isComplete
                          ? (isLimitOrder
                                ? 'Limit order placed'.tr
                                : 'Swap complete'.tr)
                          : 'Swap in progress'.tr,
                      style: PTypography.heading5(
                        color: isError
                            ? AppColors.error
                            : isComplete
                            ? AppColors.success
                            : AppColors.textPrimary,
                      ),
                    ),
                    if (message != null) ...[
                      const SizedBox(height: PSpacing.xs),
                      Text(
                        message!,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.xl),
          Container(
            padding: const EdgeInsets.all(PSpacing.lg),
            decoration: BoxDecoration(
              color: AppColors.backgroundElevated,
              borderRadius: BorderRadius.circular(PSpacing.radiusLG),
              border: Border.all(color: AppColors.borderSubtle),
            ),
            child: Column(
              children: List.generate(steps.length, (index) {
                final step = steps[index];
                final isLast = index == steps.length - 1;
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Column(
                      children: [
                        Container(
                          width: 24,
                          height: 24,
                          decoration: BoxDecoration(
                            shape: BoxShape.circle,
                            color: step.done
                                ? AppColors.success
                                : step.active
                                ? AppColors.accentPrimary
                                : AppColors.backgroundSurface,
                            border: Border.all(
                              color: step.done
                                  ? AppColors.success
                                  : step.active
                                  ? AppColors.accentPrimary
                                  : AppColors.borderStrong,
                              width: 2,
                            ),
                          ),
                          child: step.done
                              ? const Icon(
                                  Icons.check,
                                  size: 14,
                                  color: Colors.white,
                                )
                              : step.active && !isError
                              ? const Center(
                                  child: SizedBox(
                                    width: 8,
                                    height: 8,
                                    child: CircularProgressIndicator(
                                      strokeWidth: 2,
                                      color: Colors.white,
                                    ),
                                  ),
                                )
                              : null,
                        ),
                        if (!isLast)
                          Container(
                            width: 2,
                            height: 32,
                            color: step.done
                                ? AppColors.success
                                : AppColors.borderStrong,
                          ),
                      ],
                    ),
                    const SizedBox(width: PSpacing.md),
                    Expanded(
                      child: Padding(
                        padding: const EdgeInsets.only(top: 2),
                        child: Text(
                          step.label,
                          style:
                              PTypography.bodyMedium(
                                color: step.active || step.done
                                    ? AppColors.textPrimary
                                    : AppColors.textSecondary,
                              ).copyWith(
                                fontWeight: step.active
                                    ? FontWeight.bold
                                    : FontWeight.normal,
                              ),
                        ),
                      ),
                    ),
                  ],
                );
              }),
            ),
          ),
          if (!isComplete && !isError) ...[
            const SizedBox(height: PSpacing.lg),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.info.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                border: Border.all(
                  color: AppColors.info.withValues(alpha: 0.2),
                ),
              ),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(Icons.info_outline, size: 18, color: AppColors.info),
                  const SizedBox(width: PSpacing.sm),
                  Expanded(
                    child: Text(
                      'Atomic swaps do not have a fixed ETA. Keep the app online when possible; if interrupted, reopen Swap and resume from Recent swaps.'
                          .tr,
                      style: PTypography.bodySmall(color: AppColors.info),
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (isComplete && !isLimitOrder && _hasSummary) ...[
            const SizedBox(height: PSpacing.lg),
            _SummaryBox(
              payLabel:
                  '${'You paid'.tr} '
                  '$payAmount ${_isBuy ? pair.relTicker : 'ARRR'}',
              receiveLabel:
                  '${'You received'.tr} '
                  '$receiveAmount ${_isBuy ? 'ARRR' : pair.relTicker}',
            ),
          ],
          if (isComplete || isError) ...[
            const SizedBox(height: PSpacing.xl),
            if (isError && onRetry != null) ...[
              PButton(
                text: 'Try again'.tr,
                fullWidth: true,
                onPressed: onRetry,
              ),
              const SizedBox(height: PSpacing.sm),
              PButton(
                text: 'Done'.tr,
                variant: PButtonVariant.ghost,
                fullWidth: true,
                onPressed: onDone,
              ),
            ] else
              PButton(text: 'Done'.tr, fullWidth: true, onPressed: onDone),
          ],
        ],
      ),
    );
  }

  bool get _hasSummary =>
      (payAmount?.isNotEmpty ?? false) && (receiveAmount?.isNotEmpty ?? false);

  List<_ProgressStep> _stepsForStage(
    SwapProgressStage? stage,
    bool isError,
    bool isComplete,
  ) {
    const labels = [
      'Prepare swap',
      'Send deposit',
      'Match and settle',
      'Deliver funds',
    ];
    final index = switch (stage) {
      SwapProgressStage.startingKdf || SwapProgressStage.activatingCoins => 0,
      SwapProgressStage.waitingForDeposit => 1,
      SwapProgressStage.matchingMarketOrder ||
      SwapProgressStage.placingLimitRemainder => 2,
      SwapProgressStage.withdrawing || SwapProgressStage.complete => 3,
      SwapProgressStage.failed || null => isError ? 2 : 0,
    };
    return List.generate(labels.length, (i) {
      return _ProgressStep(
        label: labels[i].tr,
        active: i == index && !isComplete,
        done: i < index || isComplete,
      );
    });
  }
}

class _SummaryBox extends StatelessWidget {
  const _SummaryBox({required this.payLabel, required this.receiveLabel});

  final String payLabel;
  final String receiveLabel;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.backgroundElevated,
        borderRadius: BorderRadius.circular(PSpacing.radiusLG),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            payLabel,
            style: PTypography.bodyMedium(color: AppColors.textSecondary),
          ),
          const SizedBox(height: PSpacing.xs),
          Text(
            receiveLabel,
            style: PTypography.titleMedium(color: AppColors.success),
          ),
        ],
      ),
    );
  }
}

class _ProgressStep {
  const _ProgressStep({
    required this.label,
    required this.active,
    required this.done,
  });

  final String label;
  final bool active;
  final bool done;
}
