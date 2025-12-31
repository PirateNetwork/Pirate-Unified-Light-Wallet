import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';

/// Pirate Wallet Card
class PCard extends StatefulWidget {
  const PCard({
    required this.child,
    this.padding,
    this.onTap,
    this.elevated = false,
    this.backgroundColor,
    super.key,
  });

  final Widget child;
  final EdgeInsets? padding;
  final VoidCallback? onTap;
  final bool elevated;
  final Color? backgroundColor;

  @override
  State<PCard> createState() => _PCardState();
}

class _PCardState extends State<PCard> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      cursor: widget.onTap != null ? SystemMouseCursors.click : SystemMouseCursors.basic,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        decoration: BoxDecoration(
          color: widget.backgroundColor ?? (widget.elevated ? AppColors.backgroundElevated : AppColors.backgroundSurface),
          borderRadius: BorderRadius.circular(PSpacing.radiusCard),
          border: Border.all(
            color: _isHovered && widget.onTap != null
                ? AppColors.borderStrong
                : AppColors.borderSubtle,
            width: 1.0,
          ),
          boxShadow: _isHovered && widget.onTap != null
              ? [
                  BoxShadow(
                    color: AppColors.shadow,
                    blurRadius: 16.0,
                    offset: const Offset(0, 8),
                  ),
                ]
              : null,
        ),
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: widget.onTap,
            borderRadius: BorderRadius.circular(PSpacing.radiusCard),
            splashColor: AppColors.pressedOverlay,
            highlightColor: AppColors.hoverOverlay,
            child: Padding(
              padding: widget.padding ?? EdgeInsets.all(PSpacing.cardPadding),
              child: widget.child,
            ),
          ),
        ),
      ),
    );
  }
}
