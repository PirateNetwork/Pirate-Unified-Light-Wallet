import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../atoms/p_icon_button.dart';

class BalanceHero extends StatelessWidget {
  const BalanceHero({
    required this.balanceText,
    this.label = 'Balance',
    this.secondaryText,
    this.helperText,
    this.compact = false,
    this.isHidden = false,
    this.onToggleVisibility,
    this.onSwapDisplay,
    super.key,
  });

  final String balanceText;
  final String label;
  final String? secondaryText;
  final String? helperText;
  final bool compact;
  final bool isHidden;
  final VoidCallback? onToggleVisibility;
  final VoidCallback? onSwapDisplay;

  @override
  Widget build(BuildContext context) {
    final displayText = isHidden ? '********' : balanceText;
    final cardPadding = compact ? PSpacing.md : PSpacing.lg;
    final titleStyle = compact
        ? PTypography.heading4(color: AppColors.textPrimary)
        : PTypography.displaySmall(color: AppColors.textPrimary);

    return Container(
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(PSpacing.radiusCard),
        border: Border.all(color: AppColors.borderStrong),
        gradient: LinearGradient(
          colors: [
            AppColors.gradientAStart.withValues(alpha: 0.08),
            AppColors.gradientBStart.withValues(alpha: 0.06),
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        boxShadow: [
          BoxShadow(
            color: AppColors.shadow,
            blurRadius: compact ? 8 : 12,
            offset: Offset(0, compact ? 4 : 6),
          ),
        ],
      ),
      padding: EdgeInsets.all(cardPadding),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Text(
                label,
                style: PTypography.labelLarge(color: AppColors.textSecondary),
              ),
              const Spacer(),
              if (onToggleVisibility != null)
                PIconButton(
                  icon: Icon(
                    isHidden ? Icons.visibility_off : Icons.visibility,
                    color: AppColors.textSecondary,
                  ),
                  onPressed: onToggleVisibility,
                  tooltip: isHidden ? 'Show balance' : 'Hide balance',
                  size: compact
                      ? PIconButtonSize.medium
                      : PIconButtonSize.large,
                ),
            ],
          ),
          SizedBox(height: compact ? PSpacing.xs : PSpacing.sm),
          AnimatedSwitcher(
            duration: const Duration(milliseconds: 120),
            child: FittedBox(
              key: ValueKey(displayText),
              fit: BoxFit.scaleDown,
              alignment: Alignment.centerLeft,
              child: Text(
                displayText,
                style: titleStyle.copyWith(
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ),
          ),
          if (!isHidden && secondaryText != null) ...[
            const SizedBox(height: PSpacing.xs),
            Row(
              children: [
                Expanded(
                  child: Text(
                    secondaryText!,
                    style: PTypography.labelSmall(
                      color: AppColors.textSecondary,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                if (onSwapDisplay != null)
                  PIconButton(
                    icon: Icon(Icons.swap_vert, color: AppColors.textSecondary),
                    onPressed: onSwapDisplay,
                    tooltip: 'Swap balance display',
                    size: compact
                        ? PIconButtonSize.medium
                        : PIconButtonSize.large,
                  ),
              ],
            ),
          ],
          if (helperText != null) ...[
            SizedBox(height: compact ? PSpacing.xs : PSpacing.sm),
            Text(
              helperText!,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ],
      ),
    );
  }
}
