// Seed Display Screen - Show the generated seed phrase to user

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/screenshot_protection.dart';
import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../widgets/onboarding_progress_indicator.dart';

class SeedDisplayScreen extends ConsumerStatefulWidget {
  const SeedDisplayScreen({super.key});

  @override
  ConsumerState<SeedDisplayScreen> createState() => _SeedDisplayScreenState();
}

class _SeedDisplayScreenState extends ConsumerState<SeedDisplayScreen> {
  bool _seedRevealed = false;
  String? _mnemonic;
  bool _isLoading = false;
  ScreenProtection? _screenProtection;

  @override
  void initState() {
    super.initState();
    _loadMnemonic();
  }

  @override
  void dispose() {
    _enableScreenshots();
    super.dispose();
  }

  Future<void> _loadMnemonic() async {
    setState(() => _isLoading = true);
    
    // Generate mnemonic (same as what create_wallet will use)
    // We generate it here so we can show it before creating the wallet
    try {
      final mnemonic = await FfiBridge.generateMnemonic(wordCount: 24);
      if (mounted) {
        setState(() {
          _mnemonic = mnemonic;
          _isLoading = false;
        });
        // Store in onboarding state so we can use it when creating wallet
        ref.read(onboardingControllerProvider.notifier).setMnemonic(mnemonic);
      }
    } catch (e) {
      if (mounted) {
        setState(() => _isLoading = false);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to generate seed: $e'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  Future<void> _copyToClipboard() async {
    if (_mnemonic == null) return;
    await Clipboard.setData(ClipboardData(text: _mnemonic!));
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: const Text('Seed phrase copied to clipboard'),
          backgroundColor: AppColors.success,
          behavior: SnackBarBehavior.floating,
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }

  void _revealSeed() {
    _disableScreenshots();
    setState(() => _seedRevealed = true);
  }

  void _proceed() {
    if (!_seedRevealed) return;
    _enableScreenshots();
    ref.read(onboardingControllerProvider.notifier).nextStep();
    context.push('/onboarding/seed-confirm');
  }

  void _disableScreenshots() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _enableScreenshots() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Your Seed Phrase',
      appBar: const PAppBar(
        title: 'Backup Your Seed',
        subtitle: 'Write this down securely',
        showBackButton: true,
        centerTitle: true,
      ),
      body: _isLoading
          ? const Center(child: CircularProgressIndicator())
          : SingleChildScrollView(
              padding: AppSpacing.screenPadding(
                MediaQuery.of(context).size.width,
                vertical: AppSpacing.xl,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const OnboardingProgressIndicator(
                    currentStep: 5,
                    totalSteps: 6,
                  ),
                  const SizedBox(height: AppSpacing.xxl),
                  Text(
                    'Write down these 24 words',
                    style: AppTypography.h2.copyWith(
                      color: AppColors.textPrimary,
                    ),
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  Text(
                    'Store them in a safe place. Anyone with these words can access your wallet.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: AppSpacing.xl),
                  
                  if (!_seedRevealed) ...[
                    // Hidden seed - show reveal button
                    Container(
                      padding: const EdgeInsets.all(AppSpacing.xl),
                      decoration: BoxDecoration(
                        color: AppColors.backgroundSurface,
                        borderRadius: BorderRadius.circular(16),
                        border: Border.all(
                          color: AppColors.borderDefault,
                        ),
                      ),
                      child: Column(
                        children: [
                          Icon(
                            Icons.visibility_off,
                            size: 48,
                            color: AppColors.textSecondary,
                          ),
                          const SizedBox(height: AppSpacing.md),
                          Text(
                            'Tap to reveal your seed phrase',
                            style: AppTypography.body.copyWith(
                              color: AppColors.textSecondary,
                            ),
                          ),
                          const SizedBox(height: AppSpacing.lg),
                          PButton(
                            text: 'Reveal Seed Phrase',
                            onPressed: _revealSeed,
                            variant: PButtonVariant.primary,
                            size: PButtonSize.medium,
                          ),
                        ],
                      ),
                    ),
                  ] else ...[
                    // Revealed seed - show words
                    Container(
                      padding: const EdgeInsets.all(AppSpacing.lg),
                      decoration: BoxDecoration(
                        color: AppColors.backgroundSurface,
                        borderRadius: BorderRadius.circular(16),
                        border: Border.all(
                          color: AppColors.borderDefault,
                        ),
                      ),
                      child: _mnemonic == null
                          ? const Center(child: CircularProgressIndicator())
                          : _SeedGrid(words: _mnemonic!.split(' ')),
                    ),
                    const SizedBox(height: AppSpacing.md),
                    PButton(
                      text: 'Copy to Clipboard',
                      onPressed: _copyToClipboard,
                      variant: PButtonVariant.secondary,
                      size: PButtonSize.medium,
                    ),
                  ],
                  
                  const SizedBox(height: AppSpacing.xl),
                  
                  if (_seedRevealed) ...[
                    Container(
                      padding: const EdgeInsets.all(AppSpacing.md),
                      decoration: BoxDecoration(
                        color: AppColors.warning.withValues(alpha: 0.1),
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(
                          color: AppColors.warning.withValues(alpha: 0.3),
                        ),
                      ),
                      child: Row(
                        children: [
                          Icon(
                            Icons.warning_amber_rounded,
                            color: AppColors.warning,
                            size: 20,
                          ),
                          const SizedBox(width: AppSpacing.sm),
                          Expanded(
                            child: Text(
                              "Make sure you've written it down before continuing",
                              style: AppTypography.caption.copyWith(
                                color: AppColors.textPrimary,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(height: AppSpacing.lg),
                    PButton(
                      text: "I've Written It Down",
                      onPressed: _proceed,
                      variant: PButtonVariant.primary,
                      size: PButtonSize.large,
                    ),
                  ],
                ],
              ),
            ),
    );
  }
}

class _SeedGrid extends StatelessWidget {
  final List<String> words;

  const _SeedGrid({required this.words});

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final isNarrow = constraints.maxWidth < 360;
        final crossAxisCount = isNarrow ? 2 : 3;
        final aspectRatio = isNarrow ? 2.1 : 2.5;
        return GridView.builder(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: crossAxisCount,
            crossAxisSpacing: AppSpacing.sm,
            mainAxisSpacing: AppSpacing.sm,
            childAspectRatio: aspectRatio,
          ),
          itemCount: words.length,
          itemBuilder: (context, index) {
            return Container(
              padding: const EdgeInsets.symmetric(
                horizontal: AppSpacing.sm,
                vertical: AppSpacing.xs,
              ),
              decoration: BoxDecoration(
                color: AppColors.backgroundSurface,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: AppColors.borderDefault,
                ),
              ),
              child: Row(
                children: [
                  Text(
                    '${index + 1}.',
                    style: AppTypography.caption.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  const SizedBox(width: AppSpacing.xs),
                  Expanded(
                    child: Text(
                      words[index],
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.textPrimary,
                      ),
                      maxLines: 2,
                      softWrap: true,
                      overflow: TextOverflow.fade,
                    ),
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}
