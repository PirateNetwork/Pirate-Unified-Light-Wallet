import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Snackbar
class PSnack {
  PSnack._();

  static void show({
    required BuildContext context,
    required String message,
    PSnackVariant variant = PSnackVariant.neutral,
    Duration duration = const Duration(seconds: 4),
    String? actionLabel,
    VoidCallback? onAction,
  }) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Row(
          children: [
            if (variant.icon != null) ...[
              Icon(
                variant.icon,
                color: variant.iconColor,
                size: PSpacing.iconMD,
              ),
              SizedBox(width: PSpacing.sm),
            ],
            Expanded(
              child: Text(
                message,
                style: PTypography.bodyMedium(color: AppColors.textPrimary),
              ),
            ),
          ],
        ),
        duration: duration,
        backgroundColor: AppColors.backgroundElevated,
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          side: BorderSide(
            color: variant.borderColor,
            width: 1.0,
          ),
        ),
        action: actionLabel != null
            ? SnackBarAction(
                label: actionLabel,
                textColor: AppColors.focusRing,
                onPressed: onAction ?? () {},
              )
            : null,
      ),
    );
  }
}

enum PSnackVariant {
  neutral,
  success,
  warning,
  error,
  info;

  IconData? get icon {
    switch (this) {
      case PSnackVariant.success:
        return Icons.check_circle_outline;
      case PSnackVariant.warning:
        return Icons.warning_amber_outlined;
      case PSnackVariant.error:
        return Icons.error_outline;
      case PSnackVariant.info:
        return Icons.info_outline;
      default:
        return null;
    }
  }

  Color get iconColor {
    switch (this) {
      case PSnackVariant.success:
        return AppColors.success;
      case PSnackVariant.warning:
        return AppColors.warning;
      case PSnackVariant.error:
        return AppColors.error;
      case PSnackVariant.info:
        return AppColors.info;
      default:
        return AppColors.textSecondary;
    }
  }

  Color get borderColor {
    switch (this) {
      case PSnackVariant.success:
        return AppColors.successBorder;
      case PSnackVariant.warning:
        return AppColors.warningBorder;
      case PSnackVariant.error:
        return AppColors.errorBorder;
      case PSnackVariant.info:
        return AppColors.infoBorder;
      default:
        return AppColors.borderDefault;
    }
  }
}

