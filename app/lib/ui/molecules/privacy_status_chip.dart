import 'package:flutter/material.dart';

import '../../core/i18n/arb_text_localizer.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

enum PrivacyStatus { private, limited, connecting, offline }

class PrivacyStatusChip extends StatelessWidget {
  const PrivacyStatusChip({
    required this.status,
    this.onTap,
    this.compact = false,
    this.dotOnly = false,
    super.key,
  });

  final PrivacyStatus status;
  final VoidCallback? onTap;
  final bool compact;
  final bool dotOnly;

  String get _label {
    switch (status) {
      case PrivacyStatus.private:
        return 'Connected - Secure'.tr;
      case PrivacyStatus.limited:
        return 'Connected - Limited'.tr;
      case PrivacyStatus.connecting:
        return 'Connecting'.tr;
      case PrivacyStatus.offline:
        return 'Offline'.tr;
    }
  }

  Color get _dotColor {
    switch (status) {
      case PrivacyStatus.private:
        return AppColors.success;
      case PrivacyStatus.limited:
        return AppColors.highlight;
      case PrivacyStatus.connecting:
        return AppColors.info;
      case PrivacyStatus.offline:
        return AppColors.error;
    }
  }

  @override
  Widget build(BuildContext context) {
    final padding = dotOnly
        ? const EdgeInsets.all(PSpacing.xs)
        : compact
        ? const EdgeInsets.symmetric(
            horizontal: PSpacing.sm,
            vertical: PSpacing.xs,
          )
        : const EdgeInsets.symmetric(
            horizontal: PSpacing.md,
            vertical: PSpacing.xs,
          );

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      child: Container(
        padding: padding,
        decoration: BoxDecoration(
          color: AppColors.backgroundSurface,
          borderRadius: BorderRadius.circular(PSpacing.radiusFull),
          border: Border.all(color: AppColors.borderSubtle),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(
                color: _dotColor,
                shape: BoxShape.circle,
              ),
            ),
            if (!dotOnly) ...[
              const SizedBox(width: PSpacing.xs),
              Text(
                _label,
                style: PTypography.labelSmall(color: AppColors.textSecondary),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
