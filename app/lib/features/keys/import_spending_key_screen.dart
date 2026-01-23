import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

class ImportSpendingKeyScreen extends ConsumerStatefulWidget {
  const ImportSpendingKeyScreen({super.key});

  @override
  ConsumerState<ImportSpendingKeyScreen> createState() =>
      _ImportSpendingKeyScreenState();
}

class _ImportSpendingKeyScreenState
    extends ConsumerState<ImportSpendingKeyScreen> {
  final _labelController = TextEditingController();
  final _birthdayController = TextEditingController();
  final _saplingKeyController = TextEditingController();
  final _orchardKeyController = TextEditingController();

  bool _isSubmitting = false;
  String? _error;

  @override
  void dispose() {
    _labelController.dispose();
    _birthdayController.dispose();
    _saplingKeyController.dispose();
    _orchardKeyController.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      setState(() => _error = 'No active wallet');
      return;
    }

    final sapling = _saplingKeyController.text.trim();
    final orchard = _orchardKeyController.text.trim();
    if (sapling.isEmpty && orchard.isEmpty) {
      setState(() => _error = 'Enter a Sapling or Orchard spending key');
      return;
    }

    final birthdayText = _birthdayController.text.trim();
    final birthday = int.tryParse(birthdayText);
    if (birthday == null || birthday <= 0) {
      setState(() => _error = 'Enter a valid birthday height');
      return;
    }

    setState(() {
      _isSubmitting = true;
      _error = null;
    });

    try {
      final keyId = await FfiBridge.importSpendingKey(
        walletId: walletId,
        saplingKey: sapling.isEmpty ? null : sapling,
        orchardKey: orchard.isEmpty ? null : orchard,
        label: _labelController.text.trim().isEmpty
            ? null
            : _labelController.text.trim(),
        birthdayHeight: birthday,
      );

      if (!mounted) return;
      FfiBridge.rescan(walletId, birthday).catchError((error) {
        if (mounted) {
          setState(() {
            _error = 'Rescan failed to start: $error';
          });
        }
      });
      context.push('/settings/keys/detail?keyId=$keyId');
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Rescan started from block $birthday'),
          backgroundColor: AppColors.success,
        ),
      );
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isSubmitting = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final basePadding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return PScaffold(
      appBar: const PAppBar(
        title: 'Import spending key',
        subtitle: 'Add an existing key to this wallet',
        showBackButton: true,
        centerTitle: true,
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PInput(
              controller: _labelController,
              label: 'Label (optional)',
              hint: 'Example: Legacy wallet',
            ),
            SizedBox(height: PSpacing.md),
            PInput(
              controller: _birthdayController,
              label: 'Birthday height',
              hint: 'Block height to start scanning',
              keyboardType: TextInputType.number,
            ),
            SizedBox(height: PSpacing.md),
            PInput(
              controller: _saplingKeyController,
              label: 'Sapling spending key (optional)',
              hint: 'Paste your Sapling spending key',
              maxLines: 2,
            ),
            SizedBox(height: PSpacing.md),
            PInput(
              controller: _orchardKeyController,
              label: 'Orchard spending key (optional)',
              hint: 'Paste your Orchard spending key',
              maxLines: 2,
            ),
            SizedBox(height: PSpacing.md),
            Text(
              'A rescan will start automatically from the birthday height.',
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
            if (_error != null) ...[
              SizedBox(height: PSpacing.md),
              Text(
                _error!,
                style: PTypography.bodySmall(color: AppColors.error),
              ),
            ],
            SizedBox(height: PSpacing.lg),
            PButton(
              onPressed: _isSubmitting ? null : _submit,
              variant: PButtonVariant.primary,
              child: Text(_isSubmitting ? 'Importing...' : 'Import key'),
            ),
          ],
        ),
      ),
    );
  }
}
