import 'package:decimal/decimal.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/ltc_address.dart';
import '../../../core/swaps/swap_amount_utils.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/molecules/p_card.dart';

class SwapFundingBalanceCard extends StatelessWidget {
  const SwapFundingBalanceCard({
    required this.ltcBalance,
    required this.arrrBalance,
    required this.varrrBalance,
    required this.isLoading,
    required this.isActionPending,
    required this.onRefresh,
    required this.onGetDepositAddress,
    required this.onWithdrawAsset,
    required this.onSweepArrrToWallet,
    this.error,
    this.message,
    super.key,
  });

  static const _assets = [SwapAsset.arrr, SwapAsset.varrr, SwapAsset.ltc];

  final Decimal? ltcBalance;
  final Decimal? arrrBalance;
  final Decimal? varrrBalance;
  final bool isLoading;
  final bool isActionPending;
  final VoidCallback onRefresh;
  final Future<String> Function(SwapAsset asset) onGetDepositAddress;
  final Future<void> Function({
    required SwapAsset asset,
    required String address,
    required Decimal amount,
  })
  onWithdrawAsset;
  final Future<void> Function() onSweepArrrToWallet;
  final String? error;
  final String? message;

  bool get _hasFunds => _assets.any(
    (asset) => (_balanceFor(asset) ?? Decimal.zero) > Decimal.zero,
  );

  Decimal? _balanceFor(SwapAsset asset) {
    return switch (asset) {
      SwapAsset.arrr => arrrBalance,
      SwapAsset.varrr => varrrBalance,
      SwapAsset.ltc => ltcBalance,
    };
  }

  @override
  Widget build(BuildContext context) {
    final isMobile = PSpacing.isMobile(MediaQuery.of(context).size.width);
    final contentAlignment = isMobile
        ? WrapAlignment.center
        : WrapAlignment.start;
    final actionsAlignment = isMobile
        ? WrapAlignment.center
        : WrapAlignment.end;

    return PCard(
      padding: const EdgeInsets.all(PSpacing.md),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.all(PSpacing.sm),
                decoration: BoxDecoration(
                  color: _hasFunds
                      ? AppColors.warning.withValues(alpha: 0.12)
                      : AppColors.info.withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                ),
                child: Icon(
                  _hasFunds
                      ? Icons.account_balance_wallet_outlined
                      : Icons.account_balance_outlined,
                  color: _hasFunds ? AppColors.warning : AppColors.info,
                  size: 20,
                ),
              ),
              const SizedBox(width: PSpacing.md),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Swap funding wallet'.tr,
                      style: PTypography.titleSmall(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      'Temporary KDF balances used for deposits, open orders, and recovery. These funds are recoverable from the same wallet seed in Komodo Wallet.'
                          .tr,
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              IconButton(
                tooltip: 'Refresh'.tr,
                onPressed: isLoading ? null : onRefresh,
                icon: isLoading
                    ? SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: AppColors.accentPrimary,
                        ),
                      )
                    : Icon(
                        Icons.refresh,
                        color: AppColors.textSecondary,
                        size: 20,
                      ),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.md),
          Wrap(
            alignment: contentAlignment,
            runAlignment: contentAlignment,
            spacing: PSpacing.sm,
            runSpacing: PSpacing.sm,
            children: _assets
                .map(
                  (asset) => _BalancePill(
                    label: asset.ticker,
                    amount: _formatBalance(_balanceFor(asset)),
                    highlight:
                        (_balanceFor(asset) ?? Decimal.zero) > Decimal.zero,
                  ),
                )
                .toList(),
          ),
          if (error != null) ...[
            const SizedBox(height: PSpacing.md),
            _Notice(
              icon: Icons.warning_amber_outlined,
              text: error!,
              color: AppColors.error,
            ),
          ] else if (message != null) ...[
            const SizedBox(height: PSpacing.md),
            _Notice(
              icon: Icons.info_outline,
              text: message!,
              color: AppColors.info,
            ),
          ],
          const SizedBox(height: PSpacing.md),
          Wrap(
            alignment: actionsAlignment,
            runAlignment: actionsAlignment,
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: PSpacing.sm,
            runSpacing: PSpacing.sm,
            children: [
              PButton(
                text: 'Deposit'.tr,
                size: PButtonSize.small,
                variant: PButtonVariant.secondary,
                onPressed: isActionPending
                    ? null
                    : () => _startDeposit(context),
              ),
              PButton(
                text: 'Withdraw'.tr,
                size: PButtonSize.small,
                variant: PButtonVariant.outline,
                loading: isActionPending,
                onPressed: isActionPending
                    ? null
                    : () => _startWithdraw(context),
              ),
              PButton(
                text: 'Sweep ARRR'.tr,
                size: PButtonSize.small,
                variant: PButtonVariant.outline,
                loading: isActionPending,
                onPressed:
                    (arrrBalance ?? Decimal.zero) > Decimal.zero &&
                        !isActionPending
                    ? () => _confirmSweepArrr(context)
                    : null,
              ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _startDeposit(BuildContext context) async {
    final asset = await _pickAsset(context, title: 'Deposit'.tr);
    if (asset == null || !context.mounted) return;

    String address;
    try {
      address = await onGetDepositAddress(asset);
    } catch (_) {
      return;
    }
    if (!context.mounted) return;
    await _showDepositAddress(context, asset: asset, address: address);
  }

  Future<void> _startWithdraw(BuildContext context) async {
    final asset = await _pickAsset(context, title: 'Withdraw'.tr);
    if (asset == null || !context.mounted) return;
    await _showWithdrawForm(context, asset);
  }

  Future<SwapAsset?> _pickAsset(BuildContext context, {required String title}) {
    return showDialog<SwapAsset>(
      context: context,
      builder: (dialogContext) {
        return AlertDialog(
          title: Text(title),
          contentPadding: const EdgeInsets.fromLTRB(24, 16, 24, 8),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: _assets.map((asset) {
              final balance = _balanceFor(asset);
              return ListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(asset.ticker),
                subtitle: Text(
                  '${'Balance'.tr}: ${_formatBalance(balance)} ${asset.ticker}',
                ),
                trailing: Icon(
                  Icons.chevron_right,
                  color: AppColors.textTertiary,
                ),
                onTap: () => Navigator.of(dialogContext).pop(asset),
              );
            }).toList(),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: Text('Cancel'.tr),
            ),
          ],
        );
      },
    );
  }

  Future<void> _showDepositAddress(
    BuildContext context, {
    required SwapAsset asset,
    required String address,
  }) {
    return showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return AlertDialog(
          title: Text('${'Deposit'.tr} ${asset.ticker}'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                '${'Send'.tr} ${asset.ticker} ${'to this KDF funding address:'.tr}',
              ),
              const SizedBox(height: PSpacing.md),
              DecoratedBox(
                decoration: BoxDecoration(
                  color: AppColors.backgroundElevated,
                  borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                  border: Border.all(color: AppColors.borderSubtle),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(PSpacing.md),
                  child: SelectableText(
                    address,
                    style: PTypography.codeMedium(color: AppColors.textPrimary),
                  ),
                ),
              ),
              const SizedBox(height: PSpacing.sm),
              Text(
                'Only send the selected coin to this address. Funding balances remain recoverable from the same wallet seed in Komodo Wallet.'
                    .tr,
                style: PTypography.bodySmall(color: AppColors.textSecondary),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () async {
                await Clipboard.setData(ClipboardData(text: address));
                if (dialogContext.mounted) {
                  ScaffoldMessenger.of(
                    dialogContext,
                  ).showSnackBar(SnackBar(content: Text('Address copied'.tr)));
                }
              },
              child: Text('Copy'.tr),
            ),
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: Text('Close'.tr),
            ),
          ],
        );
      },
    );
  }

  Future<void> _showWithdrawForm(BuildContext context, SwapAsset asset) async {
    final balance = _balanceFor(asset);
    final addressController = TextEditingController();
    final amountController = TextEditingController();

    final request = await showDialog<_WithdrawRequest>(
      context: context,
      builder: (dialogContext) {
        return AlertDialog(
          title: Text('${'Withdraw'.tr} ${asset.ticker}'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                '${'Available'.tr}: ${_formatBalance(balance)} ${asset.ticker}',
                style: PTypography.bodySmall(color: AppColors.textSecondary),
              ),
              const SizedBox(height: PSpacing.md),
              PInput(
                controller: addressController,
                label: '${asset.ticker} ${'address'.tr}',
                hint: asset == SwapAsset.ltc
                    ? 'ltc1... or L...'
                    : '${'Enter'.tr} ${asset.ticker} ${'address'.tr}',
                monospace: true,
                autofocus: true,
                autocorrect: false,
                enableSuggestions: false,
              ),
              const SizedBox(height: PSpacing.md),
              PInput(
                controller: amountController,
                label: '${'Amount'.tr} ${asset.ticker}',
                hint: '0.00',
                keyboardType: const TextInputType.numberWithOptions(
                  decimal: true,
                ),
                suffixIcon: balance == null
                    ? null
                    : TextButton(
                        onPressed: () {
                          amountController.text = formatSwapAmount(
                            balance,
                            fractionDigits: 8,
                          );
                        },
                        child: Text('Max'.tr),
                      ),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(),
              child: Text('Cancel'.tr),
            ),
            TextButton(
              onPressed: () {
                Navigator.of(dialogContext).pop(
                  _WithdrawRequest(
                    address: addressController.text.trim(),
                    amountText: amountController.text.trim(),
                  ),
                );
              },
              child: Text('Send'.tr),
            ),
          ],
        );
      },
    );

    addressController.dispose();
    amountController.dispose();

    if (request == null) return;
    final amount = _parseAmount(request.amountText);
    final validationError = _withdrawValidationError(
      asset: asset,
      address: request.address,
      amount: amount,
      balance: balance,
    );
    if (validationError != null) {
      if (context.mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text(validationError)));
      }
      return;
    }

    await onWithdrawAsset(
      asset: asset,
      address: request.address,
      amount: amount!,
    );
  }

  String? _withdrawValidationError({
    required SwapAsset asset,
    required String address,
    required Decimal? amount,
    required Decimal? balance,
  }) {
    if (address.trim().isEmpty) {
      return '${'Enter a'.tr} ${asset.ticker} ${'address'.tr}';
    }
    if (asset == SwapAsset.ltc) {
      final check = checkLtcAddress(address);
      if (!check.isValid) return check.error ?? 'Invalid LTC address'.tr;
    }
    if (amount == null || amount <= Decimal.zero) {
      return 'Enter a withdrawal amount.'.tr;
    }
    if (balance != null && amount > balance) {
      return '${'Amount exceeds available'.tr} ${asset.ticker} ${'balance'.tr}.';
    }
    return null;
  }

  Future<void> _confirmSweepArrr(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) {
        return AlertDialog(
          title: Text('Sweep ARRR to wallet'.tr),
          content: Text(
            '${'Send all available'.tr} ${_formatBalance(arrrBalance)} ARRR ${'from the KDF funding wallet back to your current Unified Wallet receive address? Open maker-order funds cannot be swept until the order is cancelled or filled.'.tr}',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(false),
              child: Text('Cancel'.tr),
            ),
            TextButton(
              onPressed: () => Navigator.of(dialogContext).pop(true),
              child: Text('Sweep'.tr),
            ),
          ],
        );
      },
    );
    if (confirmed ?? false) {
      await onSweepArrrToWallet();
    }
  }

  Decimal? _parseAmount(String text) {
    try {
      return Decimal.parse(text);
    } catch (_) {
      return null;
    }
  }

  String _formatBalance(Decimal? value) {
    if (value == null) return '--';
    return formatSwapAmount(value, fractionDigits: 8);
  }
}

class _WithdrawRequest {
  const _WithdrawRequest({required this.address, required this.amountText});

  final String address;
  final String amountText;
}

class _BalancePill extends StatelessWidget {
  const _BalancePill({
    required this.label,
    required this.amount,
    required this.highlight,
  });

  final String label;
  final String amount;
  final bool highlight;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: PSpacing.md,
        vertical: PSpacing.sm,
      ),
      decoration: BoxDecoration(
        color: highlight
            ? AppColors.warning.withValues(alpha: 0.1)
            : AppColors.backgroundElevated,
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        border: Border.all(
          color: highlight
              ? AppColors.warning.withValues(alpha: 0.28)
              : AppColors.borderSubtle,
        ),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            amount,
            style: PTypography.labelMedium(color: AppColors.textPrimary),
          ),
          const SizedBox(width: PSpacing.xs),
          Text(
            label,
            style: PTypography.labelSmall(color: AppColors.textSecondary),
          ),
        ],
      ),
    );
  }
}

class _Notice extends StatelessWidget {
  const _Notice({required this.icon, required this.text, required this.color});

  final IconData icon;
  final String text;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: PSpacing.sm),
        Expanded(
          child: Text(text, style: PTypography.bodySmall(color: color)),
        ),
      ],
    );
  }
}
