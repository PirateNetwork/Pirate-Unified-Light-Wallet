import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../atoms/p_icon_button.dart';
import '../atoms/theme_toggle_button.dart';

/// Premium app bar replacement with Pirate design language.
class PAppBar extends StatelessWidget implements PreferredSizeWidget {
  const PAppBar({
    required this.title,
    this.subtitle,
    this.actions,
    this.leading,
    this.showBackButton,
    this.onBack,
    this.useGradientBackground = false,
    this.centerTitle = false,
    this.surfaceColor,
    super.key,
  });

  final String title;
  final String? subtitle;
  final List<Widget>? actions;
  final Widget? leading;
  final bool? showBackButton;
  final VoidCallback? onBack;
  final bool useGradientBackground;
  final bool centerTitle;
  final Color? surfaceColor;

  @override
  Size get preferredSize => const Size.fromHeight(88);

  bool _shouldShowBack(BuildContext context) {
    if (showBackButton != null) {
      return showBackButton!;
    }
    return Navigator.of(context).canPop();
  }

  bool _isOnboardingRoute(BuildContext context) {
    final location = GoRouterState.of(context).uri.path;
    return location.startsWith('/onboarding');
  }

  @override
  Widget build(BuildContext context) {
    final topPadding = MediaQuery.of(context).padding.top;
    final isNarrow = MediaQuery.of(context).size.width < 360;
    final resolvedLeading =
        leading ?? (_shouldShowBack(context) ? _buildBackButton(context) : null);
    
    // Automatically add theme toggle to actions if not in onboarding
    final effectiveActions = <Widget>[];
    if (actions != null) {
      effectiveActions.addAll(actions!);
    }
    if (!_isOnboardingRoute(context)) {
      effectiveActions.add(const ThemeToggleButton());
    }

    final gradient = useGradientBackground
        ? LinearGradient(
            colors: [
              AppColors.gradientAStart.withValues(alpha: 0.35),
              AppColors.gradientAEnd.withValues(alpha: 0.2),
            ],
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
          )
        : null;

    final decoration = BoxDecoration(
      color: gradient == null ? (surfaceColor ?? AppColors.backgroundSurface) : null,
      gradient: gradient,
      border: Border(
        bottom: BorderSide(color: AppColors.borderSubtle),
      ),
      boxShadow: [
        BoxShadow(
          color: AppColors.shadow,
          blurRadius: 12,
          offset: const Offset(0, 6),
        ),
      ],
    );

    final titleStyle = PTypography.heading4(color: AppColors.textPrimary).copyWith(
      fontSize: isNarrow ? 18 : null,
    );
    final subtitleStyle = PTypography.bodySmall(color: AppColors.textSecondary).copyWith(
      fontSize: isNarrow ? 12 : null,
    );

    final titleColumn = Column(
      crossAxisAlignment:
          centerTitle ? CrossAxisAlignment.center : CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          title,
          style: titleStyle,
          textAlign: centerTitle ? TextAlign.center : TextAlign.left,
          maxLines: isNarrow ? 2 : 1,
          overflow: TextOverflow.ellipsis,
        ),
        if (subtitle != null) ...[
          const SizedBox(height: PSpacing.xs),
          Text(
            subtitle!,
            style: subtitleStyle,
            textAlign: centerTitle ? TextAlign.center : TextAlign.left,
            maxLines: isNarrow ? 2 : 2,
            overflow: TextOverflow.ellipsis,
          ),
        ],
      ],
    );

    return Material(
      color: Colors.transparent,
      child: Container(
        width: double.infinity,
        decoration: decoration,
        padding: EdgeInsets.only(
          top: topPadding + PSpacing.md,
          bottom: PSpacing.md,
          left: PSpacing.lg,
          right: PSpacing.lg,
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            if (resolvedLeading != null) ...[
              resolvedLeading,
              const SizedBox(width: PSpacing.md),
            ],
            Expanded(child: titleColumn),
            if (effectiveActions.isNotEmpty)
              Row(
                mainAxisSize: MainAxisSize.min,
                children: effectiveActions
                    .map(
                      (action) => Padding(
                        padding: const EdgeInsets.only(left: PSpacing.sm),
                        child: action,
                      ),
                    )
                    .toList(),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildBackButton(BuildContext context) {
    return PIconButton(
      icon: Icon(Icons.arrow_back, color: AppColors.textPrimary),
      onPressed: onBack ?? () => Navigator.of(context).maybePop(),
      tooltip: 'Back',
      size: PIconButtonSize.large,
    );
  }
}

