import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Bottom Sheet
class PBottomSheet {
  PBottomSheet._();

  static Future<T?> showAdaptive<T>({
    required BuildContext context,
    required WidgetBuilder builder,
    bool isDismissible = true,
    bool enableDrag = true,
    bool isScrollControlled = false,
    bool useSafeArea = false,
    Color? backgroundColor,
    Color? barrierColor,
    ShapeBorder? shape,
  }) {
    final useCupertinoSheet = Theme.of(context).platform == TargetPlatform.iOS;
    if (useCupertinoSheet) {
      return showCupertinoSheet<T>(
        context: context,
        enableDrag: enableDrag,
        showDragHandle: enableDrag,
        builder: (sheetContext) {
          var child = builder(sheetContext);
          if (useSafeArea) {
            child = SafeArea(top: false, child: child);
          }
          if (!isDismissible) {
            child = PopScope(canPop: false, child: child);
          }
          return Material(
            color: backgroundColor ?? Colors.transparent,
            child: child,
          );
        },
      );
    }

    return showModalBottomSheet<T>(
      context: context,
      isDismissible: isDismissible,
      enableDrag: enableDrag,
      isScrollControlled: isScrollControlled,
      useSafeArea: useSafeArea,
      backgroundColor: backgroundColor,
      barrierColor: barrierColor,
      shape: shape,
      builder: builder,
    );
  }

  static Future<T?> show<T>({
    required BuildContext context,
    required String title,
    required Widget content,
    bool isDismissible = true,
    bool enableDrag = true,
  }) {
    return showAdaptive<T>(
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
          bottom:
              MediaQuery.of(context).viewInsets.bottom +
              PSpacing.bottomSheetPadding,
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
