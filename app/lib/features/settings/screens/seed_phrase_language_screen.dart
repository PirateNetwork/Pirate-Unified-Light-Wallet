library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/crypto/mnemonic_language.dart';
import '../../../core/i18n/arb_text_localizer.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../providers/preferences_providers.dart';
import '../../../design/deep_space_theme.dart';

class SeedPhraseLanguageScreen extends ConsumerWidget {
  const SeedPhraseLanguageScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(seedPhraseLanguagePreferenceProvider);
    return PScaffold(
      title: 'Seed phrase language'.tr,
      appBar: PAppBar(
        title: 'Seed phrase language'.tr,
        subtitle: 'Choose the language used for seed phrases'.tr,
        showBackButton: true,
      ),
      body: ListView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        children: [
          PCard(
            child: Column(
              children: supportedMnemonicLanguages
                  .map((option) {
                    final isSelected = selected == option;
                    return InkWell(
                      onTap: () {
                        ref
                            .read(seedPhraseLanguagePreferenceProvider.notifier)
                            .setLanguage(option);
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
                                option.nativeLabel,
                                style: AppTypography.body.copyWith(
                                  color: AppColors.textPrimary,
                                ),
                              ),
                            ),
                            RadioGroup(
                              groupValue: selected,
                              onChanged: (_) {
                                ref
                                    .read(
                                      seedPhraseLanguagePreferenceProvider
                                          .notifier,
                                    )
                                    .setLanguage(option);
                              },
                              child: Radio(
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
        ],
      ),
    );
  }
}
