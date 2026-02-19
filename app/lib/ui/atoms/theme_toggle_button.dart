import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../design/tokens/colors.dart';
import '../../features/settings/providers/preferences_providers.dart';
import 'p_icon_button.dart';

/// Clean theme toggle button that switches between light and dark mode
/// Toggles between light and dark (skips system mode for quick switching)
class ThemeToggleButton extends ConsumerWidget {
  const ThemeToggleButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final currentTheme = ref.watch(appThemeModeProvider);
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return PIconButton(
      icon: Icon(
        isDark ? Icons.light_mode_outlined : Icons.dark_mode_outlined,
        color: AppColors.textSecondary,
      ),
      onPressed: () {
        // Toggle between light and dark (skip system mode for quick toggle)
        // If currently on system mode, switch to the opposite of current brightness
        // Otherwise, toggle between light and dark based on current theme mode
        final newMode = currentTheme == AppThemeMode.system
            ? (isDark ? AppThemeMode.light : AppThemeMode.dark)
            : (currentTheme == AppThemeMode.light
                  ? AppThemeMode.dark
                  : AppThemeMode.light);
        ref.read(appThemeModeProvider.notifier).setThemeMode(newMode);
      },
      tooltip: isDark ? 'Switch to light mode' : 'Switch to dark mode',
      size: PIconButtonSize.medium,
    );
  }
}
