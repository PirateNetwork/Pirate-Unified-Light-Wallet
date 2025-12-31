// Panic PIN configuration screen
// 
// Allows users to set up a decoy vault that appears when the panic PIN is entered

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Panic PIN configuration state
final hasPanicPinProvider = FutureProvider<bool>((ref) async {
  return FfiBridge.hasPanicPin();
});

/// Panic PIN screen
class PanicPinScreen extends ConsumerStatefulWidget {
  const PanicPinScreen({super.key});

  @override
  ConsumerState<PanicPinScreen> createState() => _PanicPinScreenState();
}

class _PanicPinScreenState extends ConsumerState<PanicPinScreen> {
  final _pinController = TextEditingController();
  final _confirmPinController = TextEditingController();
  bool _isLoading = false;
  String? _error;
  bool _showSetup = false;

  @override
  void dispose() {
    _pinController.dispose();
    _confirmPinController.dispose();
    super.dispose();
  }

  Future<void> _setupPanicPin() async {
    // Validate PIN
    if (_pinController.text.length < 4 || _pinController.text.length > 8) {
      setState(() => _error = 'PIN must be 4-8 digits');
      return;
    }

    if (!_pinController.text.contains(RegExp(r'^[0-9]+$'))) {
      setState(() => _error = 'PIN must contain only digits');
      return;
    }

    if (_pinController.text != _confirmPinController.text) {
      setState(() => _error = 'PINs do not match');
      return;
    }

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      await FfiBridge.setPanicPin(_pinController.text);
      
      ref.invalidate(hasPanicPinProvider);
      
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Panic PIN configured successfully'),
            backgroundColor: AppColors.success,
          ),
        );
        setState(() => _showSetup = false);
        _pinController.clear();
        _confirmPinController.clear();
      }
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _removePanicPin() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Remove Panic PIN'),
          content: const Text(
            'Are you sure you want to remove the panic PIN? '
            'The decoy vault will be disabled.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: const Text('Remove'),
            ),
          ],
        );
      },
    );

    if (confirmed != true) return;

    setState(() => _isLoading = true);

    try {
      await FfiBridge.clearPanicPin();
      
      ref.invalidate(hasPanicPinProvider);
      
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Panic PIN removed'),
            backgroundColor: AppColors.info,
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to remove: $e'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    } finally {
      setState(() => _isLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final hasPanicPin = ref.watch(hasPanicPinProvider);

    return PScaffold(
      title: 'Panic PIN',
      appBar: const PAppBar(
        title: 'Panic PIN',
        subtitle: 'Configure your decoy vault shortcut',
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(AppSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Feature explanation
            _buildExplanation(),
            
            const SizedBox(height: AppSpacing.xl),
            
            // Current status and actions
            hasPanicPin.when(
              data: (hasPin) => hasPin
                  ? _buildConfiguredState()
                  : _showSetup
                      ? _buildSetupForm()
                      : _buildNotConfiguredState(),
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (e, _) => Text('Error: $e'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildExplanation() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Main info card
        PCard(
        backgroundColor: AppColors.warning.withValues(alpha: 0.1),
          child: Padding(
            padding: const EdgeInsets.all(AppSpacing.md),
            child: Column(
              children: [
                Row(
                  children: [
                    Container(
                      width: 48,
                      height: 48,
            decoration: BoxDecoration(
              color: AppColors.warning.withValues(alpha: 0.2),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.emergency,
              color: AppColors.warning,
              size: 28,
            ),
                    ),
                    const SizedBox(width: AppSpacing.md),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            'Duress Protection',
                            style: AppTypography.h4.copyWith(
                              color: AppColors.textPrimary,
                            ),
                          ),
                          Text(
                            'Decoy vault for emergencies',
                            style: AppTypography.caption.copyWith(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        // How it works
        PCard(
          child: Padding(
            padding: const EdgeInsets.all(AppSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'How it works',
                  style: AppTypography.bodyBold.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
                const SizedBox(height: AppSpacing.md),
                _HowItWorksStep(
                  number: '1',
                  title: 'Set a Panic PIN',
                  description: 'Choose a 4-8 digit PIN different from your real passphrase',
                ),
                _HowItWorksStep(
                  number: '2',
                  title: 'Enter When Under Duress',
                  description: 'If forced to unlock your wallet, enter the panic PIN instead',
                ),
                _HowItWorksStep(
                  number: '3',
                  title: 'Decoy Vault Opens',
                  description: 'The wallet shows empty balance and no transaction history',
                ),
              ],
            ),
          ),
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        // Important notes
        PCard(
          backgroundColor: AppColors.info.withValues(alpha: 0.1),
          child: Padding(
            padding: const EdgeInsets.all(AppSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(Icons.info_outline, color: AppColors.info),
                    const SizedBox(width: AppSpacing.sm),
                    Text(
                      'Important',
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.info,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: AppSpacing.sm),
                Text(
                  '• The panic PIN must be different from your real passphrase\n'
                  '• Your real wallet remains safe and hidden\n'
                  '• To return to your real wallet, close and re-open the app with your real passphrase',
                  style: AppTypography.body.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildNotConfiguredState() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          padding: const EdgeInsets.all(AppSpacing.xl),
          decoration: BoxDecoration(
            color: AppColors.surface,
            borderRadius: BorderRadius.circular(16),
            border: Border.all(color: AppColors.border),
          ),
          child: Column(
            children: [
              Icon(
                Icons.shield_outlined,
                size: 64,
                color: AppColors.textTertiary,
              ),
              const SizedBox(height: AppSpacing.md),
              Text(
                'Not Configured',
                style: AppTypography.h4.copyWith(
                  color: AppColors.textPrimary,
                ),
              ),
              const SizedBox(height: AppSpacing.sm),
              Text(
                'Set up a panic PIN for emergency situations',
                style: AppTypography.body.copyWith(
                  color: AppColors.textSecondary,
                ),
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
        
        const SizedBox(height: AppSpacing.xl),
        
        PButton(
          text: 'Set Up Panic PIN',
          onPressed: () => setState(() => _showSetup = true),
          variant: PButtonVariant.primary,
          size: PButtonSize.large,
          icon: const Icon(Icons.add_moderator),
        ),
      ],
    );
  }

  Widget _buildConfiguredState() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          padding: const EdgeInsets.all(AppSpacing.xl),
          decoration: BoxDecoration(
            color: AppColors.success.withValues(alpha: 0.1),
            borderRadius: BorderRadius.circular(16),
            border: Border.all(color: AppColors.success.withValues(alpha: 0.3)),
          ),
          child: Column(
            children: [
              Container(
                width: 64,
                height: 64,
                decoration: BoxDecoration(
                  color: AppColors.success.withValues(alpha: 0.2),
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  Icons.verified_user,
                  size: 36,
                  color: AppColors.success,
                ),
              ),
              const SizedBox(height: AppSpacing.md),
              Text(
                'Panic PIN Active',
                style: AppTypography.h4.copyWith(
                  color: AppColors.success,
                ),
              ),
              const SizedBox(height: AppSpacing.sm),
              Text(
                'Decoy vault is ready for emergencies',
                style: AppTypography.body.copyWith(
                  color: AppColors.textSecondary,
                ),
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
        
        const SizedBox(height: AppSpacing.xl),
        
        PButton(
          text: 'Change Panic PIN',
          onPressed: () => setState(() => _showSetup = true),
          variant: PButtonVariant.secondary,
          size: PButtonSize.large,
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        PButton(
          text: 'Remove Panic PIN',
          onPressed: _isLoading ? null : _removePanicPin,
          isLoading: _isLoading,
          variant: PButtonVariant.ghost,
          size: PButtonSize.large,
        ),
      ],
    );
  }

  Widget _buildSetupForm() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Create Panic PIN',
          style: AppTypography.h3.copyWith(
            color: AppColors.textPrimary,
          ),
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        Text(
          'Choose a PIN that is easy to remember but different from your real passphrase.',
          style: AppTypography.body.copyWith(
            color: AppColors.textSecondary,
          ),
        ),
        
        const SizedBox(height: AppSpacing.xl),
        
        PInput(
          controller: _pinController,
          label: 'Panic PIN',
          hint: '4-8 digits',
          obscureText: true,
          keyboardType: TextInputType.number,
          inputFormatters: [
            FilteringTextInputFormatter.digitsOnly,
            LengthLimitingTextInputFormatter(8),
          ],
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        PInput(
          controller: _confirmPinController,
          label: 'Confirm PIN',
          hint: 'Enter PIN again',
          obscureText: true,
          keyboardType: TextInputType.number,
          inputFormatters: [
            FilteringTextInputFormatter.digitsOnly,
            LengthLimitingTextInputFormatter(8),
          ],
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
          text: 'Save Panic PIN',
          onPressed: _isLoading ? null : _setupPanicPin,
          isLoading: _isLoading,
          variant: PButtonVariant.primary,
          size: PButtonSize.large,
        ),
        
        const SizedBox(height: AppSpacing.md),
        
        PButton(
          text: 'Cancel',
          onPressed: () {
            setState(() {
              _showSetup = false;
              _error = null;
            });
            _pinController.clear();
            _confirmPinController.clear();
          },
          variant: PButtonVariant.ghost,
          size: PButtonSize.large,
        ),
      ],
    );
  }
}

class _HowItWorksStep extends StatelessWidget {
  final String number;
  final String title;
  final String description;

  const _HowItWorksStep({
    required this.number,
    required this.title,
    required this.description,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpacing.md),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 24,
            height: 24,
            decoration: BoxDecoration(
              color: AppColors.accentPrimary.withValues(alpha: 0.2),
              shape: BoxShape.circle,
            ),
            child: Center(
              child: Text(
                number,
                style: AppTypography.caption.copyWith(
                  color: AppColors.accentPrimary,
                  fontWeight: FontWeight.bold,
                ),
              ),
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
                  description,
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

