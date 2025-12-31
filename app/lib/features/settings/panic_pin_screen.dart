import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Panic PIN Setup Screen
class PanicPinScreen extends ConsumerStatefulWidget {
  const PanicPinScreen({super.key});

  @override
  ConsumerState<PanicPinScreen> createState() => _PanicPinScreenState();
}

class _PanicPinScreenState extends ConsumerState<PanicPinScreen> {
  static const int _minPinLength = 4;
  static const int _maxPinLength = 8;
  static const String _backspaceKey = 'BACKSPACE';

  String _enteredPin = '';
  String _confirmPin = '';
  bool _isConfirming = false;
  bool _hasExistingPin = false;
  bool _isLoading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _checkExistingPin();
  }

  Future<void> _checkExistingPin() async {
    try {
      final hasPin = await FfiBridge.hasPanicPin();
      if (mounted) {
        setState(() {
          _hasExistingPin = hasPin;
          _isLoading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _hasExistingPin = false;
          _isLoading = false;
          _error = 'Failed to check panic PIN: $e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_isLoading) {
      return PScaffold(
        title: 'Panic PIN',
        appBar: const PAppBar(
          title: 'Panic PIN',
          subtitle: 'Preparing decoy vault access',
        ),
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    return PScaffold(
      title: 'Panic PIN',
      appBar: const PAppBar(
        title: 'Panic PIN',
        subtitle: 'Configure decoy vault access',
        showBackButton: true,
      ),
      body: SafeArea(
        child: _hasExistingPin ? _buildManageView() : _buildSetupView(),
      ),
    );
  }

  Widget _buildSetupView() {
    final pin = _isConfirming ? _confirmPin : _enteredPin;

    return SingleChildScrollView(
      padding: const EdgeInsets.all(PSpacing.xxl),
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
            _isConfirming ? 'Confirm Panic PIN' : 'Set Panic PIN',
            style: PTypography.heading2(color: AppColors.textPrimary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: PSpacing.md),
          Text(
            _isConfirming
                ? 'Re-enter your PIN to confirm.'
                : 'Enter a 4-8 digit PIN that opens a decoy wallet.',
            style: PTypography.bodyMedium(color: AppColors.textSecondary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: PSpacing.xxl),
          _buildInfoCard(),
          const SizedBox(height: PSpacing.xxl),
          _buildPinDisplay(pin),
          const SizedBox(height: PSpacing.xl),
          _buildNumPad(),
          if (!_isConfirming && _enteredPin.length >= _minPinLength) ...[
            const SizedBox(height: PSpacing.lg),
            PButton(
              onPressed: () {
                setState(() {
                  _isConfirming = true;
                  _confirmPin = '';
                  _error = null;
                });
              },
              text: 'Continue',
              fullWidth: true,
            ),
          ],
          if (_isConfirming) ...[
            const SizedBox(height: PSpacing.md),
            PTextButton(
              label: 'Start over',
              variant: PTextButtonVariant.subtle,
              fullWidth: true,
              onPressed: () {
                setState(() {
                  _isConfirming = false;
                  _enteredPin = '';
                  _confirmPin = '';
                  _error = null;
                });
              },
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
        ],
      ),
    );
  }

  Widget _buildManageView() {
    return Padding(
      padding: const EdgeInsets.all(PSpacing.xxl),
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
                        'Panic PIN configured',
                        style: PTypography.bodyLarge(
                          color: AppColors.textPrimary,
                        ).copyWith(fontWeight: FontWeight.w600),
                      ),
                      const SizedBox(height: PSpacing.xs),
                      Text(
                        'Your panic PIN opens the decoy wallet.',
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
            onPressed: _removePanicPin,
            icon: const Icon(Icons.delete_outline),
            variant: PButtonVariant.danger,
            text: 'Remove Panic PIN',
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
          _buildInfoItem('Opens a decoy wallet with fake data.'),
          _buildInfoItem('Protects your main wallet in emergencies.'),
          _buildInfoItem('Different from your app password.'),
          _buildInfoItem('Keep this PIN completely secret.'),
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

  Widget _buildPinDisplay(String pin) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: List.generate(
        _maxPinLength,
        (index) => Container(
          width: 38,
          height: 38,
          margin: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            color: index < pin.length
                ? AppColors.gradientAStart
                : AppColors.backgroundSurface,
            shape: BoxShape.circle,
            border: Border.all(
              color: index < pin.length
                  ? AppColors.gradientAEnd
                  : AppColors.borderDefault,
              width: index < pin.length ? 2 : 1,
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildNumPad() {
    return Column(
      children: [
        _buildNumPadRow(['1', '2', '3']),
        const SizedBox(height: PSpacing.md),
        _buildNumPadRow(['4', '5', '6']),
        const SizedBox(height: PSpacing.md),
        _buildNumPadRow(['7', '8', '9']),
        const SizedBox(height: PSpacing.md),
        _buildNumPadRow(['', '0', _backspaceKey]),
      ],
    );
  }

  Widget _buildNumPadRow(List<String> numbers) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: numbers.map((num) {
        if (num.isEmpty) return const SizedBox(width: 80, height: 80);

        return Padding(
          padding: const EdgeInsets.symmetric(horizontal: PSpacing.sm),
          child: InkWell(
            onTap: () => _onNumPadTap(num),
            borderRadius: BorderRadius.circular(40),
            child: Container(
              width: 80,
              height: 80,
              decoration: BoxDecoration(
                color: AppColors.backgroundSurface,
                shape: BoxShape.circle,
                border: Border.all(color: AppColors.borderStrong, width: 1),
              ),
              child: Center(
                child: num == _backspaceKey
                    ? Icon(
                        Icons.backspace_outlined,
                        color: AppColors.textPrimary,
                        size: 24,
                      )
                    : Text(
                        num,
                        style: PTypography.heading3(
                          color: AppColors.textPrimary,
                        ),
                      ),
              ),
            ),
          ),
        );
      }).toList(),
    );
  }

  void _onNumPadTap(String value) {
    setState(() => _error = null);

    if (value == _backspaceKey) {
      if (_isConfirming) {
        if (_confirmPin.isNotEmpty) {
          setState(() {
            _confirmPin = _confirmPin.substring(0, _confirmPin.length - 1);
          });
        }
      } else {
        if (_enteredPin.isNotEmpty) {
          setState(() {
            _enteredPin = _enteredPin.substring(0, _enteredPin.length - 1);
          });
        }
      }
      return;
    }

    if (_isConfirming) {
      if (_confirmPin.length < _maxPinLength) {
        setState(() => _confirmPin += value);
        if (_confirmPin.length >= _minPinLength) {
          _checkConfirmation();
        }
      }
      return;
    }

    if (_enteredPin.length < _maxPinLength) {
      setState(() => _enteredPin += value);
    }
  }

  Future<void> _checkConfirmation() async {
    if (_confirmPin.length >= _minPinLength && _confirmPin == _enteredPin) {
      await _savePanicPin();
    } else if (_confirmPin.length >= _enteredPin.length) {
      setState(() {
        _error = 'PINs do not match';
        _confirmPin = '';
      });
    }
  }

  Future<void> _savePanicPin() async {
    try {
      await FfiBridge.setPanicPin(_enteredPin);

      if (mounted) {
        setState(() => _hasExistingPin = true);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Panic PIN configured'),
            backgroundColor: AppColors.success,
          ),
        );
        Navigator.pop(context);
      }
    } catch (e) {
      setState(() => _error = 'Failed to save PIN: $e');
    }
  }

  Future<void> _removePanicPin() async {
    final confirmed = await PDialog.show<bool>(
      context: context,
      title: 'Remove panic PIN?',
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

    if (confirmed == true) {
      try {
        await FfiBridge.clearPanicPin();

        setState(() => _hasExistingPin = false);

        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: const Text('Panic PIN removed'),
              backgroundColor: AppColors.warning,
            ),
          );
        }
      } catch (e) {
        setState(() => _error = 'Failed to remove PIN: $e');
      }
    }
  }
}
