import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart' show WalletMeta;
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../features/onboarding/onboarding_flow.dart';
import '../atoms/p_badge.dart';
import '../atoms/p_button.dart';
import '../atoms/p_input.dart';
import '../atoms/p_text_button.dart';
import '../molecules/p_bottom_sheet.dart';
import '../molecules/p_dialog.dart';

class WalletSwitcherButton extends ConsumerWidget {
  const WalletSwitcherButton({
    super.key,
    this.compact = false,
  });

  final bool compact;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final walletMeta = ref.watch(activeWalletMetaProvider);
    final label = walletMeta?.name ?? 'Wallets';
    final isWatchOnly = walletMeta?.watchOnly ?? false;

    return InkWell(
      onTap: () => _showSwitcher(context, ref),
      borderRadius: BorderRadius.circular(PSpacing.radiusMD),
      child: Container(
        padding: EdgeInsets.symmetric(
          horizontal: compact ? PSpacing.sm : PSpacing.md,
          vertical: compact ? PSpacing.xs : PSpacing.sm,
        ),
        decoration: BoxDecoration(
          color: AppColors.backgroundSurface,
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          border: Border.all(color: AppColors.borderSubtle),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.account_balance_wallet_outlined,
              size: compact ? PSpacing.iconSM : PSpacing.iconMD,
              color: AppColors.textSecondary,
            ),
            SizedBox(width: compact ? PSpacing.xs : PSpacing.sm),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 180),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: (compact ? PTypography.labelSmall : PTypography.labelMedium)(
                  color: AppColors.textPrimary,
                ),
              ),
            ),
            if (isWatchOnly && !compact) ...[
              const SizedBox(width: PSpacing.xs),
              const PBadge(
                label: 'Watch',
                variant: PBadgeVariant.info,
                size: PBadgeSize.small,
              ),
            ],
            const SizedBox(width: PSpacing.xs),
            Icon(
              Icons.expand_more,
              size: compact ? PSpacing.iconSM : PSpacing.iconMD,
              color: AppColors.textSecondary,
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _showSwitcher(BuildContext context, WidgetRef ref) async {
    if (_isDesktop) {
      await PDialog.show<void>(
        context: context,
        title: 'Wallets',
        content: const _WalletSwitcherContent(),
        actions: [
          PDialogAction(
            label: 'Close',
            variant: PButtonVariant.outline,
          ),
        ],
      );
      return;
    }

    await PBottomSheet.show<void>(
      context: context,
      title: 'Wallets',
      content: const _WalletSwitcherContent(),
    );
  }
}

class _WalletSwitcherContent extends ConsumerWidget {
  const _WalletSwitcherContent();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final walletsAsync = ref.watch(walletsProvider);
    final activeWalletId = ref.watch(activeWalletProvider);

    return walletsAsync.when(
      data: (wallets) {
        if (wallets.isEmpty) {
          return _EmptyWalletState(
            onAddWallet: () => _startAddWalletFlow(context, ref),
          );
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ...wallets.map(
              (wallet) => Padding(
                padding: const EdgeInsets.only(bottom: PSpacing.sm),
                child: _WalletRow(
                  wallet: wallet,
                  isActive: wallet.id == activeWalletId,
                  onSelect: () async {
                    if (wallet.id != activeWalletId) {
                      try {
                        await ref
                            .read(activeWalletProvider.notifier)
                            .setActiveWallet(wallet.id);
                      } catch (e) {
                        if (context.mounted) {
                          _showSnack(context, 'Could not switch wallet: $e');
                        }
                        return;
                      }
                    }
                    if (Navigator.of(context).canPop()) {
                      Navigator.of(context).pop();
                    }
                  },
                  onRename: () => _renameWallet(context, ref, wallet),
                  onDelete: () => _deleteWallet(context, ref, wallet),
                ),
              ),
            ),
            const SizedBox(height: PSpacing.md),
            PButton(
              onPressed: () => _startAddWalletFlow(context, ref),
              variant: PButtonVariant.secondary,
              fullWidth: true,
              icon: const Icon(Icons.add),
              text: 'Add wallet',
            ),
            const SizedBox(height: PSpacing.sm),
            PTextButton(
              label: 'Import watch-only wallet',
              trailingIcon: Icons.visibility_outlined,
              onPressed: () => _startWatchOnlyFlow(context, ref),
              fullWidth: true,
              variant: PTextButtonVariant.subtle,
            ),
          ],
        );
      },
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => _WalletErrorState(
        message: error.toString(),
        onRetry: () => ref.invalidate(walletsProvider),
      ),
    );
  }

  void _startAddWalletFlow(BuildContext context, WidgetRef ref) {
    ref.read(onboardingControllerProvider.notifier).reset();
    context.push('/onboarding/create-or-import');
  }

  void _startWatchOnlyFlow(BuildContext context, WidgetRef ref) {
    context.push('/onboarding/import-ivk');
  }

  Future<void> _renameWallet(
    BuildContext context,
    WidgetRef ref,
    WalletMeta wallet,
  ) async {
    final controller = TextEditingController(text: wallet.name);

    final confirmed = await PDialog.show<bool>(
      context: context,
      title: 'Rename wallet',
      content: PInput(
        controller: controller,
        label: 'Wallet name',
        hint: 'e.g. Savings',
        autofocus: true,
      ),
      actions: [
        const PDialogAction(
          label: 'Cancel',
          variant: PButtonVariant.outline,
        ),
        PDialogAction<bool>(
          label: 'Save',
          result: true,
        ),
      ],
    );

    if (confirmed == true) {
      final newName = controller.text.trim();
      if (newName.isEmpty || newName == wallet.name) {
        controller.dispose();
        return;
      }

      try {
        await FfiBridge.renameWallet(wallet.id, newName);
        ref.read(refreshWalletsProvider)();
      } catch (e) {
        _showSnack(context, 'Could not rename wallet: $e');
      }
    }

    controller.dispose();
  }

  Future<void> _deleteWallet(
    BuildContext context,
    WidgetRef ref,
    WalletMeta wallet,
  ) async {
    final confirmed = await PDialog.show<bool>(
      context: context,
      title: 'Remove wallet?',
      content: Text(
        'This deletes "${wallet.name}" from this device. Make sure you have the seed saved if you want it back.',
        style: PTypography.bodyMedium(color: AppColors.textSecondary),
      ),
      actions: [
        const PDialogAction(
          label: 'Cancel',
          variant: PButtonVariant.outline,
        ),
        PDialogAction<bool>(
          label: 'Remove',
          variant: PButtonVariant.danger,
          result: true,
        ),
      ],
    );

    if (confirmed == true) {
      try {
        await FfiBridge.deleteWallet(wallet.id);
        ref.read(refreshWalletsProvider)();
        ref.invalidate(walletsExistProvider);

        final newActive = await FfiBridge.getActiveWallet();
        final notifier = ref.read(activeWalletProvider.notifier);
        if (newActive == null) {
          notifier.clearActiveWallet();
          ref.read(appUnlockedProvider.notifier).setUnlocked(false);
          ref.read(onboardingControllerProvider.notifier).reset();
          if (context.mounted) {
            context.go('/onboarding/welcome');
          }
        } else {
          await notifier.setActiveWallet(newActive);
        }
      } catch (e) {
        _showSnack(context, 'Could not remove wallet: $e');
      }
    }
  }

  void _showSnack(BuildContext context, String message) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: AppColors.error,
        behavior: SnackBarBehavior.floating,
      ),
    );
  }
}

class _WalletRow extends StatelessWidget {
  const _WalletRow({
    required this.wallet,
    required this.isActive,
    required this.onSelect,
    required this.onRename,
    required this.onDelete,
  });

  final WalletMeta wallet;
  final bool isActive;
  final VoidCallback onSelect;
  final VoidCallback onRename;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final background =
        isActive ? AppColors.selectedBackground : AppColors.backgroundSurface;
    final border = isActive ? AppColors.selectedBorder : AppColors.borderSubtle;

    return InkWell(
      onTap: onSelect,
      borderRadius: BorderRadius.circular(PSpacing.radiusMD),
      child: Container(
        padding: const EdgeInsets.all(PSpacing.md),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          border: Border.all(color: border),
        ),
        child: Row(
          children: [
            Icon(
              wallet.watchOnly
                  ? Icons.visibility_outlined
                  : Icons.account_balance_wallet_outlined,
              size: PSpacing.iconMD,
              color: AppColors.textSecondary,
            ),
            const SizedBox(width: PSpacing.sm),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    wallet.name,
                    style: PTypography.labelLarge(
                      color: AppColors.textPrimary,
                    ),
                  ),
                  const SizedBox(height: PSpacing.xxs),
                  Text(
                    wallet.watchOnly ? 'Incoming only' : 'Full access',
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                ],
              ),
            ),
            if (isActive)
              const PBadge(
                label: 'Active',
                variant: PBadgeVariant.success,
                size: PBadgeSize.small,
              ),
            const SizedBox(width: PSpacing.xs),
            PopupMenuButton<_WalletAction>(
              icon: Icon(
                Icons.more_horiz,
                color: AppColors.textSecondary,
              ),
              color: AppColors.backgroundElevated,
              onSelected: (action) {
                switch (action) {
                  case _WalletAction.rename:
                    onRename();
                    break;
                  case _WalletAction.delete:
                    onDelete();
                    break;
                }
              },
              itemBuilder: (context) => [
                const PopupMenuItem(
                  value: _WalletAction.rename,
                  child: Text('Rename'),
                ),
                PopupMenuItem(
                  value: _WalletAction.delete,
                  child: Text(
                    'Remove',
                    style: TextStyle(color: AppColors.error),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

enum _WalletAction {
  rename,
  delete,
}

class _EmptyWalletState extends StatelessWidget {
  const _EmptyWalletState({required this.onAddWallet});

  final VoidCallback onAddWallet;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'No wallets yet.',
          style: PTypography.heading4(color: AppColors.textPrimary),
        ),
        const SizedBox(height: PSpacing.xs),
        Text(
          'Create a new wallet or import an existing one.',
          style: PTypography.bodyMedium(color: AppColors.textSecondary),
        ),
        const SizedBox(height: PSpacing.lg),
        PButton(
          onPressed: onAddWallet,
          icon: const Icon(Icons.add),
          text: 'Add wallet',
          fullWidth: true,
        ),
      ],
    );
  }
}

class _WalletErrorState extends StatelessWidget {
  const _WalletErrorState({
    required this.message,
    required this.onRetry,
  });

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Could not load wallets.',
          style: PTypography.heading4(color: AppColors.textPrimary),
        ),
        const SizedBox(height: PSpacing.xs),
        Text(
          message,
          style: PTypography.bodySmall(color: AppColors.textSecondary),
        ),
        const SizedBox(height: PSpacing.md),
        PButton(
          onPressed: onRetry,
          variant: PButtonVariant.outline,
          text: 'Try again',
          fullWidth: true,
        ),
      ],
    );
  }
}
