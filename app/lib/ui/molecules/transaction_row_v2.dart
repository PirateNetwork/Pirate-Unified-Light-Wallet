import 'package:flutter/material.dart';
import 'package:timeago/timeago.dart' as timeago;

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import 'p_card.dart';
import '../../core/i18n/arb_text_localizer.dart';

class TransactionRowV2 extends StatelessWidget {
  const TransactionRowV2({
    required this.isReceived,
    required this.isConfirmed,
    this.isExpired = false,
    required this.amountText,
    required this.timestamp,
    this.memo,
    this.addressLabel,
    this.onTap,
    super.key,
  });

  final bool isReceived;
  final bool isConfirmed;
  final bool isExpired;
  final String amountText;
  final DateTime timestamp;
  final String? memo;
  final String? addressLabel;
  final VoidCallback? onTap;

  static const Key semanticsKey = Key('transaction-row-semantics');
  static const Key directionKey = Key('transaction-row-direction');
  static const Key amountKey = Key('transaction-row-amount');
  static const Key metadataKey = Key('transaction-row-metadata');

  static const double _compactContentBreakpoint = 460;

  @override
  Widget build(BuildContext context) {
    final statusText = isExpired
        ? 'Expired'.tr
        : isConfirmed
        ? 'Confirmed'.tr
        : 'Pending'.tr;
    final statusColor = isExpired
        ? AppColors.error
        : isConfirmed
        ? AppColors.success
        : AppColors.warning;
    final directionLabel = isReceived ? 'Received'.tr : 'Sent'.tr;
    final timeLabel = timeago.format(timestamp);

    final semanticLabel = <String>[
      directionLabel,
      amountText,
      statusText,
      timeLabel,
      if (addressLabel != null && addressLabel!.isNotEmpty) addressLabel!,
      if (memo != null && memo!.isNotEmpty) 'Has memo'.tr,
    ].join(', ');

    return Semantics(
      key: semanticsKey,
      button: onTap != null,
      container: true,
      excludeSemantics: true,
      label: semanticLabel,
      onTap: onTap,
      child: PCard(
        onTap: onTap,
        padding: const EdgeInsets.all(PSpacing.md),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final textScale = MediaQuery.textScalerOf(context).scale(1);
            final useCompactLayout =
                constraints.maxWidth < _compactContentBreakpoint ||
                textScale > 1.3;

            final direction = _TransactionDirection(
              directionLabel: directionLabel,
              addressLabel: addressLabel,
            );
            final metadata = _TransactionMetadata(
              statusText: statusText,
              statusColor: statusColor,
              timeLabel: timeLabel,
              hasMemo: memo != null && memo!.isNotEmpty,
            );
            final amount = _TransactionAmount(
              amountText: amountText,
              color: isReceived ? AppColors.success : AppColors.textPrimary,
              compact: useCompactLayout,
            );

            return Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                _TransactionDirectionIcon(isReceived: isReceived),
                const SizedBox(width: PSpacing.sm),
                Expanded(
                  child: useCompactLayout
                      ? Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            direction,
                            const SizedBox(height: PSpacing.xs),
                            amount,
                            const SizedBox(height: PSpacing.xs),
                            metadata,
                          ],
                        )
                      : Row(
                          crossAxisAlignment: CrossAxisAlignment.center,
                          children: [
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  direction,
                                  const SizedBox(height: PSpacing.xxs),
                                  metadata,
                                ],
                              ),
                            ),
                            const SizedBox(width: PSpacing.lg),
                            Flexible(child: amount),
                          ],
                        ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

class _TransactionDirectionIcon extends StatelessWidget {
  const _TransactionDirectionIcon({required this.isReceived});

  final bool isReceived;

  @override
  Widget build(BuildContext context) {
    final color = isReceived ? AppColors.success : AppColors.highlight;

    return Container(
      width: 44,
      height: 44,
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        shape: BoxShape.circle,
      ),
      child: Icon(
        isReceived ? Icons.south_west : Icons.north_east,
        color: color,
        size: PSpacing.iconMD,
      ),
    );
  }
}

class _TransactionDirection extends StatelessWidget {
  const _TransactionDirection({
    required this.directionLabel,
    required this.addressLabel,
  });

  final String directionLabel;
  final String? addressLabel;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: PSpacing.xs,
      runSpacing: PSpacing.xxs,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Text(
          directionLabel,
          key: TransactionRowV2.directionKey,
          style: PTypography.titleMedium(color: AppColors.textPrimary),
        ),
        if (addressLabel != null && addressLabel!.isNotEmpty)
          _AddressLabel(text: addressLabel!),
      ],
    );
  }
}

class _TransactionAmount extends StatelessWidget {
  const _TransactionAmount({
    required this.amountText,
    required this.color,
    required this.compact,
  });

  final String amountText;
  final Color color;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final alignment = compact ? Alignment.centerLeft : Alignment.centerRight;

    return Align(
      alignment: alignment,
      child: FittedBox(
        fit: BoxFit.scaleDown,
        alignment: alignment,
        child: Text(
          amountText,
          key: TransactionRowV2.amountKey,
          maxLines: 1,
          softWrap: false,
          textAlign: compact ? TextAlign.start : TextAlign.end,
          style: PTypography.numberMedium(color: color),
        ),
      ),
    );
  }
}

class _TransactionMetadata extends StatelessWidget {
  const _TransactionMetadata({
    required this.statusText,
    required this.statusColor,
    required this.timeLabel,
    required this.hasMemo,
  });

  final String statusText;
  final Color statusColor;
  final String timeLabel;
  final bool hasMemo;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      key: TransactionRowV2.metadataKey,
      spacing: PSpacing.sm,
      runSpacing: PSpacing.xxs,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 6,
              height: 6,
              decoration: BoxDecoration(
                color: statusColor,
                shape: BoxShape.circle,
              ),
            ),
            const SizedBox(width: PSpacing.xs),
            Flexible(
              child: Text(
                statusText,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: PTypography.bodySmall(color: statusColor),
              ),
            ),
          ],
        ),
        Text(
          timeLabel,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: PTypography.bodySmall(color: AppColors.textSecondary),
        ),
        if (hasMemo) const _MemoIndicator(),
      ],
    );
  }
}

class _MemoIndicator extends StatelessWidget {
  const _MemoIndicator();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: PSpacing.xs,
        vertical: PSpacing.xxs,
      ),
      decoration: BoxDecoration(
        color: AppColors.selectedBackground,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(color: AppColors.selectedBorder),
      ),
      child: Text(
        'Has memo'.tr,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: PTypography.labelSmall(color: AppColors.textSecondary),
      ),
    );
  }
}

class _AddressLabel extends StatelessWidget {
  const _AddressLabel({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: PSpacing.xs,
        vertical: PSpacing.xxs,
      ),
      decoration: BoxDecoration(
        color: AppColors.selectedBackground,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(color: AppColors.selectedBorder),
      ),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 140),
        child: Text(
          text,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: PTypography.labelSmall(color: AppColors.focusRing),
        ),
      ),
    );
  }
}
