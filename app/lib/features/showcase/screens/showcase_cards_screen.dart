import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/p_list_tile.dart';
import '../../../ui/molecules/p_form_section.dart';

/// Showcase Cards Screen
class ShowcaseCardsScreen extends StatelessWidget {
  const ShowcaseCardsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Cards Showcase',
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Cards & Lists',
              subtitle: 'Card components and list tiles',
            ),
            SizedBox(height: PSpacing.xl),
            
            // Basic Cards
            PFormSection(
              title: 'Basic Cards',
              children: [
                PCard(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Card Title',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This is a basic card with some content inside.',
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
                        'Elevated Card',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This card has elevated background.',
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
                        'Clickable Card',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'This card is interactive with hover effects.',
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
              title: 'List Tiles',
              children: [
                PCard(
                  padding: EdgeInsets.zero,
                  child: Column(
                    children: [
                      PListTile(
                        title: 'List Item 1',
                        subtitle: 'With leading and trailing icons',
                        leading: const Icon(Icons.inbox),
                        trailing: const Icon(Icons.chevron_right),
                        onTap: () {},
                      ),
                      PListTile(
                        title: 'List Item 2',
                        subtitle: 'With subtitle text',
                        leading: const Icon(Icons.favorite),
                        trailing: const Icon(Icons.chevron_right),
                        onTap: () {},
                      ),
                      const PListTile(
                        title: 'Disabled Item',
                        subtitle: 'This item is disabled',
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

