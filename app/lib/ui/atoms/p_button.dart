import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Button - Primary action button with gradient
class PButton extends StatefulWidget {
  const PButton({
    required this.onPressed,
    this.child,
    this.text,
    this.variant = PButtonVariant.primary,
    this.size = PButtonSize.medium,
    this.fullWidth = false,
    bool? loading,
    bool? isLoading,
    this.icon,
    super.key,
  })  : loading = isLoading ?? loading ?? false,
        assert(child != null || text != null, 'Provide child or text');

  final VoidCallback? onPressed;
  final Widget? child;
  final String? text;
  final PButtonVariant variant;
  final PButtonSize size;
  final bool fullWidth;
  final bool loading;
  final Widget? icon;

  @override
  State<PButton> createState() => _PButtonState();
}

class _PButtonState extends State<PButton> {
  bool _isHovered = false;
  bool _isPressed = false;

  @override
  Widget build(BuildContext context) {
    final isDisabled = widget.onPressed == null || widget.loading;
    final contentChild = widget.child ?? Text(widget.text ?? '');

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      cursor: isDisabled ? SystemMouseCursors.forbidden : SystemMouseCursors.click,
      child: GestureDetector(
        onTapDown: (_) => setState(() => _isPressed = true),
        onTapUp: (_) => setState(() => _isPressed = false),
        onTapCancel: () => setState(() => _isPressed = false),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          width: widget.fullWidth ? double.infinity : null,
          height: widget.size.height,
          decoration: BoxDecoration(
          gradient: widget.variant.gradient(
            isDisabled: isDisabled,
            isHovered: _isHovered,
            isPressed: _isPressed,
          ),
          border: widget.variant.border(isDisabled: isDisabled),
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
            boxShadow: _isHovered && !isDisabled
                ? [
                    BoxShadow(
                      color: AppColors.shadow,
                      blurRadius: 8.0,
                      offset: const Offset(0, 4),
                    ),
                  ]
                : null,
          ),
          child: Material(
            color: Colors.transparent,
            child: InkWell(
              onTap: isDisabled ? null : widget.onPressed,
              borderRadius: BorderRadius.circular(PSpacing.radiusMD),
              splashColor: AppColors.pressedOverlay,
              highlightColor: AppColors.hoverOverlay,
              child: Padding(
                padding: widget.size.padding,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    if (widget.loading)
                      SizedBox(
                        width: widget.size.iconSize,
                        height: widget.size.iconSize,
                        child: CircularProgressIndicator(
                          strokeWidth: 2.0,
                          valueColor: AlwaysStoppedAnimation<Color>(
                            widget.variant.textColor(isDisabled: isDisabled),
                          ),
                        ),
                      )
                    else if (widget.icon != null) ...[
                      IconTheme(
                        data: IconThemeData(
          color: widget.variant.textColor(isDisabled: isDisabled),
                          size: widget.size.iconSize,
                        ),
                        child: widget.icon!,
                      ),
                      SizedBox(width: PSpacing.iconTextGap),
                    ],
                    DefaultTextStyle(
                      style: widget.size.textStyle.copyWith(
                        color: widget.variant.textColor(isDisabled: isDisabled),
                      ),
                      child: contentChild,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ).animate(target: _isPressed ? 1 : 0).scaleXY(
          begin: 1.0,
          end: 0.95,
          duration: const Duration(milliseconds: 100),
        ),
      ),
    );
  }
}

/// Button variants
enum PButtonVariant {
  primary,
  secondary,
  outline,
  ghost,
  danger;

  Gradient? gradient({
    required bool isDisabled,
    required bool isHovered,
    required bool isPressed,
  }) {
    if (isDisabled) return null;

    switch (this) {
      case PButtonVariant.primary:
        return LinearGradient(
          colors: [
            AppColors.gradientAStart,
            AppColors.gradientAEnd,
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        );
      case PButtonVariant.secondary:
        return LinearGradient(
          colors: [
            AppColors.gradientBStart,
            AppColors.gradientBEnd,
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        );
      case PButtonVariant.outline:
      case PButtonVariant.ghost:
      case PButtonVariant.danger:
        return null;
    }
  }

  BoxBorder? border({required bool isDisabled}) {
    if (this == PButtonVariant.outline) {
      return Border.all(
        color: isDisabled ? AppColors.borderSubtle : AppColors.borderDefault,
        width: 1.5,
      );
    }
    return null;
  }

  Color textColor({required bool isDisabled}) {
    if (isDisabled) return AppColors.textDisabled;

    switch (this) {
      case PButtonVariant.primary:
      case PButtonVariant.secondary:
        return AppColors.textOnAccent;
      case PButtonVariant.danger:
        return AppColors.error;
      case PButtonVariant.outline:
      case PButtonVariant.ghost:
        return AppColors.textPrimary;
    }
  }
}

/// Button sizes
enum PButtonSize {
  small,
  medium,
  large;

  double get height {
    switch (this) {
      case PButtonSize.small:
        return 48.0;
      case PButtonSize.medium:
        return 52.0;
      case PButtonSize.large:
        return 56.0;
    }
  }

  EdgeInsets get padding {
    switch (this) {
      case PButtonSize.small:
        return const EdgeInsets.symmetric(horizontal: 12.0, vertical: 6.0);
      case PButtonSize.medium:
        return const EdgeInsets.symmetric(horizontal: 24.0, vertical: 12.0);
      case PButtonSize.large:
        return const EdgeInsets.symmetric(horizontal: 32.0, vertical: 16.0);
    }
  }

  TextStyle get textStyle {
    switch (this) {
      case PButtonSize.small:
        return PTypography.labelSmall();
      case PButtonSize.medium:
        return PTypography.labelLarge();
      case PButtonSize.large:
        return PTypography.labelLarge();
    }
  }

  double get iconSize {
    switch (this) {
      case PButtonSize.small:
        return PSpacing.iconSM;
      case PButtonSize.medium:
        return PSpacing.iconMD;
      case PButtonSize.large:
        return PSpacing.iconLG;
    }
  }
}

/// Icon button component
class PIconButton extends StatefulWidget {
  const PIconButton({
    required this.icon,
    required this.onPressed,
    this.size = PIconButtonSize.medium,
    this.tooltip,
    super.key,
  });

  final Widget icon;
  final VoidCallback? onPressed;
  final PIconButtonSize size;
  final String? tooltip;

  @override
  State<PIconButton> createState() => _PIconButtonState();
}

class _PIconButtonState extends State<PIconButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final button = MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      cursor: widget.onPressed == null
          ? SystemMouseCursors.forbidden
          : SystemMouseCursors.click,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        width: widget.size.size,
        height: widget.size.size,
        decoration: BoxDecoration(
          color: _isHovered && widget.onPressed != null
              ? AppColors.hoverOverlay
              : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        ),
        child: IconButton(
          icon: widget.icon,
          onPressed: widget.onPressed,
          iconSize: widget.size.iconSize,
          color: widget.onPressed == null ? AppColors.textDisabled : AppColors.textPrimary,
          padding: EdgeInsets.zero,
        ),
      ),
    );

    if (widget.tooltip != null) {
      return Tooltip(
        message: widget.tooltip,
        child: button,
      );
    }

    return button;
  }
}

/// Icon button sizes
enum PIconButtonSize {
  small,
  medium,
  large;

  double get size {
    switch (this) {
      case PIconButtonSize.small:
        return 48.0;
      case PIconButtonSize.medium:
        return 48.0;
      case PIconButtonSize.large:
        return 56.0;
    }
  }

  double get iconSize {
    switch (this) {
      case PIconButtonSize.small:
        return PSpacing.iconSM;
      case PIconButtonSize.medium:
        return PSpacing.iconMD;
      case PIconButtonSize.large:
        return PSpacing.iconLG;
    }
  }
}

