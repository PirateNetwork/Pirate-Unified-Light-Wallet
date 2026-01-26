import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../atoms/p_button.dart';

/// Pirate Wallet Dialog
class PDialog extends StatelessWidget {
  const PDialog({
    super.key,
    required this.title,
    required this.content,
    this.actions,
    this.barrierDismissible = true,
  });

  final String title;
  final Widget content;
  final List<PDialogAction<dynamic>>? actions;
  final bool barrierDismissible;

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: AppColors.backgroundElevated,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(PSpacing.radiusXL),
      ),
      child: Container(
        constraints: const BoxConstraints(maxWidth: 500),
        padding: EdgeInsets.all(PSpacing.dialogPadding),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              title,
              style: PTypography.heading4(color: AppColors.textPrimary),
            ),
            SizedBox(height: PSpacing.md),
            DefaultTextStyle(
              style: PTypography.bodyMedium(color: AppColors.textSecondary),
              child: content,
            ),
            if (actions != null && actions!.isNotEmpty) ...[
              SizedBox(height: PSpacing.lg),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: actions!.map((action) {
                  final index = actions!.indexOf(action);
                  return Row(
                    children: [
                      if (index > 0) SizedBox(width: PSpacing.sm),
                      PButton(
                        onPressed: () {
                          action.onPressed?.call();
                          Navigator.of(context).pop(action.result);
                        },
                        variant: action.variant,
                        child: Text(action.label),
                      ),
                    ],
                  );
                }).toList(),
              ),
            ],
          ],
        ),
      ),
    );
  }

  static Future<T?> show<T>({
    required BuildContext context,
    required String title,
    required Widget content,
    List<PDialogAction<dynamic>>? actions,
    bool barrierDismissible = true,
  }) {
    return showDialog<T>(
      context: context,
      barrierDismissible: barrierDismissible,
      barrierColor: AppColors.backgroundOverlay,
      builder: (context) => PDialog(
        title: title,
        content: content,
        actions: actions,
        barrierDismissible: barrierDismissible,
      ),
    );
  }
}

class PDialogAction<T> {
  const PDialogAction({
    required this.label,
    this.onPressed,
    this.variant = PButtonVariant.primary,
    this.result,
  });

  final String label;
  final VoidCallback? onPressed;
  final PButtonVariant variant;
  final T? result;
}

