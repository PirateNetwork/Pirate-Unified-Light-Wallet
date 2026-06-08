import 'dart:async';
import 'dart:convert';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:share_plus/share_plus.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/swap_amount_utils.dart';
import '../../../core/swaps/swap_history_csv.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../core/swaps/swap_providers.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_card.dart';

class SwapOpenOrdersPanel extends StatelessWidget {
  const SwapOpenOrdersPanel({
    required this.intents,
    required this.onCancel,
    required this.onResume,
    super.key,
  });

  final List<SwapIntent> intents;
  final Future<void> Function(SwapIntent intent) onCancel;
  final void Function(SwapIntent intent) onResume;

  @override
  Widget build(BuildContext context) {
    if (intents.isEmpty) return const SizedBox.shrink();

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            'Open swap orders'.tr,
            style: PTypography.heading6(color: AppColors.textPrimary),
          ),
          const SizedBox(height: PSpacing.md),
          ...intents.map(
            (intent) => Padding(
              padding: const EdgeInsets.only(bottom: PSpacing.sm),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          intent.side == SwapSide.buyArrr
                              ? 'Buy ARRR'.tr
                              : 'Sell ARRR'.tr,
                          style: PTypography.titleMedium(
                            color: AppColors.textPrimary,
                          ),
                        ),
                        Text(
                          intent.status.name,
                          style: PTypography.bodySmall(
                            color: AppColors.textSecondary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  if (intent.status == SwapIntentStatus.limitOrderPlaced)
                    PButton(
                      text: 'Cancel'.tr,
                      variant: PButtonVariant.ghost,
                      onPressed: () => onCancel(intent),
                    )
                  else if (intent.status == SwapIntentStatus.waitingForDeposit)
                    PButton(
                      text: 'Resume'.tr,
                      variant: PButtonVariant.secondary,
                      onPressed: () => onResume(intent),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

Future<void> cancelSwapIntent(WidgetRef ref, SwapIntent intent) async {
  final service = ref.read(atomicSwapServiceProvider);
  await service.cancelLimitOrder(intent);
}

/// Compact list of recent swaps. Active rows can be resumed, and completed
/// rows can be reopened so users can inspect the final swap details later.
class SwapHistoryPanel extends StatefulWidget {
  const SwapHistoryPanel({
    required this.intents,
    required this.onResume,
    super.key,
  });

  final List<SwapIntent> intents;
  final void Function(SwapIntent intent) onResume;

  @override
  State<SwapHistoryPanel> createState() => _SwapHistoryPanelState();
}

class _SwapHistoryPanelState extends State<SwapHistoryPanel> {
  static const _pageSize = 5;

  _HistoryFilter _filter = _HistoryFilter.all;
  int _page = 0;
  bool _isExporting = false;

  @override
  void didUpdateWidget(SwapHistoryPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.intents != widget.intents) {
      final count = _filtered(widget.intents).length;
      final maxPage = _maxPage(count);
      if (_page > maxPage) _page = maxPage;
    }
  }

  @override
  Widget build(BuildContext context) {
    if (widget.intents.isEmpty) return const SizedBox.shrink();

    final filtered = _filtered(widget.intents);
    final maxPage = _maxPage(filtered.length);
    final page = _page.clamp(0, maxPage);
    final rows = filtered.skip(page * _pageSize).take(_pageSize).toList();

    return Padding(
      padding: const EdgeInsets.only(top: PSpacing.lg),
      child: PCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    'Recent swaps'.tr,
                    style: PTypography.heading6(color: AppColors.textPrimary),
                  ),
                ),
                IconButton(
                  tooltip: 'Export CSV'.tr,
                  icon: _isExporting
                      ? SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: AppColors.accentPrimary,
                          ),
                        )
                      : const Icon(Icons.file_download_outlined),
                  onPressed: _isExporting
                      ? null
                      : () => unawaited(_exportCsv(context)),
                ),
                Text(
                  '${filtered.length}',
                  style: PTypography.labelMedium(
                    color: AppColors.textSecondary,
                  ),
                ),
              ],
            ),
            const SizedBox(height: PSpacing.md),
            Wrap(
              spacing: PSpacing.sm,
              runSpacing: PSpacing.sm,
              children: [
                for (final filter in _HistoryFilter.values)
                  ChoiceChip(
                    label: Text(filter.label.tr),
                    selected: _filter == filter,
                    onSelected: (_) {
                      setState(() {
                        _filter = filter;
                        _page = 0;
                      });
                    },
                  ),
              ],
            ),
            const SizedBox(height: PSpacing.md),
            if (rows.isEmpty)
              _EmptyHistoryFilter(filter: _filter)
            else
              for (final intent in rows)
                _HistoryRow(
                  intent: intent,
                  onResume: _canOpen(intent)
                      ? () => widget.onResume(intent)
                      : null,
                ),
            if (maxPage > 0) ...[
              const SizedBox(height: PSpacing.sm),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  Text(
                    '${page + 1} / ${maxPage + 1}',
                    style: PTypography.labelSmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  IconButton(
                    tooltip: 'Previous'.tr,
                    icon: const Icon(Icons.chevron_left),
                    onPressed: page == 0
                        ? null
                        : () => setState(() => _page = page - 1),
                  ),
                  IconButton(
                    tooltip: 'Next'.tr,
                    icon: const Icon(Icons.chevron_right),
                    onPressed: page >= maxPage
                        ? null
                        : () => setState(() => _page = page + 1),
                  ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }

  List<SwapIntent> _filtered(List<SwapIntent> intents) {
    final sorted = intents.toList()
      ..sort((a, b) => b.updatedAt.compareTo(a.updatedAt));
    return switch (_filter) {
      _HistoryFilter.all => sorted,
      _HistoryFilter.active => sorted.where(_isActive).toList(),
      _HistoryFilter.completed =>
        sorted
            .where((intent) => intent.status == SwapIntentStatus.completed)
            .toList(),
      _HistoryFilter.failed =>
        sorted
            .where((intent) => intent.status == SwapIntentStatus.failed)
            .toList(),
      _HistoryFilter.cancelled =>
        sorted
            .where((intent) => intent.status == SwapIntentStatus.cancelled)
            .toList(),
    };
  }

  int _maxPage(int count) => count == 0 ? 0 : ((count - 1) ~/ _pageSize);

  Future<void> _exportCsv(BuildContext context) async {
    setState(() => _isExporting = true);
    try {
      final fileName = _csvFileName();
      final csv = buildSwapHistoryCsv(widget.intents);
      final file = XFile.fromData(
        Uint8List.fromList(utf8.encode(csv)),
        mimeType: 'text/csv',
        name: fileName,
      );

      if (_usesDesktopSaveDialog) {
        final location = await getSaveLocation(
          suggestedName: fileName,
          acceptedTypeGroups: [
            XTypeGroup(
              label: 'CSV'.tr,
              extensions: ['csv'],
              mimeTypes: ['text/csv'],
            ),
          ],
        );
        if (location == null) return;
        await file.saveTo(location.path);
        if (context.mounted) {
          _showExportSnack(context, 'Swap history exported.'.tr);
        }
      } else {
        await SharePlus.instance.share(
          ShareParams(files: [file], text: 'Pirate Wallet swap history'.tr),
        );
        if (context.mounted) {
          _showExportSnack(context, 'Swap history ready to share.'.tr);
        }
      }
    } catch (error) {
      if (context.mounted) {
        _showExportSnack(
          context,
          '${'Failed to export swap history'.tr}: $error',
          isError: true,
        );
      }
    } finally {
      if (mounted) setState(() => _isExporting = false);
    }
  }

  bool get _usesDesktopSaveDialog {
    if (kIsWeb) return false;
    return switch (defaultTargetPlatform) {
      TargetPlatform.linux ||
      TargetPlatform.macOS ||
      TargetPlatform.windows => true,
      _ => false,
    };
  }

  String _csvFileName() {
    final now = DateTime.now().toUtc();
    return 'pirate-swap-history-${now.year}${_two(now.month)}${_two(now.day)}-'
        '${_two(now.hour)}${_two(now.minute)}${_two(now.second)}.csv';
  }

  void _showExportSnack(
    BuildContext context,
    String message, {
    bool isError = false,
  }) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: isError ? AppColors.error : AppColors.success,
        duration: const Duration(seconds: 2),
      ),
    );
  }

  static String _two(int value) => value.toString().padLeft(2, '0');

  static bool _isActive(SwapIntent intent) {
    return switch (intent.status) {
      SwapIntentStatus.prepared ||
      SwapIntentStatus.waitingForDeposit ||
      SwapIntentStatus.marketSwapStarted ||
      SwapIntentStatus.limitOrderPlaced => true,
      _ => false,
    };
  }

  static bool _canOpen(SwapIntent intent) {
    return _isActive(intent) || intent.status == SwapIntentStatus.completed;
  }
}

enum _HistoryFilter { all, active, completed, failed, cancelled }

extension _HistoryFilterLabel on _HistoryFilter {
  String get label {
    return switch (this) {
      _HistoryFilter.all => 'All',
      _HistoryFilter.active => 'Active',
      _HistoryFilter.completed => 'Completed',
      _HistoryFilter.failed => 'Failed',
      _HistoryFilter.cancelled => 'Cancelled',
    };
  }
}

class _EmptyHistoryFilter extends StatelessWidget {
  const _EmptyHistoryFilter({required this.filter});

  final _HistoryFilter filter;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.backgroundElevated.withValues(alpha: 0.55),
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
      ),
      child: Row(
        children: [
          Icon(Icons.filter_list_off, color: AppColors.textSecondary, size: 18),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              'No ${filter.label.toLowerCase()} swaps yet.'.tr,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ),
        ],
      ),
    );
  }
}

class _HistoryRow extends StatelessWidget {
  const _HistoryRow({required this.intent, this.onResume});

  final SwapIntent intent;
  final VoidCallback? onResume;

  @override
  Widget build(BuildContext context) {
    final isBuy = intent.side == SwapSide.buyArrr;
    final status = _statusFor(intent.status);
    final pay = formatSwapAmount(intent.requestedPayAmount, fractionDigits: 8);
    final receive = formatSwapAmount(
      intent.expectedReceiveAmount,
      fractionDigits: 8,
    );
    final relTicker = intent.pair.relTicker;
    final actionLabel = _actionLabel(intent.status);
    final canResume = onResume != null && actionLabel != null;

    final content = Padding(
      padding: const EdgeInsets.only(bottom: PSpacing.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '${isBuy ? 'Buy ARRR'.tr : 'Sell ARRR'.tr} - ${intent.pair.displayName}',
                  style: PTypography.titleSmall(color: AppColors.textPrimary),
                ),
                const SizedBox(height: 2),
                Text(
                  isBuy
                      ? '$pay $relTicker -> $receive ARRR'
                      : '$pay ARRR -> $receive $relTicker',
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                ),
                const SizedBox(height: 2),
                Text(
                  'Updated ${_timeLabel(intent.updatedAt)}'.tr,
                  style: PTypography.labelSmall(color: AppColors.textSecondary),
                ),
                if (intent.status == SwapIntentStatus.failed &&
                    (intent.lastError?.isNotEmpty ?? false)) ...[
                  const SizedBox(height: 2),
                  Text(
                    intent.lastError!,
                    style: PTypography.labelSmall(color: AppColors.error),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(width: PSpacing.sm),
          if (canResume)
            _ResumeStatusPill(status: status, actionLabel: actionLabel)
          else
            _StatusBadge(status: status),
        ],
      ),
    );

    if (onResume == null) return content;
    return Material(
      color: Colors.transparent,
      child: InkWell(
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        onTap: onResume,
        child: content,
      ),
    );
  }

  static ({Color color, String label}) _statusFor(SwapIntentStatus status) {
    return switch (status) {
      SwapIntentStatus.prepared => (
        color: AppColors.warning,
        label: 'Prepared'.tr,
      ),
      SwapIntentStatus.waitingForDeposit => (
        color: AppColors.warning,
        label: 'Awaiting deposit'.tr,
      ),
      SwapIntentStatus.marketSwapStarted => (
        color: AppColors.accentPrimary,
        label: 'In progress'.tr,
      ),
      SwapIntentStatus.limitOrderPlaced => (
        color: AppColors.accentPrimary,
        label: 'Limit open'.tr,
      ),
      SwapIntentStatus.completed => (
        color: AppColors.success,
        label: 'Completed'.tr,
      ),
      SwapIntentStatus.cancelled => (
        color: AppColors.textSecondary,
        label: 'Cancelled'.tr,
      ),
      SwapIntentStatus.failed => (color: AppColors.error, label: 'Failed'.tr),
    };
  }

  static String? _actionLabel(SwapIntentStatus status) {
    return switch (status) {
      SwapIntentStatus.waitingForDeposit => 'Resume'.tr,
      SwapIntentStatus.marketSwapStarted => 'Continue'.tr,
      SwapIntentStatus.limitOrderPlaced => 'View'.tr,
      SwapIntentStatus.prepared => 'View'.tr,
      SwapIntentStatus.completed => 'View'.tr,
      _ => null,
    };
  }

  static String _timeLabel(DateTime value) {
    final local = value.toLocal();
    return '${local.year}-${_two(local.month)}-${_two(local.day)} '
        '${_two(local.hour)}:${_two(local.minute)}';
  }

  static String _two(int value) => value.toString().padLeft(2, '0');
}

class _StatusBadge extends StatelessWidget {
  const _StatusBadge({required this.status});

  final ({Color color, String label}) status;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: PSpacing.sm, vertical: 2),
      decoration: BoxDecoration(
        color: status.color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      ),
      child: Text(
        status.label,
        style: PTypography.labelSmall(color: status.color),
        textAlign: TextAlign.center,
      ),
    );
  }
}

class _ResumeStatusPill extends StatelessWidget {
  const _ResumeStatusPill({required this.status, required this.actionLabel});

  final ({Color color, String label}) status;
  final String actionLabel;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: PSpacing.sm,
        vertical: PSpacing.xs,
      ),
      decoration: BoxDecoration(
        color: status.color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(PSpacing.radiusFull),
        border: Border.all(color: status.color.withValues(alpha: 0.18)),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Text(
            status.label,
            style: PTypography.labelSmall(color: status.color),
            textAlign: TextAlign.center,
          ),
          const SizedBox(width: PSpacing.xs),
          Container(
            width: 3,
            height: 3,
            decoration: BoxDecoration(
              color: AppColors.textSecondary.withValues(alpha: 0.6),
              shape: BoxShape.circle,
            ),
          ),
          const SizedBox(width: PSpacing.xs),
          Text(
            actionLabel,
            style: PTypography.labelSmall(color: AppColors.accentPrimary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(width: 2),
          Icon(Icons.chevron_right, color: AppColors.accentPrimary, size: 16),
        ],
      ),
    );
  }
}
