import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/security/duress_passphrase_store.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Duress passphrase setup screen (formerly panic PIN).
class PanicPinScreen extends ConsumerStatefulWidget {
  const PanicPinScreen({super.key});

  @override
  ConsumerState<PanicPinScreen> createState() => _PanicPinScreenState();
}

class _PanicPinScreenState extends ConsumerState<PanicPinScreen> {
  final _customController = TextEditingController();
  final _confirmController = TextEditingController();
  bool _useCustom = false;
  bool _showSetup = false;
  bool _hasExisting = false;
  bool _isLoading = true;
  bool _isSaving = false;
  bool _obscureCustom = true;
  bool _obscureConfirm = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _checkExisting();
  }

  @override
  void dispose() {
    _customController.dispose();
    _confirmController.dispose();
    super.dispose();
  }

  Future<void> _checkExisting() async {
    try {
      final hasConfigured = await FfiBridge.hasDuressPassphrase();
      final hasStored = await DuressPassphraseStore.exists();
      if (mounted) {
        setState(() {
          _hasExisting = hasConfigured && hasStored;
          _showSetup = !_hasExisting;
          _isLoading = false;
          if (hasConfigured && !hasStored) {
            _error =
                'Duress passphrase needs reconfiguration on this device.';
          }
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _hasExisting = false;
          _showSetup = true;
          _isLoading = false;
          _error = 'Failed to check duress passphrase: $e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_isLoading) {
      return PScaffold(
        title: 'Duress Passphrase',
        appBar: const PAppBar(
          title: 'Duress Passphrase',
          subtitle: 'Preparing decoy vault access',
        ),
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    return PScaffold(
      title: 'Duress Passphrase',
      appBar: const PAppBar(
        title: 'Duress Passphrase',
        subtitle: 'Configure decoy vault access',
        showBackButton: true,
      ),
      body: SafeArea(
        child: (_showSetup || !_hasExisting)
            ? _buildSetupView()
            : _buildManageView(),
      ),
    );
  }

  Widget _buildSetupView() {
    return SingleChildScrollView(
      padding: PSpacing.screenPadding(
        MediaQuery.of(context).size.width,
        vertical: PSpacing.xl,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Icon(
            Icons.emergency,
            size: 80,
            color: AppColors.warning,
          ),
          const SizedBox(height: PSpacing.xl),
          Text(
            'Set duress passphrase',
            style: PTypography.heading2(color: AppColors.textPrimary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: PSpacing.md),
          Text(
            'If forced to unlock your wallet, enter the duress passphrase to open an empty wallet.',
            style: PTypography.bodyMedium(color: AppColors.textSecondary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: PSpacing.xxl),
          _buildInfoCard(),
          const SizedBox(height: PSpacing.lg),
          SwitchListTile(
            title: Text(
              'Use a custom duress passphrase',
              style: PTypography.bodyLarge(color: AppColors.textPrimary),
            ),
            subtitle: Text(
              'If off, your duress passphrase is your passphrase reversed.',
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
            value: _useCustom,
            onChanged: (value) {
              setState(() {
                _useCustom = value;
                _error = null;
                if (!value) {
                  _customController.clear();
                  _confirmController.clear();
                }
              });
            },
          ),
          if (_useCustom) ...[
            const SizedBox(height: PSpacing.md),
            PInput(
              controller: _customController,
              label: 'Custom duress passphrase',
              hint: 'Enter a duress passphrase',
              obscureText: _obscureCustom,
              autocorrect: false,
              enableSuggestions: false,
              suffixIcon: IconButton(
                icon: Icon(
                  _obscureCustom
                      ? Icons.visibility_outlined
                      : Icons.visibility_off_outlined,
                  color: AppColors.textSecondary,
                ),
                onPressed: () {
                  setState(() => _obscureCustom = !_obscureCustom);
                },
              ),
            ),
            const SizedBox(height: PSpacing.md),
            PInput(
              controller: _confirmController,
              label: 'Confirm duress passphrase',
              hint: 'Re-enter to confirm',
              obscureText: _obscureConfirm,
              autocorrect: false,
              enableSuggestions: false,
              errorText: _confirmController.text.isNotEmpty &&
                      _confirmController.text != _customController.text
                  ? 'Passphrases do not match'
                  : null,
              suffixIcon: IconButton(
                icon: Icon(
                  _obscureConfirm
                      ? Icons.visibility_outlined
                      : Icons.visibility_off_outlined,
                  color: AppColors.textSecondary,
                ),
                onPressed: () {
                  setState(() => _obscureConfirm = !_obscureConfirm);
                },
              ),
            ),
          ],
          if (_error != null) ...[
            const SizedBox(height: PSpacing.lg),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.errorBackground,
                border: Border.all(color: AppColors.errorBorder),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                _error!,
                style: PTypography.bodyMedium(color: AppColors.error),
                textAlign: TextAlign.center,
              ),
            ),
          ],
          const SizedBox(height: PSpacing.xl),
          PButton(
            onPressed: _isSaving ? null : _saveDuressPassphrase,
            text: _isSaving ? 'Saving...' : 'Enable duress passphrase',
            fullWidth: true,
            isLoading: _isSaving,
          ),
          if (_hasExisting) ...[
            const SizedBox(height: PSpacing.md),
            PTextButton(
              label: 'Cancel',
              variant: PTextButtonVariant.subtle,
              fullWidth: true,
              onPressed: () {
                setState(() {
                  _showSetup = false;
                  _error = null;
                });
              },
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildManageView() {
    return Padding(
      padding: PSpacing.screenPadding(
        MediaQuery.of(context).size.width,
        vertical: PSpacing.xl,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            padding: const EdgeInsets.all(PSpacing.lg),
            decoration: BoxDecoration(
              color: AppColors.successBackground,
              border: Border.all(color: AppColors.successBorder),
              borderRadius: BorderRadius.circular(16),
            ),
            child: Row(
              children: [
                Icon(Icons.check_circle, color: AppColors.success, size: 32),
                const SizedBox(width: PSpacing.md),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Duress passphrase configured',
                        style: PTypography.bodyLarge(
                          color: AppColors.textPrimary,
                        ).copyWith(fontWeight: FontWeight.w600),
                      ),
                      const SizedBox(height: PSpacing.xs),
                      Text(
                        'Entering it opens a decoy wallet.',
                        style: PTypography.bodyMedium(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: PSpacing.xl),
          _buildInfoCard(),
          const Spacer(),
          PButton(
            onPressed: () {
              setState(() {
                _showSetup = true;
                _error = null;
              });
            },
            text: 'Update duress passphrase',
            fullWidth: true,
          ),
          const SizedBox(height: PSpacing.md),
          PButton(
            onPressed: _removeDuressPassphrase,
            icon: const Icon(Icons.delete_outline),
            variant: PButtonVariant.danger,
            text: 'Remove duress passphrase',
          ),
        ],
      ),
    );
  }

  Widget _buildInfoCard() {
    return Container(
      padding: const EdgeInsets.all(PSpacing.lg),
      decoration: BoxDecoration(
        color: AppColors.infoBackground,
        border: Border.all(color: AppColors.infoBorder),
        borderRadius: BorderRadius.circular(16),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.info_outline, color: AppColors.info, size: 24),
              const SizedBox(width: PSpacing.sm),
              Text(
                'How it works',
                style: PTypography.bodyLarge(
                  color: AppColors.textPrimary,
                ).copyWith(fontWeight: FontWeight.w600),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.md),
          _buildInfoItem('Opens a decoy wallet with empty data.'),
          _buildInfoItem('Default is your passphrase reversed.'),
          _buildInfoItem('Use a custom passphrase if you prefer.'),
          _buildInfoItem('Close and reopen to return to the real wallet.'),
        ],
      ),
    );
  }

  Widget _buildInfoItem(String text) {
    return Padding(
      padding: const EdgeInsets.only(bottom: PSpacing.xs),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('- ', style: TextStyle(color: AppColors.info, fontSize: 16)),
          Expanded(
            child: Text(
              text,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _saveDuressPassphrase() async {
    if (_isSaving) return;

    if (_useCustom) {
      final custom = _customController.text.trim();
      final confirm = _confirmController.text.trim();
      if (custom.isEmpty) {
        setState(() => _error = 'Enter a custom duress passphrase.');
        return;
      }
      if (custom != confirm) {
        setState(() => _error = 'Passphrases do not match.');
        return;
      }
      final validationError = _validateCustomPassphrase(custom);
      if (validationError != null) {
        setState(() => _error = validationError);
        return;
      }
    }

    setState(() {
      _isSaving = true;
      _error = null;
    });

    try {
      final hash = await FfiBridge.setDuressPassphrase(
        customPassphrase: _useCustom ? _customController.text.trim() : null,
      );
      await DuressPassphraseStore.store(hash);
      if (mounted) {
        setState(() {
          _hasExisting = true;
          _showSetup = false;
          _useCustom = false;
          _customController.clear();
          _confirmController.clear();
        });
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Duress passphrase configured'),
            backgroundColor: AppColors.success,
          ),
        );
      }
    } catch (e) {
      setState(() => _error = 'Failed to save duress passphrase: $e');
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  String? _validateCustomPassphrase(String value) {
    if (value.length < 12) {
      return 'Duress passphrase must be at least 12 characters.';
    }
    if (!RegExp('[a-z]').hasMatch(value)) {
      return 'Include a lowercase letter.';
    }
    if (!RegExp('[A-Z]').hasMatch(value)) {
      return 'Include an uppercase letter.';
    }
    if (!RegExp('[0-9]').hasMatch(value)) {
      return 'Include a number.';
    }
    if (!RegExp(r'[!@#$%^&*(),.?":{}|<>]').hasMatch(value)) {
      return r'Include a symbol like !@#$%.';
    }
    final reversed = value.split('').reversed.join();
    if (value == reversed) {
      return 'Passphrase cannot read the same forwards and backwards.';
    }
    return null;
  }

  Future<void> _removeDuressPassphrase() async {
    final confirmed = await PDialog.show<bool>(
      context: context,
      title: 'Remove duress passphrase?',
      content: Text(
        'This disables the decoy wallet shortcut on this device.',
        style: PTypography.bodyMedium(color: AppColors.textSecondary),
      ),
      actions: const [
        PDialogAction(
          label: 'Cancel',
          variant: PButtonVariant.outline,
        ),
        PDialogAction(
          label: 'Remove',
          variant: PButtonVariant.danger,
          result: true,
        ),
      ],
    );

    if (confirmed ?? false) {
      try {
        await FfiBridge.clearDuressPassphrase();
        await DuressPassphraseStore.clear();
        setState(() {
          _hasExisting = false;
          _showSetup = true;
        });
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: const Text('Duress passphrase removed'),
              backgroundColor: AppColors.warning,
            ),
          );
        }
      } catch (e) {
        setState(() => _error = 'Failed to remove duress passphrase: $e');
      }
    }
  }
}
