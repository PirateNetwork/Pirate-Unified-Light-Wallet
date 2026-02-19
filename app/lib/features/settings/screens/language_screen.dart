library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../l10n/app_localizations.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../providers/preferences_providers.dart';

class LanguageScreen extends ConsumerWidget {
  const LanguageScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final selected = ref.watch(localePreferenceProvider);
    return PScaffold(
      title: l10n.languageTitle,
      appBar: PAppBar(
        title: l10n.languageTitle,
        subtitle: l10n.languageSubtitle,
        showBackButton: true,
      ),
      body: ListView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        children: [
          PCard(
            child: Column(
              children: AppLocalePreference.values
                  .map((option) {
                    final label = option == AppLocalePreference.english
                        ? l10n.englishLanguage
                        : option.label;
                    final isSelected = selected == option;
                    return InkWell(
                      onTap: () {
                        ref
                            .read(localePreferenceProvider.notifier)
                            .setLocale(option);
                      },
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                          horizontal: AppSpacing.md,
                          vertical: AppSpacing.sm,
                        ),
                        child: Row(
                          children: [
                            Expanded(
                              child: Text(
                                label,
                                style: AppTypography.body.copyWith(
                                  color: AppColors.textPrimary,
                                ),
                              ),
                            ),
                            RadioGroup<AppLocalePreference>(
                              groupValue: selected,
                              onChanged: (_) {
                                ref
                                    .read(localePreferenceProvider.notifier)
                                    .setLocale(option);
                              },
                              child: Radio<AppLocalePreference>(
                                value: option,
                                activeColor: AppColors.accentPrimary,
                              ),
                            ),
                            if (isSelected)
                              Icon(
                                Icons.check_circle,
                                color: AppColors.accentPrimary,
                                size: 18,
                              ),
                          ],
                        ),
                      ),
                    );
                  })
                  .toList(growable: false),
            ),
          ),
          const SizedBox(height: AppSpacing.md),
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Text(
                l10n.moreLanguagesSoon,
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
