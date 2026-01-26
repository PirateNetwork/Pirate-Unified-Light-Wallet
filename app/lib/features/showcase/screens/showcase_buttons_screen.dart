import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_form_section.dart';

/// Showcase Buttons Screen
class ShowcaseButtonsScreen extends StatelessWidget {
  const ShowcaseButtonsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Buttons Showcase',
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Buttons',
              subtitle: 'All button variants and states',
            ),
            SizedBox(height: PSpacing.xl),
            
            // Primary Buttons
            PFormSection(
              title: 'Primary Buttons',
              description: 'Main call-to-action buttons with gradient',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.small,
                      child: const Text('Small'),
                    ),
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.medium,
                      child: const Text('Medium'),
                    ),
                    PButton(
                      onPressed: () {},
                      size: PButtonSize.large,
                      child: const Text('Large'),
                    ),
                    PButton(
                      onPressed: () {},
                      icon: const Icon(Icons.send),
                      child: const Text('With Icon'),
                    ),
                    PButton(
                      onPressed: null,
                      child: const Text('Disabled'),
                    ),
                    PButton(
                      onPressed: () {},
                      loading: true,
                      child: const Text('Loading'),
                    ),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Secondary Buttons
            PFormSection(
              title: 'Secondary Buttons',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.secondary,
                      child: const Text('Secondary'),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.outline,
                      child: const Text('Outline'),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.ghost,
                      child: const Text('Ghost'),
                    ),
                    PButton(
                      onPressed: () {},
                      variant: PButtonVariant.danger,
                      child: const Text('Danger'),
                    ),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Icon Buttons
            PFormSection(
              title: 'Icon Buttons',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PIconButton(
                      icon: const Icon(Icons.favorite),
                      onPressed: () {},
                      size: PIconButtonSize.small,
                      tooltip: 'Favorite',
                    ),
                    PIconButton(
                      icon: const Icon(Icons.share),
                      onPressed: () {},
                      size: PIconButtonSize.medium,
                      tooltip: 'Share',
                    ),
                    PIconButton(
                      icon: const Icon(Icons.settings),
                      onPressed: () {},
                      size: PIconButtonSize.large,
                      tooltip: 'Settings',
                    ),
                    PIconButton(
                      icon: const Icon(Icons.delete),
                      onPressed: null,
                      tooltip: 'Disabled',
                    ),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Full Width Button
            PFormSection(
              title: 'Full Width',
              children: [
                PButton(
                  onPressed: () {},
                  fullWidth: true,
                  child: const Text('Full Width Button'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

