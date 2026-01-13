import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// High-level text button that replaces the default Material TextButton.
///
/// Provides consistent typography, hover/focus states, and accent colors
/// that match the Pirate design system.
class PTextButton extends StatefulWidget {
  const PTextButton({
    String? label,
    String? text,
    this.onPressed,
    this.leadingIcon,
    this.trailingIcon,
    this.variant = PTextButtonVariant.accent,
    this.compact = false,
    this.fullWidth = false,
    super.key,
  })  : label = label ?? text ?? '',
        assert(label != null || text != null, 'Provide label or text');

  final String label;
  final VoidCallback? onPressed;
  final IconData? leadingIcon;
  final IconData? trailingIcon;
  final PTextButtonVariant variant;
  final bool compact;
  final bool fullWidth;

  bool get _isDisabled => onPressed == null;

  @override
  State<PTextButton> createState() => _PTextButtonState();
}

class _PTextButtonState extends State<PTextButton> {
  bool _isHovered = false;
  bool _isFocused = false;

  @override
  Widget build(BuildContext context) {
    final textColor =
        widget._isDisabled ? AppColors.textDisabled : widget.variant.textColor;
    final hoverColor = widget.variant.hoverColor;
    final focusOutline = widget.variant.focusColor;

    final horizontalPadding = widget.compact ? PSpacing.sm : PSpacing.md;
    final verticalPadding = widget.compact ? PSpacing.xs : PSpacing.sm;

    final content = Row(
      mainAxisSize:
          widget.fullWidth ? MainAxisSize.max : MainAxisSize.min,
      mainAxisAlignment: widget.fullWidth
          ? MainAxisAlignment.center
          : MainAxisAlignment.start,
      children: [
        if (widget.leadingIcon != null) ...[
          Icon(widget.leadingIcon, size: PSpacing.iconMD, color: textColor),
          const SizedBox(width: PSpacing.xs),
        ],
        Flexible(
          child: Text(
            widget.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: PTypography.labelLarge(color: textColor),
            textAlign:
                widget.fullWidth ? TextAlign.center : TextAlign.left,
          ),
        ),
        if (widget.trailingIcon != null) ...[
          const SizedBox(width: PSpacing.xs),
          Icon(widget.trailingIcon, size: PSpacing.iconMD, color: textColor),
        ],
      ],
    );

    final borderRadius = BorderRadius.circular(PSpacing.radiusMD);

    return Focus(
      onFocusChange: (value) => setState(() => _isFocused = value),
      child: MouseRegion(
        cursor: widget._isDisabled
            ? SystemMouseCursors.forbidden
            : SystemMouseCursors.click,
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() => _isHovered = false),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          constraints: const BoxConstraints(minHeight: 48, minWidth: 48),
          decoration: BoxDecoration(
            color: _isHovered && !widget._isDisabled ? hoverColor : Colors.transparent,
            borderRadius: borderRadius,
            border: _isFocused
                ? Border.all(color: focusOutline, width: 1.5)
                : null,
          ),
          child: Material(
            color: Colors.transparent,
            child: InkWell(
              onTap: widget._isDisabled ? null : widget.onPressed,
              borderRadius: borderRadius,
              splashColor: focusOutline.withValues(alpha: 0.2),
              highlightColor: hoverColor,
              child: Padding(
                padding: EdgeInsets.symmetric(
                  horizontal: horizontalPadding,
                  vertical: verticalPadding,
                ),
                child: content,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Variants for text buttons that map to brand colors.
enum PTextButtonVariant {
  accent,
  neutral,
  subtle,
  danger,
}

extension _VariantColorExtension on PTextButtonVariant {
  Color get textColor {
    switch (this) {
      case PTextButtonVariant.accent:
        return AppColors.gradientAStart;
      case PTextButtonVariant.neutral:
        return AppColors.textPrimary;
      case PTextButtonVariant.subtle:
        return AppColors.textSecondary;
      case PTextButtonVariant.danger:
        return AppColors.error;
    }
  }

  Color get hoverColor {
    switch (this) {
      case PTextButtonVariant.accent:
        return AppColors.gradientAStart.withValues(alpha: 0.08);
      case PTextButtonVariant.neutral:
        return AppColors.hoverOverlay;
      case PTextButtonVariant.subtle:
        return AppColors.hoverOverlay;
      case PTextButtonVariant.danger:
        return AppColors.error.withValues(alpha: 0.12);
    }
  }

  Color get focusColor {
    switch (this) {
      case PTextButtonVariant.accent:
        return AppColors.gradientAEnd;
      case PTextButtonVariant.neutral:
        return AppColors.focusRing;
      case PTextButtonVariant.subtle:
        return AppColors.focusRing.withValues(alpha: 0.7);
      case PTextButtonVariant.danger:
        return AppColors.error;
    }
  }
}

