// Backup Warning Screen - Warn user about seed backup importance

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';

class BackupWarningScreen extends ConsumerStatefulWidget {
  const BackupWarningScreen({super.key});

  @override
  ConsumerState<BackupWarningScreen> createState() => _BackupWarningScreenState();
}

class _BackupWarningScreenState extends ConsumerState<BackupWarningScreen> {
  bool _acknowledged = false;

  void _proceed() {
    if (!_acknowledged) return;
    ref.read(onboardingControllerProvider.notifier).nextStep();
    context.push('/onboarding/seed-display');
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Backup Warning',
      appBar: const PAppBar(
        title: 'Backup Your Seed',
        subtitle: 'Critical security step',
        showBackButton: true,
      ),
      body: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Warning icon
            Container(
              padding: const EdgeInsets.all(AppSpacing.lg),
              decoration: BoxDecoration(
                color: AppColors.warning.withValues(alpha: 0.1),
                shape: BoxShape.circle,
              ),
              child: Icon(
                Icons.warning_amber_rounded,
                size: 64,
                color: AppColors.warning,
              ),
            ),
            const SizedBox(height: AppSpacing.xl),
            
            Text(
              'Your seed phrase is your backup',
              style: AppTypography.h2.copyWith(
                color: AppColors.textPrimary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.md),
            
            Text(
              'If you lose your device or forget your passphrase, your seed phrase is the only way to recover your wallet.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.xl),
            
            // Warning points
            _WarningPoint(
              icon: Icons.lock_outline,
              text: 'Never share your seed phrase with anyone',
            ),
            const SizedBox(height: AppSpacing.md),
            _WarningPoint(
              icon: Icons.visibility_off_outlined,
              text: 'Store it securely offline',
            ),
            const SizedBox(height: AppSpacing.md),
            _WarningPoint(
              icon: Icons.backup_outlined,
              text: 'Write it down or use a hardware wallet',
            ),
            const SizedBox(height: AppSpacing.xxl),
            
            // Acknowledgment checkbox
            InkWell(
              onTap: () => setState(() => _acknowledged = !_acknowledged),
              borderRadius: BorderRadius.circular(8),
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.sm),
                child: Row(
                  children: [
                    Checkbox(
                      value: _acknowledged,
                      onChanged: (value) => setState(() => _acknowledged = value ?? false),
                      activeColor: AppColors.accentPrimary,
                    ),
                    Expanded(
                      child: Text(
                        'I understand that losing my seed phrase means losing access to my wallet forever',
                        style: AppTypography.body.copyWith(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const Spacer(),
            
            PButton(
              text: 'Continue',
              onPressed: _acknowledged ? _proceed : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
            ),
          ],
        ),
      ),
    );
  }
}

class _WarningPoint extends StatelessWidget {
  final IconData icon;
  final String text;

  const _WarningPoint({
    required this.icon,
    required this.text,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(
          icon,
          color: AppColors.warning,
          size: 20,
        ),
        const SizedBox(width: AppSpacing.sm),
        Expanded(
          child: Text(
            text,
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
        ),
      ],
    );
  }
}

