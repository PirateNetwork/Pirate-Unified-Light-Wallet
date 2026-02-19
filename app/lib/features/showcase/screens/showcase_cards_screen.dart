import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/p_list_tile.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Cards Screen
class ShowcaseCardsScreen extends StatelessWidget {
  const ShowcaseCardsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Cards Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Cards & Lists'.tr,
              subtitle: 'Card components and list tiles'.tr,
            ),
            SizedBox(height: PSpacing.xl),

            // Basic Cards
            PFormSection(
              title: 'Basic Cards'.tr,
              children: [
                PCard(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Card Title'.tr,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This is a basic card with some content inside.'.tr,
                        style: Theme.of(context).textTheme.bodyMedium,
                      ),
                    ],
                  ),
                ),
                PCard(
                  elevated: true,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Elevated Card'.tr,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This card has elevated background.'.tr,
                        style: Theme.of(context).textTheme.bodyMedium,
                      ),
                    ],
                  ),
                ),
                PCard(
                  onTap: () {},
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Clickable Card'.tr,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This card is interactive with hover effects.'.tr,
                        style: Theme.of(context).textTheme.bodyMedium,
                      ),
                    ],
                  ),
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // List Tiles
            PFormSection(
              title: 'List Tiles'.tr,
              children: [
                PCard(
                  padding: EdgeInsets.zero,
                  child: Column(
                    children: [
                      PListTile(
                        title: 'List Item 1'.tr,
                        subtitle: 'With leading and trailing icons'.tr,
                        leading: const Icon(Icons.inbox),
                        trailing: const Icon(Icons.chevron_right),
                        onTap: () {},
                      ),
                      PListTile(
                        title: 'List Item 2'.tr,
                        subtitle: 'With subtitle text'.tr,
                        leading: const Icon(Icons.favorite),
                        trailing: const Icon(Icons.chevron_right),
                        onTap: () {},
                      ),
                      PListTile(
                        title: 'Disabled Item'.tr,
                        subtitle: 'This item is disabled'.tr,
                        leading: Icon(Icons.block),
                        enabled: false,
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
