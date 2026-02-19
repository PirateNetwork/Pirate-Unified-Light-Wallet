import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Buttons Screen
class ShowcaseButtonsScreen extends StatelessWidget {
  const ShowcaseButtonsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Buttons Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Buttons'.tr,
              subtitle: 'All button variants and states'.tr,
            ),
            SizedBox(height: PSpacing.xl),

            // Primary Buttons
            PFormSection(
              title: 'Primary Buttons'.tr,
              description: 'Main call-to-action buttons with gradient'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.small,
                      child: Text('Small'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.medium,
                      child: Text('Medium'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.large,
                      child: Text('Large'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      icon: const Icon(Icons.send),
                      child: Text('With Icon'.tr),
                    ),
                    PButton(onPressed: null, child: Text('Disabled'.tr)),
                    PButton(
                      onPressed: () {},
                      loading: true,
                      child: Text('Loading'.tr),
                    ),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Secondary Buttons
            PFormSection(
              title: 'Secondary Buttons'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.secondary,
                      child: Text('Secondary'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.outline,
                      child: Text('Outline'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.ghost,
                      child: Text('Ghost'.tr),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.danger,
                      child: Text('Danger'.tr),
                    ),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Icon Buttons
            PFormSection(
              title: 'Icon Buttons'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PIconButton(
                      icon: const Icon(Icons.favorite),
                      onPressed: () {},
                      size: PIconButtonSize.small,
                      tooltip: 'Favorite'.tr,
                    ),
                    PIconButton(
                      icon: const Icon(Icons.share),
                      onPressed: () {},
                      size: PIconButtonSize.medium,
                      tooltip: 'Share'.tr,
                    ),
                    PIconButton(
                      icon: const Icon(Icons.settings),
                      onPressed: () {},
                      size: PIconButtonSize.large,
                      tooltip: 'Settings'.tr,
                    ),
                    PIconButton(
                      icon: const Icon(Icons.delete),
                      onPressed: null,
                      tooltip: 'Disabled'.tr,
                    ),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Full Width Button
            PFormSection(
              title: 'Full Width'.tr,
              children: [
                PButton(
                  onPressed: () {},
                  fullWidth: true,
                  child: Text('Full Width Button'.tr),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
