import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Bottom Sheet
class PBottomSheet {
  PBottomSheet._();

  static Future<T?> show<T>({
    required BuildContext context,
    required String title,
    required Widget content,
    bool isDismissible = true,
    bool enableDrag = true,
  }) {
    return showModalBottomSheet<T>(
      context: context,
      isDismissible: isDismissible,
      enableDrag: enableDrag,
      isScrollControlled: true,
      backgroundColor: AppColors.backgroundElevated,
      barrierColor: AppColors.backgroundOverlay,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(
          top: Radius.circular(PSpacing.radiusXL),
        ),
      ),
      builder: (context) => Container(
        padding: EdgeInsets.only(
          left: PSpacing.bottomSheetPadding,
          right: PSpacing.bottomSheetPadding,
          top: PSpacing.md,
          bottom: MediaQuery.of(context).viewInsets.bottom + PSpacing.bottomSheetPadding,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Drag handle
            Center(
              child: Container(
                width: 40,
                height: 4,
                decoration: BoxDecoration(
                  color: AppColors.borderDefault,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
            ),
            SizedBox(height: PSpacing.md),
            // Title
            Text(
              title,
              style: PTypography.heading4(color: AppColors.textPrimary),
            ),
            SizedBox(height: PSpacing.md),
            // Content
            Flexible(
              child: SingleChildScrollView(
                child: DefaultTextStyle(
                  style: PTypography.bodyMedium(color: AppColors.textSecondary),
                  child: content,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

