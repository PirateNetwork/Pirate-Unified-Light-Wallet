// Welcome screen - First screen in onboarding

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../../../core/i18n/arb_text_localizer.dart';
import '../../legal/privacy_policy_dialog.dart';

/// Welcome screen
class WelcomeScreen extends ConsumerStatefulWidget {
  const WelcomeScreen({super.key});

  @override
  ConsumerState<WelcomeScreen> createState() => _WelcomeScreenState();
}

class _WelcomeScreenState extends ConsumerState<WelcomeScreen> {
  bool _isContinuing = false;

  void _continueToOnboarding() {
    if (_isContinuing) return;

    setState(() => _isContinuing = true);
    ref.read(onboardingControllerProvider.notifier)
      ..reset(startAt: OnboardingStep.welcome)
      ..nextStep();
    context.go('/onboarding/create-or-import');
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Stashi Wallet',
      body: LayoutBuilder(
        builder: (context, constraints) {
          final screenSize = MediaQuery.sizeOf(context);
          final screenWidth = screenSize.width;
          final isMobile = AppSpacing.isHandset(screenSize);
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
                      Semantics(
                        key: const Key('welcome_logo'),
                        image: true,
                        label: 'Stashi Wallet logo'.tr,
                        child: Image.asset(
                          'assets/icons/stashi-wallet-logo.png',
                          width: logoSize,
                          height: logoSize,
                          fit: BoxFit.contain,
                          excludeFromSemantics: true,
                        ),
                      ),
                      SizedBox(
                        height: isCompactHeight ? AppSpacing.md : AppSpacing.xl,
                      ),
                      Text(
                        'Stashi Wallet',
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
                        'Privacy made convenient'.tr,
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
                        title: 'Always private'.tr,
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
                        text: 'Get started'.tr,
                        onPressed: _continueToOnboarding,
                        loading: _isContinuing,
                        variant: PButtonVariant.primary,
                        size: PButtonSize.large,
                        fullWidth: true,
                      ),
                      SizedBox(
                        height: isCompactHeight ? AppSpacing.xs : AppSpacing.md,
                      ),
                      const PrivacyPolicyAgreement(),
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
