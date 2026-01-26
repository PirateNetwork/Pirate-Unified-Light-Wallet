// ignore_for_file: noop_primitive_operations

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/transaction_row_v2.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

enum ActivityFilter {
  all,
  sent,
  received,
  pending,
}

extension ActivityFilterLabel on ActivityFilter {
  String get label {
    switch (this) {
      case ActivityFilter.all:
        return 'All';
      case ActivityFilter.sent:
        return 'Sent';
      case ActivityFilter.received:
        return 'Received';
      case ActivityFilter.pending:
        return 'Pending';
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
  ActivityFilter _filter = ActivityFilter.all;
  String _query = '';
  Timer? _searchDebounce;

  @override
  void dispose() {
    _searchController.dispose();
    _searchDebounce?.cancel();
    super.dispose();
  }

  List<TxInfo> _applyFilters(List<TxInfo> transactions) {
    Iterable<TxInfo> filtered = transactions;

    switch (_filter) {
      case ActivityFilter.sent:
        filtered = filtered.where((tx) => tx.amount < 0);
        break;
      case ActivityFilter.received:
        filtered = filtered.where((tx) => tx.amount >= 0);
        break;
      case ActivityFilter.pending:
        filtered = filtered.where((tx) => !tx.confirmed);
        break;
      case ActivityFilter.all:
        break;
    }

    final query = _query.trim().toLowerCase();
    if (query.isNotEmpty) {
      filtered = filtered.where((tx) {
        final memo = tx.memo?.toLowerCase() ?? '';
        // TxInfo doesn't have toAddress - search by txid and memo only
        return tx.txid.toLowerCase().contains(query) ||
            memo.contains(query);
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
    final transactionsAsync = ref.watch(transactionsProvider);
    final screenWidth = size.width;
    final gutter = PSpacing.responsiveGutter(screenWidth);

    final content = transactionsAsync.when(
      data: (txs) {
        final filtered = _applyFilters(txs);
        const headerCount = 4;
        final bodyCount = filtered.isEmpty ? 1 : filtered.length;
        final itemCount = headerCount + bodyCount;

        return ListView.builder(
          padding: EdgeInsets.fromLTRB(gutter, PSpacing.lg, gutter, PSpacing.lg),
          itemCount: itemCount,
          itemBuilder: (context, index) {
            if (index == 0) {
              return PInput(
                controller: _searchController,
                label: 'Search',
                hint: 'Search activity',
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
              return const _ActivityEmptyState();
            }

            final txIndex = index - headerCount;
            if (txIndex < 0 || txIndex >= filtered.length) {
              return const SizedBox.shrink();
            }
            final tx = filtered[txIndex];
            return Padding(
              padding: const EdgeInsets.only(bottom: PSpacing.md),
              child: TransactionRowV2(
                isReceived: tx.amount >= 0,
                isConfirmed: tx.confirmed,
                amountText:
                    '${tx.amount >= 0 ? '+' : '-'}${(tx.amount.abs() / 100000000.0).toStringAsFixed(4)} ARRR',
                timestamp: _convertPlatformInt64ToDateTime(tx.timestamp),
                memo: tx.memo,
                onTap: () => context.push('/transaction/${tx.txid}'),
              ),
            );
          },
        );
      },
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => _ActivityErrorState(message: error.toString()),
    );

    final isMobile = PSpacing.isMobile(screenWidth);
    final isDesktop = PSpacing.isDesktop(screenWidth);
    final appBarActions =
        isMobile ? null : const [WalletSwitcherButton(compact: true)];

    if (!widget.useScaffold) {
      if (isDesktop) {
        return content;
      }
      return PScaffold(
        title: 'Activity',
        useSafeArea: false,
        appBar: PAppBar(
          title: 'Activity',
          actions: appBarActions,
        ),
        body: content,
      );
    }

    return PScaffold(
      title: 'Activity',
      appBar: isDesktop
          ? null
          : PAppBar(
              title: 'Activity',
              actions: appBarActions,
            ),
      body: content,
    );
  }
}

class _FilterChips extends StatelessWidget {
  const _FilterChips({
    required this.selected,
    required this.onSelected,
  });

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
    final border = isSelected ? AppColors.selectedBorder : AppColors.borderSubtle;
    final textColor = isSelected ? AppColors.textPrimary : AppColors.textSecondary;

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      child: Container(
        constraints: const BoxConstraints(minHeight: 48),
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
            Icon(
              Icons.receipt_long,
              size: 48,
              color: AppColors.textTertiary,
            ),
            const SizedBox(height: PSpacing.md),
            Text(
              'Nothing here yet.',
              style: PTypography.titleMedium(color: AppColors.textPrimary),
            ),
          ],
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
            Icon(
              Icons.error_outline,
              size: 48,
              color: AppColors.error,
            ),
            const SizedBox(height: PSpacing.md),
            Text(
              'Unable to load activity',
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
