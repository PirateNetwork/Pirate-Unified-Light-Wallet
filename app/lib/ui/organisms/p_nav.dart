import 'dart:io';
import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Navigation
/// - BottomNavigationBar for mobile
/// - Compact navigation rail + AppSidebar for desktop
class PNav extends StatelessWidget {
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

  bool get _isDesktop =>
      Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  int? get _resolvedPayIndex {
    final explicit = payIndex;
    if (explicit != null) return explicit;
    final inferred = destinations.indexWhere((dest) => dest.isPay);
    return inferred >= 0 ? inferred : null;
  }

  @override
  Widget build(BuildContext context) {
    if (_isDesktop) {
      return SizedBox(
        key: const ValueKey('desktop-navigation-rail'),
        width: PSpacing.desktopNavRailWidth,
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

    return SafeArea(
      top: false,
      child: Container(
        decoration: BoxDecoration(
          color: AppColors.backgroundSurface,
          border: Border(
            top: BorderSide(color: AppColors.borderSubtle, width: 1.0),
          ),
        ),
        padding: const EdgeInsets.symmetric(
          horizontal: PSpacing.md,
          vertical: PSpacing.sm,
        ),
        child: Row(
          children: [
            ...left.map(
              (dest) => Expanded(
                child: _NavItem(
                  destination: dest,
                  isSelected: destinations.indexOf(dest) == currentIndex,
                  onTap: () =>
                      onDestinationSelected(destinations.indexOf(dest)),
                ),
              ),
            ),
            Expanded(
              child: _PayAction(
                icon: payDest.selectedIcon ?? payDest.icon,
                label: payDest.label,
                onTap: onPayTap!,
              ),
            ),
            ...right.map(
              (dest) => Expanded(
                child: _NavItem(
                  destination: dest,
                  isSelected: destinations.indexOf(dest) == currentIndex,
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

class _DesktopNavItem extends StatelessWidget {
  const _DesktopNavItem({
    required this.destination,
    required this.isSelected,
    required this.onTap,
    super.key,
  });

  final PNavDestination destination;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final radius = BorderRadius.circular(PSpacing.radiusSM);
    final iconColor = isSelected
        ? AppColors.focusRing
        : AppColors.textSecondary;
    final labelColor = isSelected
        ? AppColors.textPrimary
        : AppColors.textSecondary;
    final background = isSelected
        ? AppColors.selectedBackground
        : Colors.transparent;
    final reduceMotion =
        MediaQuery.maybeOf(context)?.disableAnimations ?? false;

    return Semantics(
      button: true,
      selected: isSelected,
      child: AnimatedContainer(
        duration: reduceMotion
            ? Duration.zero
            : const Duration(milliseconds: 150),
        constraints: const BoxConstraints(minHeight: 72),
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: background,
          borderRadius: radius,
          border: Border.all(
            color: isSelected ? AppColors.selectedBorder : Colors.transparent,
          ),
        ),
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: onTap,
            mouseCursor: SystemMouseCursors.click,
            borderRadius: radius,
            hoverColor: AppColors.hoverOverlay,
            focusColor: AppColors.focusRingSubtle,
            highlightColor: AppColors.pressedOverlay,
            splashColor: AppColors.pressedOverlay,
            child: Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: PSpacing.xs,
                vertical: PSpacing.xs,
              ),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    isSelected
                        ? destination.selectedIcon ?? destination.icon
                        : destination.icon,
                    color: iconColor,
                    size: PSpacing.iconLG,
                  ),
                  const SizedBox(height: PSpacing.xxs),
                  Text(
                    destination.label,
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
    required this.onTap,
  });

  final PNavDestination destination;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final color = isSelected ? AppColors.focusRing : AppColors.textSecondary;
    return InkWell(
      borderRadius: BorderRadius.circular(PSpacing.radiusSM),
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: PSpacing.xs),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(destination.icon, color: color, size: PSpacing.iconMD),
            const SizedBox(height: PSpacing.xxs),
            Text(
              destination.label,
              style: PTypography.labelSmall(color: color),
            ),
          ],
        ),
      ),
    );
  }
}

class _PayAction extends StatelessWidget {
  const _PayAction({
    required this.icon,
    required this.label,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusLG),
      child: Column(
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
            style: PTypography.labelSmall(color: AppColors.textSecondary),
          ),
        ],
      ),
    );
  }
}
