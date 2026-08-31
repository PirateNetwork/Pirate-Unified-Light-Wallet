import 'package:flutter/material.dart';

import '../../core/platform/platform_utils.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Stashi Wallet Navigation
/// - BottomNavigationBar for mobile
/// - Compact navigation rail + AppSidebar for desktop
class PNav extends StatelessWidget {
  static const Key mobileNavigationKey = Key('mobile-navigation-bar');

  const PNav({
    required this.currentIndex,
    required this.onDestinationSelected,
    required this.destinations,
    this.onPayTap,
    this.payIndex,
    super.key,
  });

  final int currentIndex;
  final ValueChanged<int> onDestinationSelected;
  final List<PNavDestination> destinations;
  final VoidCallback? onPayTap;
  final int? payIndex;

  int? get _resolvedPayIndex {
    final explicit = payIndex;
    if (explicit != null) return explicit;
    final inferred = destinations.indexWhere((dest) => dest.isPay);
    return inferred >= 0 ? inferred : null;
  }

  @override
  Widget build(BuildContext context) {
    if (isDesktopPlatform) {
      final compact = PSpacing.isCompactDesktopViewport(
        MediaQuery.sizeOf(context),
      );
      return SizedBox(
        key: const ValueKey('desktop-navigation-rail'),
        width: compact
            ? PSpacing.desktopCompactNavRailWidth
            : PSpacing.desktopNavRailWidth,
        child: ListView.separated(
          padding: const EdgeInsets.symmetric(
            horizontal: PSpacing.sm,
            vertical: PSpacing.sm,
          ),
          itemCount: destinations.length,
          separatorBuilder: (_, _) => const SizedBox(height: PSpacing.xs),
          itemBuilder: (context, index) {
            final destination = destinations[index];
            return _DesktopNavItem(
              key: ValueKey('desktop-nav-item-$index'),
              destination: destination,
              isSelected: index == currentIndex,
              compact: compact,
              onTap: () => onDestinationSelected(index),
            );
          },
        ),
      );
    }

    final pay = _resolvedPayIndex;
    if (pay == null || onPayTap == null) {
      return BottomNavigationBar(
        currentIndex: currentIndex,
        onTap: onDestinationSelected,
        backgroundColor: AppColors.backgroundSurface,
        selectedItemColor: AppColors.focusRing,
        unselectedItemColor: AppColors.textSecondary,
        type: BottomNavigationBarType.fixed,
        elevation: 0,
        items: destinations
            .map(
              (dest) => BottomNavigationBarItem(
                icon: Icon(dest.icon),
                activeIcon: Icon(dest.selectedIcon ?? dest.icon),
                label: dest.label,
              ),
            )
            .toList(),
      );
    }

    final left = destinations.take(pay).toList();
    final right = destinations.skip(pay + 1).toList();
    final payDest = destinations[pay];
    final compactLandscape = PSpacing.isCompactLandscape(
      MediaQuery.sizeOf(context),
    );

    return SafeArea(
      top: false,
      child: Container(
        key: mobileNavigationKey,
        decoration: BoxDecoration(
          color: AppColors.backgroundSurface,
          border: Border(
            top: BorderSide(color: AppColors.borderSubtle, width: 1.0),
          ),
        ),
        padding: EdgeInsets.symmetric(
          horizontal: compactLandscape ? PSpacing.sm : PSpacing.md,
          vertical: compactLandscape ? PSpacing.xxs : PSpacing.sm,
        ),
        child: Row(
          children: [
            ...left.map(
              (dest) => Expanded(
                child: _NavItem(
                  destination: dest,
                  isSelected: destinations.indexOf(dest) == currentIndex,
                  compact: compactLandscape,
                  onTap: () =>
                      onDestinationSelected(destinations.indexOf(dest)),
                ),
              ),
            ),
            Expanded(
              child: _PayAction(
                icon: payDest.selectedIcon ?? payDest.icon,
                label: payDest.label,
                compact: compactLandscape,
                onTap: onPayTap!,
              ),
            ),
            ...right.map(
              (dest) => Expanded(
                child: _NavItem(
                  destination: dest,
                  isSelected: destinations.indexOf(dest) == currentIndex,
                  compact: compactLandscape,
                  onTap: () =>
                      onDestinationSelected(destinations.indexOf(dest)),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _DesktopNavItem extends StatefulWidget {
  const _DesktopNavItem({
    required this.destination,
    required this.isSelected,
    required this.compact,
    required this.onTap,
    super.key,
  });

  final PNavDestination destination;
  final bool isSelected;
  final bool compact;
  final VoidCallback onTap;

  @override
  State<_DesktopNavItem> createState() => _DesktopNavItemState();
}

class _DesktopNavItemState extends State<_DesktopNavItem> {
  bool _isFocused = false;

  @override
  Widget build(BuildContext context) {
    final radius = BorderRadius.circular(PSpacing.radiusSM);
    final iconColor = widget.isSelected
        ? AppColors.focusRing
        : AppColors.textSecondary;
    final labelColor = widget.isSelected
        ? AppColors.textPrimary
        : AppColors.textSecondary;
    final background = widget.isSelected
        ? AppColors.selectedBackground
        : Colors.transparent;
    final reduceMotion =
        MediaQuery.maybeOf(context)?.disableAnimations ?? false;

    return Semantics(
      button: true,
      selected: widget.isSelected,
      child: AnimatedContainer(
        duration: reduceMotion
            ? Duration.zero
            : const Duration(milliseconds: 150),
        constraints: BoxConstraints(minHeight: widget.compact ? 60 : 72),
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: background,
          borderRadius: radius,
          border: Border.all(
            color: _isFocused
                ? AppColors.focusRing
                : widget.isSelected
                ? AppColors.selectedBorder
                : Colors.transparent,
            width: _isFocused ? 2 : 1,
          ),
        ),
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: widget.onTap,
            onFocusChange: (focused) {
              if (_isFocused != focused) {
                setState(() => _isFocused = focused);
              }
            },
            mouseCursor: SystemMouseCursors.click,
            borderRadius: radius,
            hoverColor: AppColors.hoverOverlay,
            focusColor: AppColors.focusRingSubtle,
            highlightColor: AppColors.pressedOverlay,
            splashColor: AppColors.pressedOverlay,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: widget.compact ? PSpacing.xxs : PSpacing.xs,
                vertical: PSpacing.xs,
              ),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    widget.isSelected
                        ? widget.destination.selectedIcon ??
                              widget.destination.icon
                        : widget.destination.icon,
                    color: iconColor,
                    size: widget.compact ? PSpacing.iconMD : PSpacing.iconLG,
                  ),
                  const SizedBox(height: PSpacing.xxs),
                  Text(
                    widget.destination.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: PTypography.caption(color: labelColor),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class PNavDestination {
  const PNavDestination({
    required this.icon,
    required this.label,
    this.selectedIcon,
    this.isPay = false,
  });

  final IconData icon;
  final IconData? selectedIcon;
  final String label;
  final bool isPay;
}

class _NavItem extends StatelessWidget {
  const _NavItem({
    required this.destination,
    required this.isSelected,
    required this.compact,
    required this.onTap,
  });

  final PNavDestination destination;
  final bool isSelected;
  final bool compact;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final color = isSelected ? AppColors.focusRing : AppColors.textSecondary;
    final content = compact
        ? Row(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(destination.icon, color: color, size: PSpacing.iconMD),
              const SizedBox(width: PSpacing.xxs),
              Flexible(
                child: Text(
                  destination.label,
                  maxLines: 2,
                  textAlign: TextAlign.center,
                  style: PTypography.labelSmall(color: color),
                ),
              ),
            ],
          )
        : Column(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(destination.icon, color: color, size: PSpacing.iconMD),
              const SizedBox(height: PSpacing.xxs),
              Text(
                destination.label,
                maxLines: 2,
                textAlign: TextAlign.center,
                style: PTypography.labelSmall(color: color),
              ),
            ],
          );
    return InkWell(
      borderRadius: BorderRadius.circular(PSpacing.radiusSM),
      onTap: onTap,
      child: ConstrainedBox(
        constraints: BoxConstraints(minHeight: compact ? 44 : 56),
        child: Padding(
          padding: EdgeInsets.symmetric(
            vertical: compact ? PSpacing.xxs : PSpacing.xs,
          ),
          child: content,
        ),
      ),
    );
  }
}

class _PayAction extends StatelessWidget {
  const _PayAction({
    required this.icon,
    required this.label,
    required this.compact,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool compact;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final content = compact
        ? Row(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                icon,
                color: AppColors.textSecondary,
                size: PSpacing.iconMD,
                semanticLabel: label,
              ),
              const SizedBox(width: PSpacing.xxs),
              Flexible(
                child: Text(
                  label,
                  maxLines: 2,
                  textAlign: TextAlign.center,
                  style: PTypography.labelSmall(color: AppColors.textSecondary),
                ),
              ),
            ],
          )
        : Column(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                icon,
                color: AppColors.textSecondary,
                size: PSpacing.iconMD,
                semanticLabel: label,
              ),
              const SizedBox(height: PSpacing.xxs),
              Text(
                label,
                maxLines: 2,
                textAlign: TextAlign.center,
                style: PTypography.labelSmall(color: AppColors.textSecondary),
              ),
            ],
          );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusLG),
      child: ConstrainedBox(
        constraints: BoxConstraints(minHeight: compact ? 44 : 56),
        child: Padding(
          padding: EdgeInsets.symmetric(
            vertical: compact ? PSpacing.xxs : PSpacing.xs,
          ),
          child: content,
        ),
      ),
    );
  }
}
