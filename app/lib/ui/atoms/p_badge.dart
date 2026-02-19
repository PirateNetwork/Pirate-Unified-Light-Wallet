import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Badge
class PBadge extends StatelessWidget {
  const PBadge({
    required this.label,
    this.variant = PBadgeVariant.neutral,
    this.size = PBadgeSize.medium,
    super.key,
  });

  final String label;
  final PBadgeVariant variant;
  final PBadgeSize size;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: size.padding,
      decoration: BoxDecoration(
        color: variant.backgroundColor,
        borderRadius: BorderRadius.circular(PSpacing.radiusFull),
        border: Border.all(color: variant.borderColor, width: 1.0),
      ),
      child: Text(
        label.toUpperCase(),
        style: size.textStyle.copyWith(color: variant.textColor),
      ),
    );
  }
}

enum PBadgeVariant {
  neutral,
  success,
  warning,
  error,
  info;

  Color get backgroundColor {
    switch (this) {
      case PBadgeVariant.neutral:
        return AppColors.backgroundSurface;
      case PBadgeVariant.success:
        return AppColors.successBackground;
      case PBadgeVariant.warning:
        return AppColors.warningBackground;
      case PBadgeVariant.error:
        return AppColors.errorBackground;
      case PBadgeVariant.info:
        return AppColors.infoBackground;
    }
  }

  Color get borderColor {
    switch (this) {
      case PBadgeVariant.neutral:
        return AppColors.borderDefault;
      case PBadgeVariant.success:
        return AppColors.successBorder;
      case PBadgeVariant.warning:
        return AppColors.warningBorder;
      case PBadgeVariant.error:
        return AppColors.errorBorder;
      case PBadgeVariant.info:
        return AppColors.infoBorder;
    }
  }

  Color get textColor {
    switch (this) {
      case PBadgeVariant.neutral:
        return AppColors.textSecondary;
      case PBadgeVariant.success:
        return AppColors.success;
      case PBadgeVariant.warning:
        return AppColors.warning;
      case PBadgeVariant.error:
        return AppColors.error;
      case PBadgeVariant.info:
        return AppColors.info;
    }
  }
}

enum PBadgeSize {
  small,
  medium,
  large;

  EdgeInsets get padding {
    switch (this) {
      case PBadgeSize.small:
        return EdgeInsets.symmetric(
          horizontal: PSpacing.xs,
          vertical: PSpacing.xxs,
        );
      case PBadgeSize.medium:
        return EdgeInsets.symmetric(
          horizontal: PSpacing.sm,
          vertical: PSpacing.xxs,
        );
      case PBadgeSize.large:
        return EdgeInsets.symmetric(
          horizontal: PSpacing.md,
          vertical: PSpacing.xs,
        );
    }
  }

  TextStyle get textStyle {
    switch (this) {
      case PBadgeSize.small:
        return PTypography.overline();
      case PBadgeSize.medium:
        return PTypography.labelSmall();
      case PBadgeSize.large:
        return PTypography.labelMedium();
    }
  }
}
