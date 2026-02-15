import 'dart:io';
import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Navigation
/// - BottomNavigationBar for mobile
/// - NavigationRail + AppSidebar for desktop
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

  bool get _isDesktop => Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  int? get _resolvedPayIndex {
    final explicit = payIndex;
    if (explicit != null) return explicit;
    final inferred = destinations.indexWhere((dest) => dest.isPay);
    return inferred >= 0 ? inferred : null;
  }

  @override
  Widget build(BuildContext context) {
    if (_isDesktop) {
      return NavigationRail(
        selectedIndex: currentIndex,
        onDestinationSelected: onDestinationSelected,
        scrollable: true,
        backgroundColor: AppColors.backgroundSurface,
        indicatorColor: AppColors.selectedBackground,
        labelType: NavigationRailLabelType.all,
        destinations: destinations
            .map(
              (dest) => NavigationRailDestination(
                icon: Icon(dest.icon),
                selectedIcon: Icon(dest.selectedIcon ?? dest.icon),
                label: Text(dest.label),
              ),
            )
            .toList(),
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
        padding: const EdgeInsets.symmetric(horizontal: PSpacing.md, vertical: PSpacing.sm),
        child: Row(
          children: [
            ...left.map(
              (dest) => Expanded(
                child: _NavItem(
                  destination: dest,
                  isSelected: destinations.indexOf(dest) == currentIndex,
                  onTap: () => onDestinationSelected(destinations.indexOf(dest)),
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
                  onTap: () => onDestinationSelected(destinations.indexOf(dest)),
                ),
              ),
            ),
          ],
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
