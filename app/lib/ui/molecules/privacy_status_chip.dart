import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

enum PrivacyStatus {
  private,
  limited,
  connecting,
  offline,
}

class PrivacyStatusChip extends StatelessWidget {
  const PrivacyStatusChip({
    required this.status,
    this.onTap,
    this.compact = false,
    super.key,
  });

  final PrivacyStatus status;
  final VoidCallback? onTap;
  final bool compact;

  String get _label {
    switch (status) {
      case PrivacyStatus.private:
        return 'Connected - Secure';
      case PrivacyStatus.limited:
        return 'Connected - Limited';
      case PrivacyStatus.connecting:
        return 'Connecting';
      case PrivacyStatus.offline:
        return 'Offline';
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
    final padding = compact
        ? const EdgeInsets.symmetric(horizontal: PSpacing.sm, vertical: PSpacing.xs)
        : const EdgeInsets.symmetric(horizontal: PSpacing.md, vertical: PSpacing.xs);

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
            const SizedBox(width: PSpacing.xs),
            Text(
              _label,
              style: PTypography.labelSmall(color: AppColors.textSecondary),
            ),
          ],
        ),
      ),
    );
  }
}
