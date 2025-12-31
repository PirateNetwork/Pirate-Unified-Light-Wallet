/// IVK Import Screen - Create watch-only wallet from Incoming Viewing Key

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';

/// IVK import screen for creating watch-only wallets
class IvkImportScreen extends ConsumerStatefulWidget {
  const IvkImportScreen({super.key});

  @override
  ConsumerState<IvkImportScreen> createState() => _IvkImportScreenState();
}

class _IvkImportScreenState extends ConsumerState<IvkImportScreen> {
  final _nameController = TextEditingController(text: 'Watch-only wallet');
  final _ivkController = TextEditingController();
  final _birthdayController = TextEditingController();

  bool _isImporting = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    // Set default birthday to current tip - some buffer
    _birthdayController.text = '${FfiBridge.defaultBirthdayHeight}';
  }

  @override
  void dispose() {
    _nameController.dispose();
    _ivkController.dispose();
    _birthdayController.dispose();
    super.dispose();
  }

  bool get _isValid {
    return _nameController.text.trim().isNotEmpty &&
           _ivkController.text.trim().isNotEmpty &&
           _birthdayController.text.trim().isNotEmpty;
  }

  Future<void> _pasteIvk() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    if (data?.text != null) {
      _ivkController.text = data!.text!.trim();
      setState(() {});
    }
  }

  Future<void> _importIvk() async {
    if (!_isValid) return;

    setState(() {
      _isImporting = true;
      _error = null;
    });

    try {
      final birthday = int.tryParse(_birthdayController.text.trim());
      if (birthday == null || birthday < 1) {
        throw ArgumentError('Invalid birthday height');
      }

      // Import IVK via FFI
      final walletId = await FfiBridge.importIvk(
        name: _nameController.text.trim(),
        ivk: _ivkController.text.trim(),
        birthday: birthday,
      );

      // Set as active wallet
      ref.read(activeWalletProvider.notifier).setActiveWallet(walletId);
      
      // Refresh wallets list
      ref.read(refreshWalletsProvider)();

      if (mounted) {
        // Navigate to home with success message
        context.go('/home');
        
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Row(
              children: [
                const Icon(Icons.visibility, color: Colors.white, size: 20),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'Watch-only wallet created.',
                  ),
                ),
              ],
            ),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    } catch (e) {
      setState(() {
        _error = e.toString();
        _isImporting = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Import IVK',
      appBar: PAppBar(
        title: 'Import Viewing Key',
        subtitle: 'Create a watch-only wallet',
        onBack: () => context.pop(),
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(AppSpacing.xl),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Watch-only info banner
            Container(
              padding: const EdgeInsets.all(AppSpacing.md),
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  colors: [
                    AppColors.accentPrimary.withValues(alpha: 0.1),
                    AppColors.accentSecondary.withValues(alpha: 0.1),
                  ],
                ),
                borderRadius: BorderRadius.circular(16),
                border: Border.all(
                  color: AppColors.accentPrimary.withValues(alpha: 0.3),
                ),
              ),
              child: Column(
                children: [
                  Row(
                    children: [
                      Container(
                        padding: const EdgeInsets.all(AppSpacing.sm),
                        decoration: BoxDecoration(
                          color: AppColors.accentPrimary.withValues(alpha: 0.2),
                          borderRadius: BorderRadius.circular(12),
                        ),
                        child: Icon(
                          Icons.visibility,
                          color: AppColors.accentPrimary,
                          size: 24,
                        ),
                      ),
                      const SizedBox(width: AppSpacing.md),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              kWatchOnlyLabel,
                              style: AppTypography.h4.copyWith(
                                color: AppColors.accentPrimary,
                              ),
                            ),
                            Text(
                              'Incoming activity only',
                              style: AppTypography.caption.copyWith(
                                color: AppColors.textSecondary,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: AppSpacing.md),
                  const Divider(),
                  const SizedBox(height: AppSpacing.sm),
                  _InfoRow(
                    icon: Icons.check_circle_outline,
                    iconColor: AppColors.success,
                    text: 'View incoming transactions',
                  ),
                  _InfoRow(
                    icon: Icons.check_circle_outline,
                    iconColor: AppColors.success,
                    text: 'See balance and incoming activity',
                  ),
                  _InfoRow(
                    icon: Icons.cancel_outlined,
                    iconColor: AppColors.error,
                    text: 'Cannot spend funds',
                  ),
                  _InfoRow(
                    icon: Icons.cancel_outlined,
                    iconColor: AppColors.error,
                    text: 'Outgoing details are hidden',
                  ),
                ],
              ),
            ),

            const SizedBox(height: AppSpacing.xxl),

            // Wallet name input
            PInput(
              controller: _nameController,
              label: 'Wallet name',
              hint: 'e.g., Watch-only wallet',
            ),

            const SizedBox(height: AppSpacing.lg),

            // IVK input
            PInput(
              controller: _ivkController,
              label: 'Incoming viewing key (IVK)',
              hint: 'zxviews1...',
              maxLines: 4,
              suffixIcon: IconButton(
                icon: const Icon(Icons.content_paste),
                onPressed: _pasteIvk,
                tooltip: 'Paste from clipboard',
              ),
            ),

            const SizedBox(height: AppSpacing.lg),

            // Birthday height input
            PInput(
              controller: _birthdayController,
              label: 'Birthday height',
              hint: 'Block height when the wallet was created',
              keyboardType: TextInputType.number,
              helperText: 'Lower values scan more blocks and take longer.',
            ),

            const SizedBox(height: AppSpacing.lg),

            // Error message
            if (_error != null)
              Container(
                padding: const EdgeInsets.all(AppSpacing.md),
                margin: const EdgeInsets.only(bottom: AppSpacing.lg),
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
                    Icons.info_outline,
                    color: AppColors.warning,
                    size: 20,
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Text(
                      'An IVK can view incoming activity but cannot spend. '
                      'Keep your full seed backed up separately.',
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ],
              ),
            ),

            const SizedBox(height: AppSpacing.xxl),

            // Import button
            PButton(
              text: _isImporting ? 'Importing...' : 'Import watch-only wallet',
              onPressed: _isValid && !_isImporting ? _importIvk : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isImporting,
            ),
          ],
        ),
      ),
    );
  }
}

/// Info row for watch-only capabilities
class _InfoRow extends StatelessWidget {
  final IconData icon;
  final Color iconColor;
  final String text;

  const _InfoRow({
    required this.icon,
    required this.iconColor,
    required this.text,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSpacing.xs),
      child: Row(
        children: [
          Icon(icon, color: iconColor, size: 18),
          const SizedBox(width: AppSpacing.sm),
          Expanded(
            child: Text(
              text,
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
                fontSize: 14,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

