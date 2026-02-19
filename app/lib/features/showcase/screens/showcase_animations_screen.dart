import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/organisms/p_skeleton.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../ui/motion/micro_interactions.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Animations Screen
class ShowcaseAnimationsScreen extends StatelessWidget {
  const ShowcaseAnimationsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Animations Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Animations'.tr,
              subtitle:
                  'Transitions, micro-interactions, and loading states'.tr,
            ),
            SizedBox(height: PSpacing.xl),

            // Micro Interactions
            PFormSection(
              title: 'Micro Interactions'.tr,
              description: 'Hover, press, and state animations'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      child: Text('Hover Me'.tr),
                    ).fadeIn(delay: const Duration(milliseconds: 100)),

                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.secondary,
                      child: Text('Animated Entry'.tr),
                    ).slideInFromBottom(
                      delay: const Duration(milliseconds: 200),
                    ),

                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.outline,
                      child: Text('Pulse Animation'.tr),
                    ).pulse(),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Skeleton Loaders
            PFormSection(
              title: 'Skeleton Loaders'.tr,
              description: 'Loading placeholders with shimmer effect'.tr,
              children: [
                const PSkeletonCard(),
                SizedBox(height: PSpacing.md),
                const PSkeletonListTile(),
                const PSkeletonListTile(),
                const PSkeletonListTile(),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Animated Cards
            PFormSection(
              title: 'Animated Cards'.tr,
              children: [
                PCard(
                  onTap: () {},
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Interactive Card'.tr,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'Hover over this card to see the elevation animation'
                            .tr,
                        style: Theme.of(context).textTheme.bodyMedium,
                      ),
                    ],
                  ),
                ).fadeIn(delay: const Duration(milliseconds: 300)),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
