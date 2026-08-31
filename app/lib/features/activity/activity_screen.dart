// ignore_for_file: noop_primitive_operations

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/platform/platform_utils.dart';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/transaction_row_v2.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';

enum ActivityFilter { all, sent, received, pending, expired }

extension ActivityFilterLabel on ActivityFilter {
  String get label {
    switch (this) {
      case ActivityFilter.all:
        return 'All'.tr;
      case ActivityFilter.sent:
        return 'Sent'.tr;
      case ActivityFilter.received:
        return 'Received'.tr;
      case ActivityFilter.pending:
        return 'Pending'.tr;
      case ActivityFilter.expired:
        return 'Expired'.tr;
    }
  }
}

/// Activity screen showing full transaction history.
class ActivityScreen extends ConsumerStatefulWidget {
  const ActivityScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  ConsumerState<ActivityScreen> createState() => _ActivityScreenState();
}

class _ActivityScreenState extends ConsumerState<ActivityScreen> {
  final TextEditingController _searchController = TextEditingController();
  final ScrollController _scrollController = ScrollController();
  ActivityFilter _filter = ActivityFilter.all;
  String _query = '';
  Timer? _searchDebounce;

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(_loadMoreNearListEnd);
  }

  @override
  void dispose() {
    _searchController.dispose();
    _scrollController
      ..removeListener(_loadMoreNearListEnd)
      ..dispose();
    _searchDebounce?.cancel();
    super.dispose();
  }

  void _loadMoreNearListEnd() {
    if (!_scrollController.hasClients ||
        _scrollController.position.extentAfter > 600) {
      return;
    }
    unawaited(ref.read(activityHistoryProvider.notifier).loadMore());
  }

  void _fillViewportIfNeeded(ActivityHistoryState history) {
    if (!history.hasMore ||
        history.isLoadingMore ||
        history.loadMoreError != null) {
      return;
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scrollController.hasClients) {
        return;
      }
      _loadMoreNearListEnd();
    });
  }

  int _confirmationsForTx(TxInfo tx, int? currentHeight) {
    final txHeight = tx.height;
    if (txHeight == null || txHeight <= 0 || currentHeight == null) {
      return 0;
    }
    if (currentHeight < txHeight) {
      return 0;
    }
    return (currentHeight - txHeight) + 1;
  }

  bool _isConfirmedTx(TxInfo tx, int? currentHeight) {
    if (tx.confirmed) {
      return true;
    }
    return _confirmationsForTx(tx, currentHeight) >= 1;
  }

  List<TxInfo> _applyFilters(List<TxInfo> transactions, int? currentHeight) {
    Iterable<TxInfo> filtered = transactions;

    switch (_filter) {
      case ActivityFilter.sent:
        filtered = filtered.where((tx) => tx.amount < 0);
        break;
      case ActivityFilter.received:
        filtered = filtered.where((tx) => tx.amount >= 0);
        break;
      case ActivityFilter.pending:
        filtered = filtered.where(
          (tx) => !tx.expired && !_isConfirmedTx(tx, currentHeight),
        );
        break;
      case ActivityFilter.expired:
        filtered = filtered.where((tx) => tx.expired);
        break;
      case ActivityFilter.all:
        break;
    }

    final query = _query.trim().toLowerCase();
    if (query.isNotEmpty) {
      filtered = filtered.where((tx) {
        final memo = tx.memo?.toLowerCase() ?? '';
        // TxInfo doesn't have toAddress - search by txid and memo only
        return tx.txid.toLowerCase().contains(query) || memo.contains(query);
      });
    }

    return filtered.toList();
  }

  /// Convert PlatformInt64 timestamp to DateTime
  DateTime _convertPlatformInt64ToDateTime(PlatformInt64 timestamp) {
    final timestampValue = timestamp.toInt();
    return DateTime.fromMillisecondsSinceEpoch(timestampValue * 1000);
  }

  void _onSearchChanged(String value) {
    _searchDebounce?.cancel();
    _searchDebounce = Timer(const Duration(milliseconds: 150), () {
      if (mounted) {
        setState(() => _query = value);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final size = MediaQuery.of(context).size;
    final historyAsync = ref.watch(activityHistoryProvider);
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
    final screenWidth = size.width;
    final gutter = PSpacing.responsiveGutter(screenWidth);

    final content = historyAsync.when(
      data: (history) {
        final filtered = _applyFilters(history.transactions, currentHeight);
        _fillViewportIfNeeded(history);
        const headerCount = 4;
        final bodyCount = filtered.isEmpty ? 1 : filtered.length;
        final showFooter = filtered.isNotEmpty && history.hasMore;
        final itemCount = headerCount + bodyCount + (showFooter ? 1 : 0);

        return ListView.builder(
          controller: _scrollController,
          padding: EdgeInsets.fromLTRB(
            gutter,
            PSpacing.lg,
            gutter,
            PSpacing.lg,
          ),
          itemCount: itemCount,
          itemBuilder: (context, index) {
            if (index == 0) {
              return PInput(
                controller: _searchController,
                label: 'Search'.tr,
                hint: 'Search activity'.tr,
                prefixIcon: const Icon(Icons.search),
                textInputAction: TextInputAction.search,
                onChanged: _onSearchChanged,
              );
            }
            if (index == 1) {
              return const SizedBox(height: PSpacing.md);
            }
            if (index == 2) {
              return _FilterChips(
                selected: _filter,
                onSelected: (filter) => setState(() => _filter = filter),
              );
            }
            if (index == 3) {
              return const SizedBox(height: PSpacing.lg);
            }
            if (filtered.isEmpty) {
              if (history.hasMore) {
                return _ActivityLoadMoreState(
                  error: history.loadMoreError,
                  onRetry: () => unawaited(
                    ref.read(activityHistoryProvider.notifier).retryLoadMore(),
                  ),
                );
              }
              return const _ActivityEmptyState();
            }

            final txIndex = index - headerCount;
            if (txIndex >= filtered.length) {
              return _ActivityLoadMoreState(
                error: history.loadMoreError,
                onRetry: () => unawaited(
                  ref.read(activityHistoryProvider.notifier).retryLoadMore(),
                ),
              );
            }
            final tx = filtered[txIndex];
            return Padding(
              padding: const EdgeInsets.only(bottom: PSpacing.md),
              child: TransactionRowV2(
                isReceived: tx.amount >= 0,
                isConfirmed: _isConfirmedTx(tx, currentHeight),
                isExpired: tx.expired,
                amountText:
                    '${tx.amount >= 0 ? '+' : '-'}${(tx.amount.abs() / 100000000.0).toStringAsFixed(4)} ARRR',
                timestamp: _convertPlatformInt64ToDateTime(tx.timestamp),
                memo: tx.memo,
                onTap: () => context.push(
                  '/transaction/${tx.txid}?amount=${tx.amount.toInt()}',
                  extra: tx,
                ),
              ),
            );
          },
        );
      },
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => _ActivityErrorState(message: error.toString()),
    );

    final isMobile = PSpacing.isHandset(size);
    final isDesktop = isDesktopPlatform;
    final appBarActions = [
      ConnectionStatusIndicator(
        full: !isMobile,
        onTap: () => context.push('/settings/privacy-shield'),
      ),
      if (!isMobile) const WalletSwitcherButton(compact: true),
    ];

    if (!widget.useScaffold) {
      if (isDesktop) {
        return content;
      }
      return PScaffold(
        title: 'Activity'.tr,
        useSafeArea: false,
        appBar: PAppBar(title: 'Activity'.tr, actions: appBarActions),
        body: content,
      );
    }

    return PScaffold(
      title: 'Activity'.tr,
      appBar: isDesktop
          ? null
          : PAppBar(title: 'Activity'.tr, actions: appBarActions),
      body: content,
    );
  }
}

class _FilterChips extends StatelessWidget {
  const _FilterChips({required this.selected, required this.onSelected});

  final ActivityFilter selected;
  final ValueChanged<ActivityFilter> onSelected;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: ActivityFilter.values.map((filter) {
          final isSelected = filter == selected;
          return Padding(
            padding: const EdgeInsets.only(right: PSpacing.sm),
            child: _FilterChipButton(
              label: filter.label,
              isSelected: isSelected,
              onTap: () => onSelected(filter),
            ),
          );
        }).toList(),
      ),
    );
  }
}

class _FilterChipButton extends StatelessWidget {
  const _FilterChipButton({
    required this.label,
    required this.isSelected,
    required this.onTap,
  });

  final String label;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final background = isSelected
        ? AppColors.selectedBackground
        : AppColors.backgroundSurface;
    final border = isSelected
        ? AppColors.selectedBorder
        : AppColors.borderSubtle;
    final textColor = isSelected
        ? AppColors.textPrimary
        : AppColors.textSecondary;

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      child: Container(
        constraints: const BoxConstraints(minHeight: 48),
        alignment: Alignment.center,
        padding: const EdgeInsets.symmetric(
          horizontal: PSpacing.md,
          vertical: PSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(PSpacing.radiusFull),
          border: Border.all(color: border),
        ),
        child: Text(
          label,
          textAlign: TextAlign.center,
          style: PTypography.labelSmall(color: textColor),
        ),
      ),
    );
  }
}

class _ActivityEmptyState extends StatelessWidget {
  const _ActivityEmptyState();

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
              'Nothing here yet.'.tr,
              style: PTypography.titleMedium(color: AppColors.textPrimary),
            ),
          ],
        ),
      ),
    );
  }
}

class _ActivityLoadMoreState extends StatelessWidget {
  const _ActivityLoadMoreState({required this.error, required this.onRetry});

  final Object? error;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    if (error != null) {
      return Center(
        child: Padding(
          key: const ValueKey('activity-load-more-error'),
          padding: const EdgeInsets.symmetric(vertical: PSpacing.md),
          child: IconButton(
            tooltip: 'Retry'.tr,
            onPressed: onRetry,
            icon: const Icon(Icons.refresh),
          ),
        ),
      );
    }
    return const Center(
      child: Padding(
        key: ValueKey('activity-loading-more'),
        padding: EdgeInsets.symmetric(vertical: PSpacing.lg),
        child: SizedBox.square(
          dimension: 24,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      ),
    );
  }
}

class _ActivityErrorState extends StatelessWidget {
  const _ActivityErrorState({required this.message});

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
              'Unable to load activity'.tr,
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
