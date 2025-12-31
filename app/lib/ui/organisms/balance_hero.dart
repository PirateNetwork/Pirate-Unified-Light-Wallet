import 'dart:ui';

import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../atoms/p_icon_button.dart';

class BalanceHero extends StatelessWidget {
  const BalanceHero({
    required this.balanceText,
    this.label = 'Balance',
    this.fiatText,
    this.helperText,
    this.isHidden = false,
    this.onToggleVisibility,
    super.key,
  });

  final String balanceText;
  final String label;
  final String? fiatText;
  final String? helperText;
  final bool isHidden;
  final VoidCallback? onToggleVisibility;

  @override
  Widget build(BuildContext context) {
    final displayText = isHidden ? '********' : balanceText;

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
            blurRadius: 12,
            offset: const Offset(0, 6),
          ),
        ],
      ),
      padding: const EdgeInsets.all(PSpacing.lg),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
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
                ),
            ],
          ),
          const SizedBox(height: PSpacing.sm),
          AnimatedSwitcher(
            duration: const Duration(milliseconds: 120),
            child: FittedBox(
              key: ValueKey(displayText),
              fit: BoxFit.scaleDown,
              alignment: Alignment.centerLeft,
              child: Text(
                displayText,
                style: PTypography.displaySmall(
                  color: AppColors.textPrimary,
                ).copyWith(
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ),
          ),
          if (!isHidden && fiatText != null) ...[
            const SizedBox(height: PSpacing.xs),
            Text(
              fiatText!,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
          if (helperText != null) ...[
            const SizedBox(height: PSpacing.sm),
            Text(
              helperText!,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ],
      ),
    );
  }
}
