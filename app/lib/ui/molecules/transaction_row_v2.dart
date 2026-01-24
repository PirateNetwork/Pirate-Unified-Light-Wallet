import 'package:flutter/material.dart';
import 'package:timeago/timeago.dart' as timeago;

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import 'p_card.dart';

class TransactionRowV2 extends StatelessWidget {
  const TransactionRowV2({
    required this.isReceived,
    required this.isConfirmed,
    required this.amountText,
    required this.timestamp,
    this.memo,
    this.addressLabel,
    this.onTap,
    super.key,
  });

  final bool isReceived;
  final bool isConfirmed;
  final String amountText;
  final DateTime timestamp;
  final String? memo;
  final String? addressLabel;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final statusText = isConfirmed ? 'Confirmed' : 'Pending';
    final statusColor = isConfirmed ? AppColors.success : AppColors.warning;
    final directionLabel = isReceived ? 'Received' : 'Sent';
    final timeLabel = timeago.format(timestamp);

    return PCard(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.md),
        child: Row(
          children: [
            Container(
              width: 44,
              height: 44,
              decoration: BoxDecoration(
                color: (isReceived ? AppColors.success : AppColors.highlight)
                    .withValues(alpha: 0.12),
                shape: BoxShape.circle,
              ),
              child: Icon(
                isReceived ? Icons.south_west : Icons.north_east,
                color: isReceived ? AppColors.success : AppColors.highlight,
                size: 20,
                semanticLabel: directionLabel,
              ),
            ),
            const SizedBox(width: PSpacing.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Text(
                        directionLabel,
                        style: PTypography.titleMedium(
                          color: AppColors.textPrimary,
                        ),
                      ),
                      if (addressLabel != null) ...[
                        const SizedBox(width: PSpacing.xs),
                        _AddressLabel(text: addressLabel!),
                      ],
                    ],
                  ),
                  const SizedBox(height: PSpacing.xxs),
                  Row(
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
                      Text(
                        statusText,
                        style: PTypography.bodySmall(color: statusColor),
                      ),
                      const SizedBox(width: PSpacing.sm),
                      Text(
                        timeLabel,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                      if (memo != null && memo!.isNotEmpty) ...[
                        const SizedBox(width: PSpacing.sm),
                        const _MemoIndicator(),
                      ],
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(width: PSpacing.md),
            Text(
              amountText,
              style: PTypography.titleSmall(
                color: isReceived ? AppColors.success : AppColors.textPrimary,
              ),
            ),
          ],
        ),
      ),
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
        'Has memo',
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
