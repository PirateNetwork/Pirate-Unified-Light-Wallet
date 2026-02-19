/// Currency settings screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../features/settings/providers/preferences_providers.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/i18n/arb_text_localizer.dart';

class CurrencyScreen extends ConsumerWidget {
  const CurrencyScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(currencyPreferenceProvider);

    return PScaffold(
      title: 'Currency'.tr,
      appBar: PAppBar(
        title: 'Currency'.tr,
        subtitle: 'Choose the default display currency'.tr,
        showBackButton: true,
      ),
      body: ListView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        children: CurrencyPreference.values.map((preference) {
          return Padding(
            padding: const EdgeInsets.only(bottom: AppSpacing.sm),
            child: _CurrencyOption(
              preference: preference,
              selected: selected,
              onTap: () => ref
                  .read(currencyPreferenceProvider.notifier)
                  .setCurrency(preference),
            ),
          );
        }).toList(),
      ),
    );
  }
}

class _CurrencyOption extends StatelessWidget {
  final CurrencyPreference preference;
  final CurrencyPreference selected;
  final VoidCallback onTap;

  const _CurrencyOption({
    required this.preference,
    required this.selected,
    required this.onTap,
  });

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
                Icons.currency_exchange,
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
                      preference.code,
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    Text(
                      preference.label,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              RadioGroup<CurrencyPreference>(
                groupValue: selected,
                onChanged: (_) => onTap(),
                child: Radio<CurrencyPreference>(
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
