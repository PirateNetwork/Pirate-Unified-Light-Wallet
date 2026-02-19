/// Theme settings screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/i18n/arb_text_localizer.dart';

class ThemeScreen extends ConsumerWidget {
  const ThemeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(appThemeModeProvider);

    return PScaffold(
      title: 'Theme'.tr,
      appBar: PAppBar(
        title: 'Theme'.tr,
        subtitle: 'Match your device or pick a fixed look'.tr,
        showBackButton: true,
      ),
      body: Padding(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          children: [
            _ThemeOption(
              mode: AppThemeMode.system,
              selected: selected,
              title: 'System'.tr,
              subtitle: 'Follow device light or dark mode'.tr,
              onTap: () => ref
                  .read(appThemeModeProvider.notifier)
                  .setThemeMode(AppThemeMode.system),
            ),
            const SizedBox(height: AppSpacing.sm),
            _ThemeOption(
              mode: AppThemeMode.dark,
              selected: selected,
              title: 'Dark'.tr,
              subtitle: 'Deep Space'.tr,
              onTap: () => ref
                  .read(appThemeModeProvider.notifier)
                  .setThemeMode(AppThemeMode.dark),
            ),
            const SizedBox(height: AppSpacing.sm),
            _ThemeOption(
              mode: AppThemeMode.light,
              selected: selected,
              title: 'Light'.tr,
              subtitle: 'Ivory'.tr,
              onTap: () => ref
                  .read(appThemeModeProvider.notifier)
                  .setThemeMode(AppThemeMode.light),
            ),
          ],
        ),
      ),
    );
  }
}

class _ThemeOption extends StatelessWidget {
  final AppThemeMode mode;
  final AppThemeMode selected;
  final String title;
  final String subtitle;
  final VoidCallback onTap;

  const _ThemeOption({
    required this.mode,
    required this.selected,
    required this.title,
    required this.subtitle,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final isSelected = mode == selected;
    return PCard(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(16),
        child: Container(
          padding: const EdgeInsets.all(AppSpacing.md),
          decoration: BoxDecoration(
            border: isSelected
                ? Border.all(color: AppColors.accentPrimary, width: 2)
                : null,
            borderRadius: BorderRadius.circular(16),
          ),
          child: Row(
            children: [
              Icon(
                mode == AppThemeMode.light
                    ? Icons.light_mode_outlined
                    : mode == AppThemeMode.dark
                    ? Icons.dark_mode_outlined
                    : Icons.brightness_auto_outlined,
                color: isSelected
                    ? AppColors.accentPrimary
                    : AppColors.textSecondary,
              ),
              const SizedBox(width: AppSpacing.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    Text(
                      subtitle,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              RadioGroup<AppThemeMode>(
                groupValue: selected,
                onChanged: (_) => onTap(),
                child: Radio<AppThemeMode>(
                  value: mode,
                  activeColor: AppColors.accentPrimary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
