import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Form Section
class PFormSection extends StatelessWidget {
  const PFormSection({
    required this.title,
    required this.children,
    this.description,
    super.key,
  });

  final String title;
  final String? description;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          title,
          style: PTypography.heading5(color: AppColors.textPrimary),
        ),
        if (description != null) ...[
          SizedBox(height: PSpacing.xs),
          Text(
            description!,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
        ],
        SizedBox(height: PSpacing.md),
        ...children.map((child) {
          final index = children.indexOf(child);
          return Column(
            children: [
              child,
              if (index < children.length - 1) SizedBox(height: PSpacing.formFieldGap),
            ],
          );
        }),
      ],
    );
  }
}

