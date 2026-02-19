/// Gradient Button — Primary CTA with Gradient A
///
/// A premium button component using Gradient A (Cyan → Blue) for primary actions.
/// Features:
/// - 60fps hover/press animations
/// - WCAG AA accessible (48dp min touch target)
/// - Focus ring support
/// - Haptic feedback on mobile
/// - High contrast mode support
library;

import 'package:flutter/material.dart';
import '../../design/deep_space_theme.dart';

/// Primary CTA button with Gradient A
class PGradientButton extends StatefulWidget {
  const PGradientButton({
    required this.text,
    required this.onPressed,
    this.icon,
    this.size = PGradientButtonSize.large,
    this.fullWidth = false,
    this.isLoading = false,
    this.enabled = true,
    this.semanticLabel,
    this.autofocus = false,
    super.key,
  });

  final String text;
  final VoidCallback? onPressed;
  final IconData? icon;
  final PGradientButtonSize size;
  final bool fullWidth;
  final bool isLoading;
  final bool enabled;
  final String? semanticLabel;
  final bool autofocus;

  @override
  State<PGradientButton> createState() => _PGradientButtonState();
}

class _PGradientButtonState extends State<PGradientButton>
    with SingleTickerProviderStateMixin {
  bool _isHovered = false;
  bool _isPressed = false;
  bool _isFocused = false;
  bool _reduceMotion = false;
  late AnimationController _animationController;
  late Animation<double> _scaleAnimation;

  bool get _isEnabled =>
      widget.enabled && widget.onPressed != null && !widget.isLoading;

  @override
  void initState() {
    super.initState();
    _animationController = AnimationController(
      duration: DeepSpaceDurations.fast,
      vsync: this,
    );
    _scaleAnimation = Tween<double>(begin: 1.0, end: 0.96).animate(
      CurvedAnimation(
        parent: _animationController,
        curve: DeepSpaceCurves.snap,
      ),
    );
  }

  @override
  void dispose() {
    _animationController.dispose();
    super.dispose();
  }

  void _handleTapDown(TapDownDetails details) {
    if (!_isEnabled) return;
    setState(() => _isPressed = true);
    if (_reduceMotion) return;
    _animationController.forward();
  }

  void _handleTapUp(TapUpDetails details) {
    setState(() => _isPressed = false);
    if (_reduceMotion) return;
    _animationController.reverse();
  }

  void _handleTapCancel() {
    setState(() => _isPressed = false);
    if (_reduceMotion) return;
    _animationController.reverse();
  }

  void _handleTap() {
    if (!_isEnabled) return;
    // Haptic feedback on mobile
    DeepSpaceHaptics.lightImpact();
    widget.onPressed?.call();
  }

  @override
  Widget build(BuildContext context) {
    _reduceMotion = MediaQuery.of(context).disableAnimations;
    return Semantics(
      label: widget.semanticLabel ?? widget.text,
      button: true,
      enabled: _isEnabled,
      child: Focus(
        autofocus: widget.autofocus,
        onFocusChange: (focused) => setState(() => _isFocused = focused),
        child: MouseRegion(
          onEnter: (_) => setState(() => _isHovered = true),
          onExit: (_) => setState(() => _isHovered = false),
          cursor: _isEnabled
              ? SystemMouseCursors.click
              : SystemMouseCursors.forbidden,
          child: GestureDetector(
            onTapDown: _handleTapDown,
            onTapUp: _handleTapUp,
            onTapCancel: _handleTapCancel,
            onTap: _handleTap,
            child: _reduceMotion
                ? _buildButton(reduceMotion: true)
                : AnimatedBuilder(
                    animation: _animationController,
                    builder: (context, child) {
                      return Transform.scale(
                        scale: _scaleAnimation.value,
                        child: _buildButton(reduceMotion: false),
                      );
                    },
                  ),
          ),
        ),
      ),
    );
  }

  Widget _buildButton({required bool reduceMotion}) {
    final double height = widget.size.height;
    final EdgeInsets padding = widget.size.padding;
    final TextStyle textStyle = widget.size.textStyle;
    final showShadow =
        _isEnabled && (_isHovered || _isPressed) && !reduceMotion;

    return AnimatedContainer(
      duration: reduceMotion ? Duration.zero : DeepSpaceDurations.fast,
      curve: DeepSpaceCurves.standard,
      width: widget.fullWidth ? double.infinity : null,
      height: height,
      constraints: BoxConstraints(
        minWidth: DeepSpaceA11y.minTouchTarget,
        minHeight: DeepSpaceA11y.minTouchTarget,
      ),
      decoration: BoxDecoration(
        gradient: _isEnabled ? _buildGradient() : null,
        color: _isEnabled ? null : AppColors.nebula,
        borderRadius: BorderRadius.circular(widget.size.borderRadius),
        border: _isFocused
            ? Border.all(
                color: AppColors.focus,
                width: DeepSpaceA11y.focusRingWidth,
              )
            : null,
        boxShadow: showShadow
            ? [
                BoxShadow(
                  color: AppColors.glow.withValues(
                    alpha: _isPressed ? 0.6 : 0.4,
                  ),
                  blurRadius: _isPressed ? 24 : 16,
                  spreadRadius: _isPressed ? 2 : 0,
                  offset: const Offset(0, 4),
                ),
              ]
            : null,
      ),
      child: Material(
        color: Colors.transparent,
        child: Padding(
          padding: padding,
          child: Row(
            mainAxisSize: widget.fullWidth
                ? MainAxisSize.max
                : MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              if (widget.isLoading) ...[
                SizedBox(
                  width: widget.size.iconSize,
                  height: widget.size.iconSize,
                  child: CircularProgressIndicator(
                    strokeWidth: 2.0,
                    valueColor: AlwaysStoppedAnimation<Color>(
                      _isEnabled
                          ? AppColors.textOnGradient
                          : AppColors.textDisabled,
                    ),
                  ),
                ),
                const SizedBox(width: 12),
              ] else if (widget.icon != null) ...[
                Icon(
                  widget.icon,
                  size: widget.size.iconSize,
                  color: _isEnabled
                      ? AppColors.textOnGradient
                      : AppColors.textDisabled,
                ),
                const SizedBox(width: 10),
              ],
              Text(
                widget.text,
                style: textStyle.copyWith(
                  color: _isEnabled
                      ? AppColors.textOnGradient
                      : AppColors.textDisabled,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  LinearGradient _buildGradient() {
    // Shift gradient on hover for subtle effect
    final double shift = _isHovered ? 0.1 : 0.0;

    return LinearGradient(
      colors: [
        Color.lerp(AppColors.gradientAStart, AppColors.gradientAMid, shift)!,
        Color.lerp(AppColors.gradientAMid, AppColors.gradientAEnd, shift)!,
      ],
      begin: Alignment.topLeft,
      end: Alignment.bottomRight,
    );
  }
}

/// Button sizes
enum PGradientButtonSize {
  small,
  medium,
  large;

  double get height {
    switch (this) {
      case PGradientButtonSize.small:
        return 40.0;
      case PGradientButtonSize.medium:
        return 48.0;
      case PGradientButtonSize.large:
        return 56.0;
    }
  }

  EdgeInsets get padding {
    switch (this) {
      case PGradientButtonSize.small:
        return const EdgeInsets.symmetric(horizontal: 16.0, vertical: 10.0);
      case PGradientButtonSize.medium:
        return const EdgeInsets.symmetric(horizontal: 24.0, vertical: 12.0);
      case PGradientButtonSize.large:
        return const EdgeInsets.symmetric(horizontal: 32.0, vertical: 16.0);
    }
  }

  double get borderRadius {
    switch (this) {
      case PGradientButtonSize.small:
        return 10.0;
      case PGradientButtonSize.medium:
        return 12.0;
      case PGradientButtonSize.large:
        return 14.0;
    }
  }

  double get iconSize {
    switch (this) {
      case PGradientButtonSize.small:
        return 18.0;
      case PGradientButtonSize.medium:
        return 20.0;
      case PGradientButtonSize.large:
        return 24.0;
    }
  }

  TextStyle get textStyle {
    switch (this) {
      case PGradientButtonSize.small:
        return AppTypography.caption.copyWith(fontWeight: FontWeight.w600);
      case PGradientButtonSize.medium:
        return AppTypography.body.copyWith(fontWeight: FontWeight.w600);
      case PGradientButtonSize.large:
        return AppTypography.body.copyWith(
          fontWeight: FontWeight.w600,
          fontSize: 16,
        );
    }
  }
}

// =============================================================================
// Hero Header with Gradient A
// =============================================================================

/// Hero header with Gradient A background glow
class PGradientHeroHeader extends StatelessWidget {
  const PGradientHeroHeader({
    required this.title,
    this.subtitle,
    this.actions,
    this.showGlow = true,
    this.height,
    super.key,
  });

  final String title;
  final String? subtitle;
  final List<Widget>? actions;
  final bool showGlow;
  final double? height;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: height,
      padding: const EdgeInsets.all(AppSpacing.xl),
      decoration: BoxDecoration(
        color: AppColors.deepSpace,
        gradient: showGlow
            ? LinearGradient(
                colors: [
                  AppColors.gradientAStart.withValues(alpha: 0.08),
                  AppColors.gradientAEnd.withValues(alpha: 0.04),
                  Colors.transparent,
                ],
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                stops: const [0.0, 0.4, 1.0],
              )
            : null,
        border: Border(
          bottom: BorderSide(color: AppColors.borderSubtle, width: 1.0),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    // Gradient text effect for title
                    ShaderMask(
                      shaderCallback: (bounds) =>
                          AppColors.gradientALinear.createShader(bounds),
                      blendMode: BlendMode.srcIn,
                      child: Text(
                        title,
                        style: AppTypography.h2.copyWith(
                          color: Colors.white,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                    ),
                    if (subtitle != null) ...[
                      const SizedBox(height: AppSpacing.sm),
                      Text(
                        subtitle!,
                        style: AppTypography.body.copyWith(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (actions != null && actions!.isNotEmpty) ...[
                const SizedBox(width: AppSpacing.lg),
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: actions!
                      .map(
                        (action) => Padding(
                          padding: const EdgeInsets.only(left: AppSpacing.sm),
                          child: action,
                        ),
                      )
                      .toList(),
                ),
              ],
            ],
          ),
        ],
      ),
    );
  }
}

// =============================================================================
// Animated Focus Ring (Accessibility)
// =============================================================================

/// Focus ring wrapper for custom focusable widgets
class PFocusRing extends StatefulWidget {
  const PFocusRing({
    required this.child,
    this.borderRadius = 12.0,
    this.color,
    this.width = DeepSpaceA11y.focusRingWidth,
    this.offset = DeepSpaceA11y.focusRingOffset,
    super.key,
  });

  final Widget child;
  final double borderRadius;
  final Color? color;
  final double width;
  final double offset;

  @override
  State<PFocusRing> createState() => _PFocusRingState();
}

class _PFocusRingState extends State<PFocusRing>
    with SingleTickerProviderStateMixin {
  bool _isFocused = false;
  late AnimationController _controller;
  late Animation<double> _opacityAnimation;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      duration: DeepSpaceDurations.fast,
      vsync: this,
    );
    _opacityAnimation = Tween<double>(begin: 0.0, end: 1.0).animate(
      CurvedAnimation(parent: _controller, curve: DeepSpaceCurves.standard),
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      onFocusChange: (focused) {
        setState(() => _isFocused = focused);
        if (focused) {
          _controller.forward();
        } else {
          _controller.reverse();
        }
      },
      child: AnimatedBuilder(
        animation: _controller,
        builder: (context, child) {
          return Container(
            decoration: _isFocused
                ? BoxDecoration(
                    borderRadius: BorderRadius.circular(
                      widget.borderRadius + widget.offset,
                    ),
                    border: Border.all(
                      color: (widget.color ?? AppColors.focus).withValues(
                        alpha: _opacityAnimation.value,
                      ),
                      width: widget.width,
                    ),
                  )
                : null,
            padding: EdgeInsets.all(_isFocused ? widget.offset : 0),
            child: widget.child,
          );
        },
      ),
    );
  }
}
