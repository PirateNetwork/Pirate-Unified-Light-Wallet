/// Welcome screen - First screen in onboarding

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';

/// Welcome screen
class WelcomeScreen extends ConsumerWidget {
  const WelcomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return PScaffold(
      title: 'Pirate Unified Wallet',
      body: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Spacer(),
            // Logo/Brand
            TweenAnimationBuilder<double>(
              tween: Tween(begin: 0.0, end: 1.0),
              duration: const Duration(milliseconds: 800),
              curve: Curves.easeOut,
              builder: (context, value, child) {
                return Opacity(
                  opacity: value,
                  child: Transform.scale(
                    scale: 0.8 + (0.2 * value),
                    child: child,
                  ),
                );
              },
              child: Icon(
                Icons.shield_outlined,
                size: 120,
                color: AppColors.accentPrimary,
                semanticLabel: 'Pirate Wallet Logo',
              ),
            ),
            const SizedBox(height: AppSpacing.xl),

            // Title
            Text(
              'Pirate Unified Wallet',
              style: AppTypography.h1.copyWith(
                color: AppColors.textPrimary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.md),

            // Subtitle
            Text(
              'Privacy made convenient.\nKeys stay on your device.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
                height: 1.5,
              ),
              textAlign: TextAlign.center,
            ),

            const Spacer(),

            // Features list
            _FeatureItem(
              icon: Icons.key,
              title: 'Self-custody',
              subtitle: 'Keys stay on your device',
            ),
            const SizedBox(height: AppSpacing.md),
            _FeatureItem(
              icon: Icons.visibility_off,
              title: 'Private by default',
              subtitle: 'Shielded transactions',
            ),
            const SizedBox(height: AppSpacing.md),
            _FeatureItem(
              icon: Icons.lock_outline,
              title: 'Encrypted',
              subtitle: 'No telemetry',
            ),

            const Spacer(),

            // Get Started button
            PButton(
              text: 'Get Started',
              onPressed: () {
                ref.read(onboardingControllerProvider.notifier).nextStep();
                context.push('/onboarding/create-or-import');
              },
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
            ),
            const SizedBox(height: AppSpacing.md),

            // Legal notice
            Text(
              'By continuing, you agree to the Terms and Privacy Policy',
              style: AppTypography.caption.copyWith(
                color: AppColors.textTertiary,
              ),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}

/// Feature item widget
class _FeatureItem extends StatelessWidget {
  final IconData icon;
  final String title;
  final String subtitle;

  const _FeatureItem({
    required this.icon,
    required this.title,
    required this.subtitle,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Container(
          padding: const EdgeInsets.all(AppSpacing.sm),
          decoration: BoxDecoration(
            color: AppColors.accentPrimary.withValues(alpha: 0.1),
            borderRadius: BorderRadius.circular(12),
          ),
          child: Icon(
            icon,
            color: AppColors.accentPrimary,
            size: 24,
            semanticLabel: title,
          ),
        ),
        const SizedBox(width: AppSpacing.md),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                title,
                style: AppTypography.bodyBold.copyWith(
                  color: AppColors.textPrimary,
                ),
              ),
              Text(
                subtitle,
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

