// Seed phrase export screen with gated security flow

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/security/screenshot_protection.dart';

/// Export seed state
enum ExportSeedState {
  warning,
  biometric,
  passphrase,
  display,
  complete,
}

/// Export seed screen provider
class ExportSeedStateNotifier extends Notifier<ExportSeedState> {
  @override
  ExportSeedState build() => ExportSeedState.warning;

  ExportSeedState get value => state;
  set value(ExportSeedState next) => state = next;
}

final exportSeedStateProvider =
    NotifierProvider<ExportSeedStateNotifier, ExportSeedState>(
  ExportSeedStateNotifier.new,
);

/// Seed words provider
class SeedWordsNotifier extends Notifier<List<String>?> {
  @override
  List<String>? build() => null;

  List<String>? get words => state;
  set words(List<String> value) => state = value;

  void clear() {
    state = null;
  }
}

final seedWordsProvider =
    NotifierProvider<SeedWordsNotifier, List<String>?>(
  SeedWordsNotifier.new,
);

/// Clipboard timer provider
class ClipboardTimerNotifier extends Notifier<int?> {
  @override
  int? build() => null;

  int? get value => state;
  set value(int? seconds) => state = seconds;

  bool tick() {
    final current = state;
    if (current == null) {
      return true;
    }
    if (current <= 1) {
      state = null;
      return true;
    }
    state = current - 1;
    return false;
  }

  void clear() {
    state = null;
  }
}

final clipboardTimerProvider =
    NotifierProvider<ClipboardTimerNotifier, int?>(
  ClipboardTimerNotifier.new,
);

/// Seed phrase export screen
class ExportSeedScreen extends ConsumerStatefulWidget {
  const ExportSeedScreen({super.key});

  @override
  ConsumerState<ExportSeedScreen> createState() => _ExportSeedScreenState();
}

class _ExportSeedScreenState extends ConsumerState<ExportSeedScreen> {
  final _passphraseController = TextEditingController();
  bool _isLoading = false;
  bool _obscurePassphrase = true;
  String? _error;
  ScreenProtection? _screenProtection;

  @override
  void initState() {
    super.initState();
    // Enable screenshot protection
    _enableScreenshotProtection();
  }

  @override
  void dispose() {
    _passphraseController.dispose();
    // Disable screenshot protection
    _disableScreenshotProtection();
    super.dispose();
  }

  void _enableScreenshotProtection() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _disableScreenshotProtection() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  Future<void> _acknowledgeWarning() async {
    setState(() => _isLoading = true);
    
    try {
      await FfiBridge.acknowledgeSeedWarning();
      ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.biometric;
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _completeBiometric() async {
    setState(() => _isLoading = true);
    
    try {
      // In production, trigger actual biometric authentication
      final success = await FfiBridge.authenticateBiometric('Verify identity to export seed');
      
      if (success) {
        ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.passphrase;
      } else {
        setState(() => _error = 'Biometric authentication failed');
      }
    } catch (e) {
      // Biometric not available, skip to passphrase
      ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.passphrase;
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _skipBiometric() async {
    ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.passphrase;
  }

  Future<void> _verifyPassphrase() async {
    if (_passphraseController.text.isEmpty) {
      setState(() => _error = 'Please enter your passphrase');
      return;
    }

    setState(() {
      _isLoading = true;
      _error = null;
    });
    
    try {
      final walletId = ref.read(activeWalletProvider);
      if (walletId == null) {
        throw Exception('No active wallet');
      }

      final words = await FfiBridge.exportSeedWithPassphrase(
        walletId,
        _passphraseController.text,
      );
      
      ref.read(seedWordsProvider.notifier).words = words;
      ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.display;
      
      // Clear passphrase from memory
      _passphraseController.clear();
    } catch (e) {
      setState(() => _error = 'Invalid passphrase');
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _copyToClipboard() async {
    final words = ref.read(seedWordsProvider);
    if (words == null) return;

    final seedPhrase = words.join(' ');
    await Clipboard.setData(ClipboardData(text: seedPhrase));
    
    // Start auto-clear timer (30 seconds)
    ref.read(clipboardTimerProvider.notifier).value = 30;
    _startClipboardTimer();
    
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: const Text('Seed phrase copied. Auto-clearing in 30 seconds.'),
          backgroundColor: AppColors.warning,
        ),
      );
    }
  }

  void _startClipboardTimer() {
    Future.doWhile(() async {
      await Future<void>.delayed(const Duration(seconds: 1));
      final shouldStop = ref.read(clipboardTimerProvider.notifier).tick();
      if (shouldStop) {
        // Clear clipboard
        await Clipboard.setData(const ClipboardData(text: ''));
        return false;
      }
      return true;
    });
  }

  void _complete() {
    // Clear seed from state
    ref.read(seedWordsProvider.notifier).clear();
    ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.warning;
    context.pop();
  }

  void _resetExportFlow() {
    ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.warning;
    ref.read(seedWordsProvider.notifier).clear();
    ref.read(clipboardTimerProvider.notifier).clear();
    _passphraseController.clear();
    setState(() {
      _error = null;
      _isLoading = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    // Listen for wallet changes
    ref.listen<WalletId?>(
      activeWalletProvider,
      (previous, next) {
        if (previous != next) {
          _resetExportFlow();
        }
      },
    );
    
    final state = ref.watch(exportSeedStateProvider);
    
    return PScaffold(
      title: 'Export Seed Phrase',
      appBar: PAppBar(
        title: 'Export Seed Phrase',
        subtitle: 'Securely back up your recovery words',
        onBack: () {
          ref.read(seedWordsProvider.notifier).clear();
          ref.read(exportSeedStateProvider.notifier).value = ExportSeedState.warning;
          context.pop();
        },
        showBackButton: true,
      ),
      body: switch (state) {
        ExportSeedState.warning => _buildWarningStep(),
        ExportSeedState.biometric => _buildBiometricStep(),
        ExportSeedState.passphrase => _buildPassphraseStep(),
        ExportSeedState.display => _buildDisplayStep(),
        ExportSeedState.complete => _buildCompleteStep(),
      },
    );
  }

  Widget _buildWarningStep() {
    return SingleChildScrollView(
      padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Warning icon
          Container(
            width: 80,
            height: 80,
            decoration: BoxDecoration(
              color: AppColors.error.withValues(alpha: 0.1),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.warning_amber_rounded,
              size: 48,
              color: AppColors.error,
            ),
          ),
          
          SizedBox(height: AppSpacing.xl),
          
          Text(
            'Backup Your Seed Phrase',
            style: AppTypography.h2.copyWith(
              color: AppColors.textPrimary,
            ),
            textAlign: TextAlign.center,
          ),
          
          SizedBox(height: AppSpacing.lg),
          
          // Primary warning
          PCard(
            backgroundColor: AppColors.error.withValues(alpha: 0.1),
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(Icons.security, color: AppColors.error),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: Text(
                      'Your seed phrase is the ONLY way to recover your wallet. '
                      'Anyone with access to these words can steal all your funds.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.error,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          
          SizedBox(height: AppSpacing.md),
          
          // Secondary warning
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Column(
                children: [
                  const _WarningItem(
                    icon: Icons.visibility_off,
                    text: 'Never share your seed phrase with anyone',
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  const _WarningItem(
                    icon: Icons.language,
                    text: 'Never enter it on any website',
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  const _WarningItem(
                    icon: Icons.cloud_off,
                    text: 'Never store it digitally unencrypted',
                  ),
                ],
              ),
            ),
          ),
          
          const SizedBox(height: AppSpacing.md),
          
          // Backup instructions
          PCard(
            backgroundColor: AppColors.info.withValues(alpha: 0.1),
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(Icons.edit_note, color: AppColors.info),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: Text(
                      'Write down these 24 words on paper and store them '
                      'in a secure location. Consider using a metal backup.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          
          SizedBox(height: AppSpacing.xxl),
          
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(bottom: AppSpacing.md),
              child: Text(
                _error!,
                style: AppTypography.caption.copyWith(color: AppColors.error),
                textAlign: TextAlign.center,
              ),
            ),
          
          PButton(
            text: 'I Understand the Risks',
            onPressed: _isLoading ? null : _acknowledgeWarning,
            isLoading: _isLoading,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
          ),
          
          SizedBox(height: AppSpacing.md),
          
          PButton(
            text: 'Cancel',
            onPressed: () => context.pop(),
            variant: PButtonVariant.ghost,
            size: PButtonSize.large,
          ),
        ],
      ),
    );
  }

  Widget _buildBiometricStep() {
    return Padding(
      padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 100,
            height: 100,
            decoration: BoxDecoration(
              color: AppColors.accentPrimary.withValues(alpha: 0.1),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.fingerprint,
              size: 64,
              color: AppColors.accentPrimary,
            ),
          ),
          
          SizedBox(height: AppSpacing.xl),
          
          Text(
            'Verify Your Identity',
            style: AppTypography.h2.copyWith(
              color: AppColors.textPrimary,
            ),
            textAlign: TextAlign.center,
          ),
          
          SizedBox(height: AppSpacing.md),
          
          Text(
            'Use biometric authentication to continue',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
            textAlign: TextAlign.center,
          ),
          
          const SizedBox(height: AppSpacing.xxl),
          
          PButton(
            text: 'Authenticate',
            onPressed: _isLoading ? null : _completeBiometric,
            isLoading: _isLoading,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
            icon: const Icon(Icons.fingerprint),
          ),
          
          const SizedBox(height: AppSpacing.md),
          
          PTextButton(
            label: 'Use Passphrase Instead',
            onPressed: _skipBiometric,
            variant: PTextButtonVariant.subtle,
          ),
        ],
      ),
    );
  }

  Widget _buildPassphraseStep() {
    return Padding(
      padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 80,
            height: 80,
            decoration: BoxDecoration(
              color: AppColors.accentPrimary.withValues(alpha: 0.1),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.key,
              size: 48,
              color: AppColors.accentPrimary,
            ),
          ),
          
          SizedBox(height: AppSpacing.xl),
          
          Text(
            'Enter Your Passphrase',
            style: AppTypography.h2.copyWith(
              color: AppColors.textPrimary,
            ),
            textAlign: TextAlign.center,
          ),
          
          const SizedBox(height: AppSpacing.md),
          
          Text(
            'Verify your identity to view the seed phrase',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
            textAlign: TextAlign.center,
          ),
          
          SizedBox(height: AppSpacing.xl),
          
          PInput(
            controller: _passphraseController,
            label: 'Passphrase',
            hint: 'Enter your wallet passphrase',
            obscureText: _obscurePassphrase,
            suffixIcon: IconButton(
              icon: Icon(
                _obscurePassphrase ? Icons.visibility : Icons.visibility_off,
              ),
              onPressed: () {
                setState(() => _obscurePassphrase = !_obscurePassphrase);
              },
            ),
            onSubmitted: (_) => _verifyPassphrase(),
          ),
          
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: AppSpacing.md),
              child: Text(
                _error!,
                style: AppTypography.caption.copyWith(color: AppColors.error),
              ),
            ),
          
          const SizedBox(height: AppSpacing.xl),
          
          PButton(
            text: 'Verify & Export',
            onPressed: _isLoading ? null : _verifyPassphrase,
            isLoading: _isLoading,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
          ),
        ],
      ),
    );
  }

  Widget _buildDisplayStep() {
    final words = ref.watch(seedWordsProvider);
    final clipboardTimer = ref.watch(clipboardTimerProvider);
    
    if (words == null) {
      return const Center(child: CircularProgressIndicator());
    }
    
    return SingleChildScrollView(
      padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Warning banner
          Container(
            padding: const EdgeInsets.all(AppSpacing.md),
            decoration: BoxDecoration(
              color: AppColors.warning.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppColors.warning.withValues(alpha: 0.3)),
            ),
            child: Row(
              children: [
                Icon(Icons.screenshot_monitor, color: AppColors.warning),
                const SizedBox(width: AppSpacing.md),
                Expanded(
                  child: Text(
                    'Screenshots are disabled. Write these words down.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.warning,
                    ),
                  ),
                ),
              ],
            ),
          ),
          
          SizedBox(height: AppSpacing.lg),
          
          Text(
            'Your Seed Phrase',
            style: AppTypography.h3.copyWith(
              color: AppColors.textPrimary,
            ),
            textAlign: TextAlign.center,
          ),
          
          SizedBox(height: AppSpacing.lg),
          
          // Seed words grid
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Wrap(
                spacing: AppSpacing.sm,
                runSpacing: AppSpacing.sm,
                children: List.generate(words.length, (index) {
                  return Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppSpacing.md,
                      vertical: AppSpacing.sm,
                    ),
                    decoration: BoxDecoration(
                      color: AppColors.surfaceElevated,
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          '${index + 1}.',
                          style: AppTypography.caption.copyWith(
                            color: AppColors.textTertiary,
                          ),
                        ),
                        const SizedBox(width: 4),
                        Text(
                          words[index],
                          style: AppTypography.bodyBold.copyWith(
                            color: AppColors.textPrimary,
                            fontFamily: 'monospace',
                          ),
                        ),
                      ],
                    ),
                  );
                }),
              ),
            ),
          ),
          
          SizedBox(height: AppSpacing.lg),
          
          // Copy button
          PButton(
            text: clipboardTimer != null 
                ? 'Copied (clearing in ${clipboardTimer}s)'
                : 'Copy to Clipboard',
            onPressed: clipboardTimer != null ? null : _copyToClipboard,
            variant: PButtonVariant.secondary,
            size: PButtonSize.large,
            icon: const Icon(Icons.copy),
          ),
          
          if (clipboardTimer != null)
            Padding(
              padding: const EdgeInsets.only(top: AppSpacing.sm),
              child: Text(
                'Clipboard will be automatically cleared for security',
                style: AppTypography.caption.copyWith(
                  color: AppColors.warning,
                ),
                textAlign: TextAlign.center,
              ),
            ),
          
          SizedBox(height: AppSpacing.xxl),
          
          PButton(
            text: "I've Written It Down",
            onPressed: _complete,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
          ),
        ],
      ),
    );
  }

  Widget _buildCompleteStep() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 100,
            height: 100,
            decoration: BoxDecoration(
              color: AppColors.success.withValues(alpha: 0.1),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.check_circle,
              size: 64,
              color: AppColors.success,
            ),
          ),
          SizedBox(height: AppSpacing.xl),
          Text(
            'Backup Complete',
            style: AppTypography.h2.copyWith(
              color: AppColors.textPrimary,
            ),
          ),
          SizedBox(height: AppSpacing.md),
          Text(
            'Store your seed phrase securely',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
          SizedBox(height: AppSpacing.xxl),
          PButton(
            text: 'Done',
            onPressed: () => context.pop(),
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
          ),
        ],
      ),
    );
  }
}

class _WarningItem extends StatelessWidget {
  final IconData icon;
  final String text;

  const _WarningItem({required this.icon, required this.text});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 20, color: AppColors.warning),
        const SizedBox(width: AppSpacing.md),
        Expanded(
          child: Text(
            text,
            style: AppTypography.body.copyWith(
              color: AppColors.textPrimary,
            ),
          ),
        ),
      ],
    );
  }
}
