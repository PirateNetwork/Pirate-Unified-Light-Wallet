import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Tag (Chip)
class PTag extends StatelessWidget {
  const PTag({
    required this.label,
    this.onDelete,
    this.selected = false,
    this.avatar,
    super.key,
  });

  final String label;
  final VoidCallback? onDelete;
  final bool selected;
  final Widget? avatar;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: PSpacing.sm,
        vertical: PSpacing.xs,
      ),
      decoration: BoxDecoration(
        color: selected
            ? AppColors.selectedBackground
            : AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(
          color: selected ? AppColors.selectedBorder : AppColors.borderDefault,
          width: 1.0,
        ),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (avatar != null) ...[avatar!, SizedBox(width: PSpacing.xs)],
          Text(
            label,
            style: PTypography.labelSmall(
              color: selected ? AppColors.focusRing : AppColors.textPrimary,
            ),
          ),
          if (onDelete != null) ...[
            SizedBox(width: PSpacing.xs),
            GestureDetector(
              onTap: onDelete,
              child: Icon(
                Icons.close,
                size: PSpacing.iconSM,
                color: AppColors.textSecondary,
              ),
            ),
          ],
        ],
      ),
    );
  }
}
