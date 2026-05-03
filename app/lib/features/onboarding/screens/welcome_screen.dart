// Welcome screen - First screen in onboarding

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Welcome screen
class WelcomeScreen extends ConsumerWidget {
  const WelcomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return PScaffold(
      title: 'Pirate Unified Wallet'.tr,
      body: LayoutBuilder(
        builder: (context, constraints) {
          final screenWidth = MediaQuery.of(context).size.width;
          final isMobile = AppSpacing.isMobile(screenWidth);
          final isCompactHeight = constraints.maxHeight < 680;
          final isVeryCompactHeight = constraints.maxHeight < 540;
          final verticalPadding = isCompactHeight
              ? AppSpacing.md
              : AppSpacing.xl;
          final contentPadding = AppSpacing.screenPadding(
            screenWidth,
            vertical: verticalPadding,
          );
          final minContentHeight =
              constraints.maxHeight > contentPadding.vertical
              ? constraints.maxHeight - contentPadding.vertical
              : 0.0;

          final logoSize = isVeryCompactHeight
              ? 56.0
              : (isCompactHeight ? 76.0 : 120.0);
          final titleStyle = isCompactHeight
              ? AppTypography.h2
              : AppTypography.h1;
          final largeGap = isCompactHeight ? AppSpacing.lg : AppSpacing.xxl;
          final mediumGap = isCompactHeight ? AppSpacing.sm : AppSpacing.md;

          return SingleChildScrollView(
            padding: contentPadding,
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: minContentHeight),
              child: Center(
                child: ConstrainedBox(
                  constraints: BoxConstraints(
                    maxWidth: isMobile ? double.infinity : 560,
                  ),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
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
                          size: logoSize,
                          color: AppColors.accentPrimary,
                          semanticLabel: 'Pirate Wallet Logo'.tr,
                        ),
                      ),
                      SizedBox(
                        height: isCompactHeight ? AppSpacing.md : AppSpacing.xl,
                      ),
                      Text(
                        'Pirate Unified Wallet'.tr,
                        style: titleStyle.copyWith(
                          color: AppColors.textPrimary,
                          height: isMobile ? 1.3 : null,
                        ),
                        textAlign: TextAlign.center,
                      ),
                      SizedBox(
                        height: isCompactHeight ? AppSpacing.xs : AppSpacing.md,
                      ),
                      Text(
                        'Privacy made convenient.\nKeys stay on your device.',
                        style: AppTypography.body.copyWith(
                          color: AppColors.textSecondary,
                          height: isMobile ? 1.6 : 1.5,
                        ),
                        textAlign: TextAlign.center,
                      ),
                      SizedBox(height: largeGap),
                      _FeatureItem(
                        icon: Icons.key,
                        title: 'Self-custody'.tr,
                        subtitle: 'Keys stay on your device'.tr,
                        compact: isCompactHeight,
                        hideSubtitle: isVeryCompactHeight,
                      ),
                      SizedBox(height: mediumGap),
                      _FeatureItem(
                        icon: Icons.visibility_off,
                        title: 'Private by default'.tr,
                        subtitle: 'Shielded transactions'.tr,
                        compact: isCompactHeight,
                        hideSubtitle: isVeryCompactHeight,
                      ),
                      SizedBox(height: mediumGap),
                      _FeatureItem(
                        icon: Icons.lock_outline,
                        title: 'Encrypted'.tr,
                        subtitle: 'No telemetry'.tr,
                        compact: isCompactHeight,
                        hideSubtitle: isVeryCompactHeight,
                      ),
                      SizedBox(height: largeGap),
                      PButton(
                        text: 'Get Started',
                        onPressed: () {
                          ref.read(onboardingControllerProvider.notifier)
                            ..reset(startAt: OnboardingStep.welcome)
                            ..nextStep();
                          context.push('/onboarding/create-or-import');
                        },
                        variant: PButtonVariant.primary,
                        size: PButtonSize.large,
                        fullWidth: true,
                      ),
                      SizedBox(
                        height: isCompactHeight ? AppSpacing.xs : AppSpacing.md,
                      ),
                      Text(
                        'By continuing, you agree to the Terms and Privacy Policy'
                            .tr,
                        style: AppTypography.caption.copyWith(
                          color: AppColors.textTertiary,
                        ),
                        textAlign: TextAlign.center,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}

/// Feature item widget
class _FeatureItem extends StatelessWidget {
  final IconData icon;
  final String title;
  final String subtitle;
  final bool compact;
  final bool hideSubtitle;

  const _FeatureItem({
    required this.icon,
    required this.title,
    required this.subtitle,
    this.compact = false,
    this.hideSubtitle = false,
  });

  @override
  Widget build(BuildContext context) {
    final iconPadding = compact ? AppSpacing.xs : AppSpacing.sm;
    final iconSize = compact ? 20.0 : 24.0;

    return Row(
      children: [
        Container(
          padding: EdgeInsets.all(iconPadding),
          decoration: BoxDecoration(
            color: AppColors.accentPrimary.withValues(alpha: 0.1),
            borderRadius: BorderRadius.circular(12),
          ),
          child: Icon(
            icon,
            color: AppColors.accentPrimary,
            size: iconSize,
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
              if (!hideSubtitle)
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
