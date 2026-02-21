// ignore_for_file: noop_primitive_operations

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
import '../../ui/organisms/p_skeleton.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// Transaction detail screen.
class TransactionDetailScreen extends ConsumerWidget {
  const TransactionDetailScreen({super.key, required this.txid, this.amount});

  final String txid;
  final int? amount;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final transactionsAsync = ref.watch(transactionsProvider);

    final content = transactionsAsync.when(
      data: (txs) {
        TxInfo? tx;
        if (amount != null) {
          for (final item in txs) {
            if (item.txid == txid && item.amount.toInt() == amount) {
              tx = item;
              break;
            }
          }
        }
        tx ??= txs.where((item) => item.txid == txid).firstOrNull;
        if (tx == null) {
          return _TransactionMissing(txid: txid);
        }
        return _TransactionDetails(tx: tx);
      },
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => _TransactionError(message: error.toString()),
    );

    return PScaffold(
      title: 'Transaction'.tr,
      appBar: PAppBar(
        title: 'Transaction'.tr,
        actions: [WalletSwitcherButton(compact: true)],
      ),
      body: content,
    );
  }
}

class _TransactionDetails extends ConsumerStatefulWidget {
  const _TransactionDetails({required this.tx});

  final TxInfo tx;

  @override
  ConsumerState<_TransactionDetails> createState() =>
      _TransactionDetailsState();
}

class _TransactionDetailsState extends ConsumerState<_TransactionDetails> {
  Future<String?>? _memoFuture;

  @override
  void initState() {
    super.initState();
    _refreshMemoFuture();
  }

  @override
  void didUpdateWidget(covariant _TransactionDetails oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.tx.txid != widget.tx.txid ||
        (oldWidget.tx.memo ?? '') != (widget.tx.memo ?? '')) {
      _refreshMemoFuture();
    }
  }

  void _refreshMemoFuture() {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      _memoFuture = null;
      return;
    }
    if (widget.tx.memo != null && widget.tx.memo!.isNotEmpty) {
      _memoFuture = null;
      return;
    }
    _memoFuture =
        FfiBridge.fetchTransactionMemo(
          walletId: walletId,
          txid: widget.tx.txid,
        ).then((memo) {
          if (mounted && memo != null && memo.isNotEmpty) {
            ref.invalidate(transactionsProvider);
          }
          return memo;
        });
  }

  /// Convert PlatformInt64 timestamp to DateTime
  DateTime _convertTimestamp(PlatformInt64 timestamp) {
    final timestampValue = timestamp.toInt();
    return DateTime.fromMillisecondsSinceEpoch(timestampValue * 1000);
  }

  int _confirmationsFor({
    required int? txHeight,
    required int? currentHeight,
  }) {
    if (txHeight == null || txHeight <= 0 || currentHeight == null) {
      return 0;
    }
    if (currentHeight < txHeight) {
      return 0;
    }
    return (currentHeight - txHeight) + 1;
  }

  int _displayFeeArrrtoshis(TxInfo tx) {
    final recordedFee = tx.fee.toInt();
    if (recordedFee > 0) {
      return recordedFee;
    }
    // Some historic records have missing stored fees for outgoing txs.
    // Pirate uses a fixed minimum fee, so show that instead of 0.
    if (tx.amount < 0) {
      return FfiBridge.minFee;
    }
    return 0;
  }

  @override
  Widget build(BuildContext context) {
    final padding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    final tx = widget.tx;
    final isReceived = tx.amount >= 0;
    final amountArrr = _formatArrr(tx.amount.abs());
    final displayFeeArrrtoshis = _displayFeeArrrtoshis(tx);
    final feeArrr = _formatArrr(displayFeeArrrtoshis);
    final syncProgressStatus = ref
        .watch(syncProgressStreamProvider)
        .asData
        ?.value;
    final syncStatus = ref.watch(syncStatusProvider).asData?.value;
    final currentHeight =
        (syncProgressStatus?.targetHeight ??
                syncProgressStatus?.localHeight ??
                syncStatus?.targetHeight ??
                syncStatus?.localHeight)
            ?.toInt();
    final confirmations = _confirmationsFor(
      txHeight: tx.height,
      currentHeight: currentHeight,
    );
    final isConfirmed = tx.confirmed || confirmations >= 10;
    final statusText = isConfirmed ? 'Confirmed' : 'Pending';
    final statusColor = isConfirmed ? AppColors.success : AppColors.warning;
    final timestamp = _convertTimestamp(tx.timestamp);
    final localizations = MaterialLocalizations.of(context);
    final dateText = localizations.formatFullDate(timestamp);
    final timeText = localizations.formatTimeOfDay(
      TimeOfDay.fromDateTime(timestamp),
    );
    final relativeText = timeago.format(timestamp);
    final timestampValue = '$dateText at $timeText ($relativeText)';

    return ListView(
      padding: padding,
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
                  'Summary'.tr,
                  style: PTypography.titleMedium(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.md),
                _DetailRow(label: 'Amount'.tr, value: amountArrr),
                const SizedBox(height: PSpacing.sm),
                _DetailRow(label: 'Network fee'.tr, value: feeArrr),
                const SizedBox(height: PSpacing.sm),
                _DetailRow(label: 'Date and time'.tr, value: timestampValue),
                if (tx.height != null) ...[
                  const SizedBox(height: PSpacing.sm),
                  _DetailRow(
                    label: 'Block height'.tr,
                    value: tx.height.toString(),
                  ),
                  const SizedBox(height: PSpacing.sm),
                  _DetailRow(
                    label: 'Confirmations'.tr,
                    value: confirmations.toString(),
                  ),
                ],
              ],
            ),
          ),
        ),
        if (displayFeeArrrtoshis > 0 &&
            tx.amount.abs() == displayFeeArrrtoshis) ...[
          const SizedBox(height: PSpacing.lg),
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(PSpacing.md),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(
                    Icons.swap_horiz,
                    color: AppColors.textSecondary,
                    size: 20,
                  ),
                  const SizedBox(width: PSpacing.sm),
                  Expanded(
                    child: Text(
                      'Internal transfer between your addresses. Net change equals the network fee.'
                          .tr,
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
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
          _MemoCard(memo: tx.memo!),
        ] else if (_memoFuture != null) ...[
          const SizedBox(height: PSpacing.lg),
          FutureBuilder<String?>(
            future: _memoFuture,
            builder: (context, snapshot) {
              if (snapshot.connectionState == ConnectionState.waiting) {
                return _MemoLoadingCard();
              }
              final memo = snapshot.data;
              if (memo == null || memo.isEmpty) {
                return const SizedBox.shrink();
              }
              return _MemoCard(memo: memo);
            },
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
                  'Transaction ID'.tr,
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
            ScaffoldMessenger.of(
              context,
            ).showSnackBar(SnackBar(content: Text('Transaction ID copied'.tr)));
          },
          variant: PButtonVariant.secondary,
          fullWidth: true,
        ),
      ],
    );
  }

  String _formatArrr(int arrrtoshis) {
    return '${(arrrtoshis / 100000000).toStringAsFixed(8)} ARRR';
  }
}

class _DetailRow extends StatelessWidget {
  const _DetailRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: Text(
            label,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
        ),
        const SizedBox(width: PSpacing.sm),
        Expanded(
          child: Text(
            value,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.right,
            style: PTypography.bodySmall(color: AppColors.textPrimary),
          ),
        ),
      ],
    );
  }
}

class _MemoCard extends StatelessWidget {
  const _MemoCard({required this.memo});

  final String memo;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text(
                  'Memo'.tr,
                  style: PTypography.titleMedium(color: AppColors.textPrimary),
                ),
                const Spacer(),
                IconButton(
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: memo));
                    ScaffoldMessenger.of(
                      context,
                    ).showSnackBar(SnackBar(content: Text('Memo copied'.tr)));
                  },
                  icon: const Icon(Icons.copy, size: 18),
                  tooltip: 'Copy memo'.tr,
                  color: AppColors.textSecondary,
                ),
              ],
            ),
            const SizedBox(height: PSpacing.sm),
            SelectableText(
              memo,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ),
      ),
    );
  }
}

class _MemoLoadingCard extends StatelessWidget {
  const _MemoLoadingCard();

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: const [
            PSkeleton(width: 64, height: 18),
            SizedBox(height: PSpacing.sm),
            PSkeleton(width: double.infinity, height: 14),
            SizedBox(height: PSpacing.xs),
            PSkeleton(width: 220, height: 14),
          ],
        ),
      ),
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
        padding: PSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: PSpacing.xl,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.receipt_long, size: 48, color: AppColors.textTertiary),
            const SizedBox(height: PSpacing.md),
            Text(
              'Transaction not found'.tr,
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
        padding: PSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: PSpacing.xl,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, size: 48, color: AppColors.error),
            const SizedBox(height: PSpacing.md),
            Text(
              'Unable to load transaction'.tr,
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
