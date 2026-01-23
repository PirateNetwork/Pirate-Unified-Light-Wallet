/// Seed Confirm Screen - Verify user has backed up their seed phrase

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/colors.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../../../core/providers/wallet_providers.dart';

class SeedConfirmScreen extends ConsumerStatefulWidget {
  const SeedConfirmScreen({super.key});

  @override
  ConsumerState<SeedConfirmScreen> createState() => _SeedConfirmScreenState();
}

class _SeedConfirmScreenState extends ConsumerState<SeedConfirmScreen> {
  final List<TextEditingController> _wordControllers =
      List.generate(3, (_) => TextEditingController());
  List<int> _selectedIndices = [];
  bool _isVerifying = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _selectRandomWords();
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
    super.dispose();
  }

  void _onTextChanged() {
    setState(() {}); // Rebuild to update button state
  }

  void _selectRandomWords() {
    // Select 3 random word positions (1-24)
    final random = DateTime.now().millisecondsSinceEpoch;
    final randomGenerator = (random % 1000000).toInt();
    _selectedIndices = [];
    final used = <int>{};
    
    while (_selectedIndices.length < 3) {
      final index = ((randomGenerator + _selectedIndices.length * 7) % 24) + 1;
      if (!used.contains(index)) {
        _selectedIndices.add(index);
        used.add(index);
      }
      if (used.length >= 24) break; // Safety check
    }
    _selectedIndices.sort();
    setState(() {});
  }

  bool get _isComplete {
    return _wordControllers.every((c) => c.text.trim().isNotEmpty);
  }

  Future<void> _verifyAndProceed() async {
    if (!_isComplete) return;

    setState(() {
      _isVerifying = true;
      _error = null;
    });

    try {
      final state = ref.read(onboardingControllerProvider);
      final mnemonic = state.mnemonic;
      
      if (mnemonic == null || mnemonic.isEmpty) {
        throw StateError('Mnemonic not found in onboarding state');
      }

      final words = mnemonic.split(' ');
      
      // Verify each word
      for (int i = 0; i < _selectedIndices.length; i++) {
        final index = _selectedIndices[i] - 1; // Convert to 0-based
        final expectedWord = words[index].toLowerCase().trim();
        final enteredWord = _wordControllers[i].text.toLowerCase().trim();
        
        if (expectedWord != enteredWord) {
          setState(() {
            _error = 'Word ${_selectedIndices[i]} is incorrect. Please check and try again.';
            _isVerifying = false;
          });
          return;
        }
      }

      // Verification successful
      ref.read(onboardingControllerProvider.notifier).markSeedBackedUp();
      ref.read(onboardingControllerProvider.notifier).nextStep();
      
      // Create wallet with the mnemonic we generated
      await ref.read(onboardingControllerProvider.notifier).complete('My Pirate Wallet');
      
      if (mounted) {
        // Invalidate walletsExistProvider to refresh it after wallet creation
        ref.invalidate(walletsExistProvider);
        // Ensure app is marked as unlocked
        ref.read(appUnlockedProvider.notifier).setUnlocked(true);
        // Navigate to home
        context.go('/home');
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Wallet created. Syncing...'),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    } catch (e) {
      setState(() {
        _error = 'Verification failed: $e';
        _isVerifying = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final basePadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: AppSpacing.xl,
    );
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return PScaffold(
      title: 'Confirm Seed',
      appBar: const PAppBar(
        title: 'Verify Your Backup',
        subtitle: 'Confirm you wrote it down',
        showBackButton: true,
      ),
      body: Padding(
        padding: contentPadding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Enter these words from your seed phrase',
              style: AppTypography.h2.copyWith(
                color: AppColors.textPrimary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.sm),
            Text(
              'This confirms you\'ve written down your seed phrase correctly.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.xl),
            
            // Word inputs
            ...List.generate(3, (i) {
              return Padding(
                padding: const EdgeInsets.only(bottom: AppSpacing.md),
                child: PInput(
                  controller: _wordControllers[i],
                  label: 'Word ${_selectedIndices[i]}',
                  hint: 'Enter word ${_selectedIndices[i]}',
                  textInputAction: i < 2 ? TextInputAction.next : TextInputAction.done,
                  onSubmitted: i < 2 ? null : (_) => _verifyAndProceed(),
                  autofocus: i == 0,
                ),
              );
            }),
            
            if (_error != null) ...[
              const SizedBox(height: AppSpacing.md),
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
                        _error!,
                        style: AppTypography.body.copyWith(
                          color: AppColors.error,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
            
            const SizedBox(height: AppSpacing.xl),
            
            PButton(
              text: 'Verify & Create Wallet',
              onPressed: _isComplete && !_isVerifying ? _verifyAndProceed : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isVerifying,
            ),
          ],
        ),
      ),
    );
  }
}

