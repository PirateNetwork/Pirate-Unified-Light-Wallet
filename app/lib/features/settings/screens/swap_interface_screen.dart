/// Swap interface preference screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/i18n/arb_text_localizer.dart';

class SwapInterfaceScreen extends ConsumerWidget {
  const SwapInterfaceScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(swapInterfacePreferenceProvider);

    return PScaffold(
      title: 'Swap interface'.tr,
      appBar: PAppBar(
        title: 'Swap interface'.tr,
        subtitle: 'Choose how trading looks in the Swap screen'.tr,
        showBackButton: true,
      ),
      body: ListView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        children: SwapInterfacePreference.values.map((preference) {
          return Padding(
            padding: const EdgeInsets.only(bottom: AppSpacing.sm),
            child: _SwapInterfaceOption(
              preference: preference,
              selected: selected,
              onTap: () => ref
                  .read(swapInterfacePreferenceProvider.notifier)
                  .setPreference(preference),
            ),
          );
        }).toList(),
      ),
    );
  }
}

class _SwapInterfaceOption extends StatelessWidget {
  const _SwapInterfaceOption({
    required this.preference,
    required this.selected,
    required this.onTap,
  });

  final SwapInterfacePreference preference;
  final SwapInterfacePreference selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final isSelected = preference == selected;
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
                preference == SwapInterfacePreference.simple
                    ? Icons.swap_horiz
                    : Icons.candlestick_chart_outlined,
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
                      preference.label,
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    Text(
                      preference.description,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              RadioGroup<SwapInterfacePreference>(
                groupValue: selected,
                onChanged: (_) => onTap(),
                child: Radio<SwapInterfacePreference>(
                  value: preference,
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
