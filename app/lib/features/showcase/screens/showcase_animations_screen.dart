import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/organisms/p_skeleton.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../ui/motion/micro_interactions.dart';

/// Showcase Animations Screen
class ShowcaseAnimationsScreen extends StatelessWidget {
  const ShowcaseAnimationsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Animations Showcase',
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Animations',
              subtitle: 'Transitions, micro-interactions, and loading states',
            ),
            SizedBox(height: PSpacing.xl),
            
            // Micro Interactions
            PFormSection(
              title: 'Micro Interactions',
              description: 'Hover, press, and state animations',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      child: const Text('Hover Me'),
                    ).fadeIn(delay: const Duration(milliseconds: 100)),
                    
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.secondary,
                      child: const Text('Animated Entry'),
                    ).slideInFromBottom(delay: const Duration(milliseconds: 200)),
                    
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.outline,
                      child: const Text('Pulse Animation'),
                    ).pulse(),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Skeleton Loaders
            PFormSection(
              title: 'Skeleton Loaders',
              description: 'Loading placeholders with shimmer effect',
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
              title: 'Animated Cards',
              children: [
                PCard(
                  onTap: () {},
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Interactive Card',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      SizedBox(height: PSpacing.sm),
                      Text(
                        'Hover over this card to see the elevation animation',
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

