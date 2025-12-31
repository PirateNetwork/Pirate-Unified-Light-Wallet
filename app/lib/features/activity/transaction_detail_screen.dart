import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:timeago/timeago.dart' as timeago;

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Transaction detail screen.
class TransactionDetailScreen extends ConsumerWidget {
  const TransactionDetailScreen({super.key, required this.txid});

  final String txid;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final transactionsAsync = ref.watch(transactionsProvider);

    final content = transactionsAsync.when(
      data: (txs) {
        final tx = txs.where((item) => item.txid == txid).firstOrNull;
        if (tx == null) {
          return _TransactionMissing(txid: txid);
        }
        return _TransactionDetails(tx: tx);
      },
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => _TransactionError(message: error.toString()),
    );

    return PScaffold(
      title: 'Transaction',
      appBar: const PAppBar(
        title: 'Transaction',
        actions: [WalletSwitcherButton(compact: true)],
      ),
      body: content,
    );
  }
}

class _TransactionDetails extends StatelessWidget {
  const _TransactionDetails({required this.tx});

  final TxInfo tx;

  /// Convert PlatformInt64 timestamp to DateTime
  DateTime _convertTimestamp(PlatformInt64 timestamp) {
    int timestampValue;
    if (timestamp is int) {
      timestampValue = timestamp;
    } else {
      timestampValue = (timestamp as dynamic).toInt() as int;
    }
    return DateTime.fromMillisecondsSinceEpoch(timestampValue * 1000);
  }

  @override
  Widget build(BuildContext context) {
    final isReceived = tx.amount >= 0;
    final amountArrr = _formatArrr(tx.amount.abs());
    final feeArrr = _formatArrr(tx.fee.toInt());
    final statusText = tx.confirmed ? 'Confirmed' : 'Confirming';
    final statusColor = tx.confirmed ? AppColors.success : AppColors.warning;
    final timestamp = _convertTimestamp(tx.timestamp);
    final localizations = MaterialLocalizations.of(context);
    final dateText = localizations.formatFullDate(timestamp);
    final timeText = localizations.formatTimeOfDay(
      TimeOfDay.fromDateTime(timestamp),
    );
    final relativeText = timeago.format(timestamp);
    final timestampValue = '$dateText at $timeText ($relativeText)';

    return ListView(
      padding: const EdgeInsets.all(PSpacing.lg),
      children: [
        PCard(
          child: Padding(
            padding: const EdgeInsets.all(PSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  isReceived ? 'Received' : 'Sent',
                  style: PTypography.heading4(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.xs),
                Text(
                  amountArrr,
                  style: PTypography.displaySmall(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.md),
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: PSpacing.sm,
                    vertical: PSpacing.xxs,
                  ),
                  decoration: BoxDecoration(
                    color: statusColor.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(PSpacing.radiusFull),
                  ),
                  child: Text(
                    statusText,
                    style: PTypography.labelSmall(color: statusColor),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: PSpacing.lg),
        PCard(
          child: Padding(
            padding: const EdgeInsets.all(PSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Summary',
                  style: PTypography.titleMedium(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.md),
                _DetailRow(label: 'Amount', value: amountArrr),
                const SizedBox(height: PSpacing.sm),
                _DetailRow(label: 'Network fee', value: feeArrr),
                const SizedBox(height: PSpacing.sm),
                _DetailRow(
                  label: 'Date and time',
                  value: timestampValue,
                ),
                if (tx.height != null) ...[
                  const SizedBox(height: PSpacing.sm),
                  _DetailRow(
                    label: 'Block height',
                    value: tx.height.toString(),
                  ),
                ],
              ],
            ),
          ),
        ),
        // Note: TxInfo doesn't have toAddress field - address info would need to come from transaction details
        // if (tx.toAddress != null) ...[
        //   const SizedBox(height: PSpacing.lg),
        //   PCard(
        //     child: Padding(
        //       padding: const EdgeInsets.all(PSpacing.md),
        //       child: Column(
        //         crossAxisAlignment: CrossAxisAlignment.start,
        //         children: [
        //           Text(
        //             isReceived ? 'From' : 'To',
        //             style: PTypography.titleMedium(color: AppColors.textPrimary),
        //           ),
        //           const SizedBox(height: PSpacing.sm),
        //           SelectableText(
        //             tx.toAddress!,
        //             style: PTypography.codeMedium(color: AppColors.textSecondary),
        //           ),
        //         ],
        //       ),
        //     ),
        //   ),
        // ],
        if (tx.memo != null && tx.memo!.isNotEmpty) ...[
          const SizedBox(height: PSpacing.lg),
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(PSpacing.md),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Memo',
                    style: PTypography.titleMedium(color: AppColors.textPrimary),
                  ),
                  const SizedBox(height: PSpacing.sm),
                  Text(
                    tx.memo!,
                    style: PTypography.bodyMedium(color: AppColors.textSecondary),
                  ),
                ],
              ),
            ),
          ),
        ],
        const SizedBox(height: PSpacing.lg),
        PCard(
          child: Padding(
            padding: const EdgeInsets.all(PSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Transaction ID',
                  style: PTypography.titleMedium(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.sm),
                SelectableText(
                  tx.txid,
                  style: PTypography.codeMedium(color: AppColors.textSecondary),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: PSpacing.lg),
        PButton(
          text: 'Copy transaction ID',
          onPressed: () {
            Clipboard.setData(ClipboardData(text: tx.txid));
            ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(content: Text('Transaction ID copied')),
            );
          },
          variant: PButtonVariant.secondary,
          fullWidth: true,
        ),
      ],
    );
  }

  String _formatArrr(int zatoshis) {
    return '${(zatoshis / 100000000).toStringAsFixed(8)} ARRR';
  }
}

class _DetailRow extends StatelessWidget {
  const _DetailRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Text(
          label,
          style: PTypography.bodySmall(color: AppColors.textSecondary),
        ),
        Text(
          value,
          style: PTypography.bodySmall(color: AppColors.textPrimary),
        ),
      ],
    );
  }
}

class _TransactionMissing extends StatelessWidget {
  const _TransactionMissing({required this.txid});

  final String txid;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.xl),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.receipt_long,
              size: 48,
              color: AppColors.textTertiary,
            ),
            const SizedBox(height: PSpacing.md),
            Text(
              'Transaction not found',
              style: PTypography.titleMedium(color: AppColors.textPrimary),
            ),
            const SizedBox(height: PSpacing.xs),
            SelectableText(
              txid,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}

class _TransactionError extends StatelessWidget {
  const _TransactionError({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.xl),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.error_outline,
              size: 48,
              color: AppColors.error,
            ),
            const SizedBox(height: PSpacing.md),
            Text(
              'Unable to load transaction',
              style: PTypography.titleMedium(color: AppColors.textPrimary),
            ),
            const SizedBox(height: PSpacing.xs),
            Text(
              message,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}
