import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Home Screen - Landing page for design system showcase
class ShowcaseHomeScreen extends StatelessWidget {
  const ShowcaseHomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Design System Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Pirate Wallet Design System'.tr,
              subtitle: 'World-class components for premium dark UI'.tr,
            ),
            SizedBox(height: PSpacing.xl),
            _buildGrid(context),
          ],
        ),
      ),
    );
  }

  Widget _buildGrid(BuildContext context) {
    return Wrap(
      spacing: PSpacing.md,
      runSpacing: PSpacing.md,
      children: [
        _ShowcaseCard(
          title: 'Buttons'.tr,
          description: 'Primary, secondary, outline, ghost variants'.tr,
          icon: Icons.smart_button,
          onTap: () => context.push('/buttons'),
        ),
        _ShowcaseCard(
          title: 'Forms'.tr,
          description: 'Inputs, checkboxes, radios, toggles'.tr,
          icon: Icons.edit_note,
          onTap: () => context.push('/forms'),
        ),
        _ShowcaseCard(
          title: 'Cards'.tr,
          description: 'Cards, list tiles, form sections'.tr,
          icon: Icons.view_module,
          onTap: () => context.push('/cards'),
        ),
        _ShowcaseCard(
          title: 'Dialogs'.tr,
          description: 'Modals, bottom sheets, snackbars'.tr,
          icon: Icons.crop_square,
          onTap: () => context.push('/dialogs'),
        ),
        _ShowcaseCard(
          title: 'Animations'.tr,
          description: 'Transitions, micro-interactions, skeletons'.tr,
          icon: Icons.animation,
          onTap: () => context.push('/animations'),
        ),
      ],
    );
  }
}

class _ShowcaseCard extends StatelessWidget {
  const _ShowcaseCard({
    required this.title,
    required this.description,
    required this.icon,
    required this.onTap,
  });

  final String title;
  final String description;
  final IconData icon;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 300,
      height: 200,
      child: PCard(
        onTap: onTap,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              width: 48,
              height: 48,
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  colors: [AppColors.gradientAStart, AppColors.gradientAEnd],
                ),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
              ),
              child: Icon(icon, color: AppColors.textOnAccent),
            ),
            SizedBox(height: PSpacing.md),
            Text(title, style: Theme.of(context).textTheme.titleLarge),
            SizedBox(height: PSpacing.xs),
            Text(description, style: Theme.of(context).textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}
