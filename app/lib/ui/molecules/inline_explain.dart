import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

enum InlineExplainTone { info, warning, success }

class InlineExplain extends StatelessWidget {
  const InlineExplain({
    required this.text,
    this.tone = InlineExplainTone.info,
    this.icon,
    super.key,
  });

  final String text;
  final InlineExplainTone tone;
  final IconData? icon;

  Color get _color {
    switch (tone) {
      case InlineExplainTone.info:
        return AppColors.info;
      case InlineExplainTone.warning:
        return AppColors.warning;
      case InlineExplainTone.success:
        return AppColors.success;
    }
  }

  @override
  Widget build(BuildContext context) {
    final color = _color;

    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: PSpacing.md,
        vertical: PSpacing.sm,
      ),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        border: Border.all(color: color.withValues(alpha: 0.24)),
      ),
      child: Row(
        children: [
          Icon(icon ?? Icons.info_outline, color: color, size: 18),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              text,
              style: PTypography.bodySmall(color: AppColors.textPrimary),
            ),
          ),
        ],
      ),
    );
  }
}
