import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Hero Header
class PHeroHeader extends StatelessWidget {
  const PHeroHeader({
    required this.title,
    this.subtitle,
    this.actions,
    this.gradient = true,
    this.gradientStart,
    this.gradientEnd,
    super.key,
  });

  final String title;
  final String? subtitle;
  final List<Widget>? actions;
  final bool gradient;
  final Color? gradientStart;
  final Color? gradientEnd;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: EdgeInsets.all(PSpacing.xl),
      decoration: gradient
          ? BoxDecoration(
              gradient: LinearGradient(
                colors: [
                  (gradientStart ?? AppColors.gradientAStart).withValues(
                    alpha: 0.1,
                  ),
                  (gradientEnd ?? AppColors.gradientAEnd).withValues(
                    alpha: 0.05,
                  ),
                ],
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
              ),
              border: Border(
                bottom: BorderSide(color: AppColors.borderSubtle, width: 1.0),
              ),
            )
          : BoxDecoration(
              color: AppColors.backgroundSurface,
              border: Border(
                bottom: BorderSide(color: AppColors.borderSubtle, width: 1.0),
              ),
            ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: PTypography.displaySmall(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    if (subtitle != null) ...[
                      SizedBox(height: PSpacing.sm),
                      Text(
                        subtitle!,
                        style: PTypography.bodyLarge(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (actions != null && actions!.isNotEmpty) ...[
                SizedBox(width: PSpacing.lg),
                Row(
                  children: actions!.map((action) {
                    final index = actions!.indexOf(action);
                    return Row(
                      children: [
                        if (index > 0) SizedBox(width: PSpacing.sm),
                        action,
                      ],
                    );
                  }).toList(),
                ),
              ],
            ],
          ),
        ],
      ),
    );
  }
}
