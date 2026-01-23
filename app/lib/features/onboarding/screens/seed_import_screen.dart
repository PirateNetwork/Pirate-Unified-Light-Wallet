/// Seed Import Screen - Restore wallet from 24-word mnemonic

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_icon_button.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/screenshot_protection.dart';
import '../onboarding_flow.dart';

/// Seed import screen for wallet restoration
class SeedImportScreen extends ConsumerStatefulWidget {
  const SeedImportScreen({super.key});

  @override
  ConsumerState<SeedImportScreen> createState() => _SeedImportScreenState();
}

class _SeedImportScreenState extends ConsumerState<SeedImportScreen> {
  final _formKey = GlobalKey<FormState>();
  final List<TextEditingController> _wordControllers =
      List.generate(24, (_) => TextEditingController());
  final List<FocusNode> _focusNodes =
      List.generate(24, (_) => FocusNode());
  ScreenProtection? _screenProtection;

  bool _isValidating = false;
  String? _validationError;
  int _wordCount = 24;

  @override
  void initState() {
    super.initState();
    _disableScreenshots();
    // Add listeners to update button state when text changes
    for (final controller in _wordControllers) {
      controller.addListener(_onTextChanged);
    }
  }

  @override
  void dispose() {
    for (final controller in _wordControllers) {
      controller.removeListener(_onTextChanged);
      controller.dispose();
    }
    for (final node in _focusNodes) {
      node.dispose();
    }
    _enableScreenshots();
    super.dispose();
  }

  void _disableScreenshots() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _enableScreenshots() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  void _onTextChanged() {
    setState(() {}); // Rebuild to update button state
  }

  String get _mnemonic {
    return _wordControllers
        .take(_wordCount)
        .map((c) => c.text.trim().toLowerCase())
        .join(' ');
  }

  bool get _isComplete {
    return _wordControllers
        .take(_wordCount)
        .every((c) => c.text.trim().isNotEmpty);
  }

  Future<void> _validateAndProceed() async {
    if (!_isComplete) return;

    setState(() {
      _isValidating = true;
      _validationError = null;
    });

    try {
      // Validate mnemonic via FFI
      final isValid = await FfiBridge.validateMnemonic(_mnemonic);
      
      if (!isValid) {
        setState(() {
          _validationError = 'Invalid seed phrase. Check the words and order.';
          _isValidating = false;
        });
        return;
      }

      // Store mnemonic in onboarding state
      ref.read(onboardingControllerProvider.notifier).setMnemonic(_mnemonic);
      ref.read(onboardingControllerProvider.notifier).nextStep();
      
      if (mounted) {
        context.push('/onboarding/birthday');
      }
    } catch (e) {
      setState(() {
        _validationError = 'Could not validate the phrase. Try again.';
        _isValidating = false;
      });
    }
  }

  Future<void> _pasteFromClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    if (data?.text == null) return;

    final words = data!.text!.trim().split(RegExp(r'\s+'));
    
    // Determine word count from pasted content
    if (words.length == 12) {
      setState(() => _wordCount = 12);
    } else if (words.length >= 24) {
      setState(() => _wordCount = 24);
    }

    // Fill in word fields
    for (int i = 0; i < _wordCount && i < words.length; i++) {
      _wordControllers[i].text = words[i].toLowerCase();
    }

    setState(() {});
  }

  void _clearAll() {
    for (final controller in _wordControllers) {
      controller.clear();
    }
    setState(() {
      _validationError = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final gutter = AppSpacing.responsiveGutter(MediaQuery.of(context).size.width);
    final viewInsets = MediaQuery.of(context).viewInsets.bottom;
    return PScaffold(
      title: 'Import Seed',
      appBar: PAppBar(
        title: 'Import Seed Phrase',
        subtitle: 'Restore a wallet with your seed phrase',
        onBack: () => context.pop(),
        centerTitle: true,
        actions: [
          PIconButton(
            icon: const Icon(Icons.content_paste),
            tooltip: 'Paste from clipboard',
            onPressed: _pasteFromClipboard,
          ),
        ],
      ),
      body: Column(
        children: [
          // Progress indicator
          Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              AppSpacing.lg,
              gutter,
              AppSpacing.lg,
            ),
            child: _ProgressIndicator(currentStep: 2, totalSteps: 5),
          ),

          Expanded(
            child: SingleChildScrollView(
              padding: EdgeInsets.symmetric(horizontal: gutter),
              child: Form(
                key: _formKey,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    // Title
                    Text(
                      'Enter your seed phrase',
                      style: AppTypography.h2.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),

                    const SizedBox(height: AppSpacing.sm),

                    Text(
                      'Enter the ${_wordCount} words in order.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),

                    const SizedBox(height: AppSpacing.md),

                    // Word count toggle
                    Row(
                      children: [
                        _WordCountChip(
                          label: '12 words',
                          selected: _wordCount == 12,
                          onTap: () => setState(() => _wordCount = 12),
                        ),
                        const SizedBox(width: AppSpacing.sm),
                        _WordCountChip(
                          label: '24 words',
                          selected: _wordCount == 24,
                          onTap: () => setState(() => _wordCount = 24),
                        ),
                        const Spacer(),
                        PTextButton(
                          label: 'Clear all',
                          leadingIcon: Icons.clear,
                          variant: PTextButtonVariant.subtle,
                          onPressed: _clearAll,
                        ),
                      ],
                    ),

                    const SizedBox(height: AppSpacing.lg),

                    // Word grid
                    GridView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      gridDelegate:
                          const SliverGridDelegateWithFixedCrossAxisCount(
                        crossAxisCount: 3,
                        childAspectRatio: 2.5,
                        crossAxisSpacing: AppSpacing.sm,
                        mainAxisSpacing: AppSpacing.sm,
                      ),
                      itemCount: _wordCount,
                      itemBuilder: (context, index) {
                        return _WordInput(
                          index: index,
                          controller: _wordControllers[index],
                          focusNode: _focusNodes[index],
                          onSubmitted: () {
                            if (index < _wordCount - 1) {
                              _focusNodes[index + 1].requestFocus();
                            } else {
                              _focusNodes[index].unfocus();
                              _validateAndProceed();
                            }
                          },
                        );
                      },
                    ),

                    const SizedBox(height: AppSpacing.lg),

                    // Error message
                    if (_validationError != null)
                      Container(
                        padding: const EdgeInsets.all(AppSpacing.md),
                        decoration: BoxDecoration(
                          color: AppColors.error.withValues(alpha: 0.1),
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(
                            color: AppColors.error.withValues(alpha: 0.3),
                          ),
                        ),
                        child: Row(
                          children: [
                            Icon(
                              Icons.error_outline,
                              color: AppColors.error,
                              size: 20,
                            ),
                            const SizedBox(width: AppSpacing.sm),
                            Expanded(
                              child: Text(
                                _validationError!,
                                style: AppTypography.body.copyWith(
                                  color: AppColors.error,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),

                    const SizedBox(height: AppSpacing.lg),

                    // Security notice
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
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Icon(
                            Icons.security,
                            color: AppColors.warning,
                            size: 20,
                          ),
                          const SizedBox(width: AppSpacing.sm),
                          Expanded(
                            child: Text(
                              'Keep this private. Anyone with your seed phrase '
                              'can access your funds.',
                              style: AppTypography.caption.copyWith(
                                color: AppColors.textPrimary,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),

                    const SizedBox(height: AppSpacing.xl),
                  ],
                ),
              ),
            ),
          ),

          // Continue button
          Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              AppSpacing.lg,
              gutter,
              AppSpacing.lg + viewInsets,
            ),
            child: PButton(
              text: _isValidating ? 'Validating...' : 'Continue',
              onPressed: _isComplete && !_isValidating
                  ? _validateAndProceed
                  : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isValidating,
            ),
          ),
        ],
      ),
    );
  }
}

/// Word input field
class _WordInput extends StatelessWidget {
  final int index;
  final TextEditingController controller;
  final FocusNode focusNode;
  final VoidCallback onSubmitted;

  const _WordInput({
    required this.index,
    required this.controller,
    required this.focusNode,
    required this.onSubmitted,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: AppColors.surfaceElevated,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppColors.border),
      ),
      child: Row(
        children: [
          // Word number
          Container(
            width: 28,
            alignment: Alignment.center,
            child: Text(
              '${index + 1}',
              style: AppTypography.caption.copyWith(
                color: AppColors.textTertiary,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          // Word input
          Expanded(
            child: TextField(
              controller: controller,
              focusNode: focusNode,
              style: AppTypography.body.copyWith(
                color: AppColors.textPrimary,
                fontSize: 14,
              ),
              decoration: const InputDecoration(
                border: InputBorder.none,
                contentPadding: EdgeInsets.symmetric(
                  horizontal: 4,
                  vertical: 8,
                ),
                isDense: true,
              ),
              autocorrect: false,
              enableSuggestions: false,
              textInputAction: TextInputAction.next,
              onSubmitted: (_) => onSubmitted(),
            ),
          ),
        ],
      ),
    );
  }
}

/// Word count selection chip
class _WordCountChip extends StatelessWidget {
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _WordCountChip({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    // Use smaller padding on desktop to match mobile size
    final isDesktop = MediaQuery.of(context).size.width > 600;
    final horizontalPadding = isDesktop ? AppSpacing.sm : AppSpacing.md;
    final verticalPadding = isDesktop ? AppSpacing.xs : AppSpacing.sm;
    
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: EdgeInsets.symmetric(
          horizontal: horizontalPadding,
          vertical: verticalPadding,
        ),
        decoration: BoxDecoration(
          color: selected 
              ? AppColors.accentPrimary.withValues(alpha: 0.1) 
              : AppColors.surfaceElevated,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: selected 
                ? AppColors.accentPrimary 
                : AppColors.border,
          ),
        ),
        child: Text(
          label,
          style: AppTypography.caption.copyWith(
            color: selected 
                ? AppColors.accentPrimary 
                : AppColors.textSecondary,
            fontWeight: selected ? FontWeight.w600 : FontWeight.normal,
          ),
        ),
      ),
    );
  }
}

/// Progress indicator (shared widget)
class _ProgressIndicator extends StatelessWidget {
  final int currentStep;
  final int totalSteps;

  const _ProgressIndicator({
    required this.currentStep,
    required this.totalSteps,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Row(
          children: [
            Expanded(
              child: ClipRRect(
                borderRadius: BorderRadius.circular(4),
                child: LinearProgressIndicator(
                  value: currentStep / totalSteps,
                  backgroundColor: AppColors.surfaceElevated,
                  valueColor: AlwaysStoppedAnimation<Color>(
                    AppColors.accentPrimary,
                  ),
                  minHeight: 4,
                  semanticsLabel: 'Onboarding progress',
                  semanticsValue: '${(currentStep / totalSteps * 100).round()}%',
                ),
              ),
            ),
          ],
        ),
        const SizedBox(height: AppSpacing.xs),
        Row(
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            Text(
              'Step $currentStep of $totalSteps',
              style: AppTypography.caption.copyWith(
                color: AppColors.textTertiary,
              ),
            ),
          ],
        ),
      ],
    );
  }
}
