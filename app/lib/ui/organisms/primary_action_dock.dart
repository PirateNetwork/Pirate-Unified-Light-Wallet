import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';

class PrimaryActionDock extends StatelessWidget {
  const PrimaryActionDock({
    required this.child,
    this.padding,
    super.key,
  });

  final Widget child;
  final EdgeInsets? padding;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        border: Border(top: BorderSide(color: AppColors.borderSubtle)),
        boxShadow: [
          BoxShadow(
            color: AppColors.shadow,
            blurRadius: 12,
            offset: const Offset(0, -6),
          ),
        ],
      ),
      child: SafeArea(
        top: false,
        child: Padding(
          padding: padding ??
              const EdgeInsets.symmetric(
                horizontal: PSpacing.lg,
                vertical: PSpacing.lg,
              ),
          child: child,
        ),
      ),
    );
  }
}
