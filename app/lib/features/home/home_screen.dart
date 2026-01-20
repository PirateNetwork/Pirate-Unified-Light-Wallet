/// Home screen - Main wallet dashboard
library;

import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/privacy_status_chip.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/transaction_row_v2.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/balance_hero.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../ui/organisms/p_sliver_header.dart';
import '../../core/ffi/generated/models.dart'
    show TxInfo, TunnelMode, TunnelMode_Tor, TunnelMode_I2p, TunnelMode_Socks5;
import '../../core/providers/wallet_providers.dart';
import '../settings/providers/transport_providers.dart';
import '../address_book/providers/address_book_provider.dart';

/// Home screen
class HomeScreen extends ConsumerStatefulWidget {
  const HomeScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends ConsumerState<HomeScreen>
    with SingleTickerProviderStateMixin {
  late AnimationController _syncAnimationController;
  bool _hideBalance = false;

  @override
  void initState() {
    super.initState();
    _syncAnimationController = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 2),
    );
  }

  @override
  void dispose() {
    _syncAnimationController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // Watch real providers
    final syncStatusAsync = ref.watch(syncProgressStreamProvider);
    final isSyncRunningAsync = ref.watch(isSyncRunningProvider);
    final balanceAsync = ref.watch(balanceStreamProvider);
    final transactionsAsync = ref.watch(transactionsProvider);
    final tunnelMode = ref.watch(tunnelModeProvider);
    final torStatus = ref.watch(torStatusProvider);
    final transportConfig = ref.watch(transportConfigProvider);
    final endpointConfigAsync = ref.watch(lightdEndpointConfigProvider);
    
    // Get sync status
    final syncStatus = syncStatusAsync.when(
      data: (status) => status,
      loading: () => null,
      error: (_, __) => null,
    );
    
    // Check if sync is running (even when caught up, sync monitors for new blocks)
    final isSyncRunning = isSyncRunningAsync.when(
      data: (running) => running,
      loading: () => false,
      error: (_, __) => false,
    );

    final endpointConfig = endpointConfigAsync.value;
    final i2pEndpoint = transportConfig.i2pEndpoint.trim();
    final i2pEndpointReady =
        !(tunnelMode is TunnelMode_I2p) || i2pEndpoint.isNotEmpty;
    final usesPrivacyTunnel = (tunnelMode is TunnelMode_Tor) ||
        (tunnelMode is TunnelMode_I2p) ||
        (tunnelMode is TunnelMode_Socks5);
    final tunnelReady =
        (!(tunnelMode is TunnelMode_Tor) || torStatus.isReady) &&
        i2pEndpointReady;
    final tunnelError =
        (tunnelMode is TunnelMode_Tor && torStatus.status == 'error') ||
        (tunnelMode is TunnelMode_I2p && !i2pEndpointReady);

    // Determine privacy status: only show offline if endpoint is not set or if there's a real connection error
    // If endpoint is set and test connection works, don't show offline just because sync hasn't started
    // Don't show "Connecting" when sync is complete and monitoring - use sync status to determine
    final hasEndpoint = endpointConfig != null || endpointConfigAsync.hasValue;
    final effectiveHasEndpoint =
        tunnelMode is TunnelMode_I2p ? i2pEndpointReady : hasEndpoint;
    final tunnelBlocked = usesPrivacyTunnel && !tunnelReady;
    final displaySyncStatus = tunnelBlocked ? null : syncStatus;
    final hasStatus = displaySyncStatus != null &&
        (displaySyncStatus!.targetHeight > BigInt.zero ||
            displaySyncStatus!.localHeight > BigInt.zero);
    final privacyStatus = !effectiveHasEndpoint
        ? PrivacyStatus.offline
        : tunnelBlocked
            ? (tunnelError ? PrivacyStatus.offline : PrivacyStatus.connecting)
            : !hasStatus
                ? PrivacyStatus.connecting
                : usesPrivacyTunnel
                    ? PrivacyStatus.private
                    : PrivacyStatus.limited;
    
    final syncProgress = (displaySyncStatus?.percent ?? 0.0) / 100.0;
    final currentHeight = displaySyncStatus?.localHeight ?? BigInt.zero;
    final targetHeight = displaySyncStatus?.targetHeight ?? BigInt.zero;
    final heightLag =
        targetHeight > currentHeight ? targetHeight - currentHeight : BigInt.zero;
    const syncLagThreshold = 10;
    final isNearTip =
        targetHeight > BigInt.zero && heightLag <= BigInt.from(syncLagThreshold);
    final isSyncing =
        !tunnelBlocked && !isNearTip && (displaySyncStatus?.isSyncing ?? false);
    final isComplete = !tunnelBlocked &&
        (isNearTip || (displaySyncStatus?.isComplete ?? false));
    // Show progress bar when actively syncing OR when monitoring (sync running but caught up)
    final showProgress =
        !tunnelBlocked && (isSyncing || (isComplete && isSyncRunning));
    
    // Control animation based on sync state
    // Keep animation going when syncing OR when monitoring (caught up but sync is still active)
    if (isSyncing && !_syncAnimationController.isAnimating) {
      _syncAnimationController.repeat();
    } else if (!isSyncing && _syncAnimationController.isAnimating) {
      _syncAnimationController.stop();
    }
    
    // Get balance
    final balanceData = balanceAsync.when(
      data: (b) => b,
      loading: () => null,
      error: (_, __) => null,
    );
    final totalBalance = balanceData?.total ?? BigInt.zero;
    final spendableBalance = balanceData?.spendable ?? BigInt.zero;
    final pendingBalance = balanceData?.pending ?? BigInt.zero;
    final displayBalance =
        (isSyncing || !isComplete) ? spendableBalance : totalBalance;
    final balanceArrr = displayBalance.toDouble() / 100000000.0;
    final pendingArrr = pendingBalance.toDouble() / 100000000.0;
    String? balanceHelper;
    if (balanceArrr <= 0) {
      balanceHelper = 'Share your address to get paid.';
    } else if (pendingBalance > BigInt.zero) {
      balanceHelper = 'Pending: ${pendingArrr.toStringAsFixed(8)} ARRR';
    }
    
    // Get transactions
    final transactions = transactionsAsync.when(
      data: (txs) => txs,
      loading: () => <TxInfo>[],
      error: (_, __) => <TxInfo>[],
    );
    
    final content = CustomScrollView(
        slivers: [
          SliverPersistentHeader(
            pinned: true,
            delegate: PSliverHeaderDelegate(
              maxExtentHeight: 320,
              minExtentHeight: 320, // Keep header fully visible, don't collapse
              builder: (context, shrinkOffset, overlapsContent) {
                return ClipRect(
                  child: BackdropFilter(
                    filter: ImageFilter.blur(sigmaX: 10, sigmaY: 10),
                    child: Container(
                      decoration: BoxDecoration(
                        color: AppColors.backgroundBase.withValues(alpha: 0.85),
                      ),
                      child: SafeArea(
                        bottom: true,
                        child: Padding(
                          padding: const EdgeInsets.fromLTRB(
                            PSpacing.lg,
                            PSpacing.lg,
                            PSpacing.lg,
                            PSpacing.lg,
                          ),
                          child: Column(
                            mainAxisAlignment: MainAxisAlignment.spaceBetween,
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  const WalletSwitcherButton(),
                                  const Spacer(),
                                  PrivacyStatusChip(
                                    status: privacyStatus,
                                    compact: true,
                                    onTap: () => context.push('/settings/privacy-shield'),
                                  ),
                                ],
                              ),
                              const SizedBox(height: PSpacing.md),
                              BalanceHero(
                                balanceText:
                                    '${balanceArrr.toStringAsFixed(8)} ARRR',
                                helperText: balanceHelper,
                                isHidden: _hideBalance,
                                onToggleVisibility: () {
                                  setState(() {
                                    _hideBalance = !_hideBalance;
                                  });
                                },
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),

          // Sync indicator (always visible to show sync status)
          // Add small top padding to prevent overlap with pinned header
          SliverPadding(
            padding: EdgeInsets.only(
              top: PSpacing.md, // Small spacing to prevent overlap
              left: PSpacing.lg,
              right: PSpacing.lg,
              bottom: PSpacing.md,
            ),
            sliver: SliverToBoxAdapter(
              child: _SyncIndicator(
                progress: syncProgress,
                currentHeight: currentHeight.toInt(),
                targetHeight: targetHeight.toInt(),
                stage: isComplete
                    ? 'Synced'
                    : displaySyncStatus?.stageName ??
                        (displaySyncStatus != null ? 'Syncing' : 'Not synced'),
                eta: isComplete
                    ? null
                    : displaySyncStatus?.etaFormatted ??
                        (isSyncing ? 'Calculating...' : null),
                blocksPerSecond:
                    displaySyncStatus?.blocksPerSecond ?? 0.0,
                animation: _syncAnimationController,
                isSyncing: isSyncing,
                isComplete: isComplete,
              ),
            ),
          ),

          // Quick actions
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.all(PSpacing.lg),
              child: Row(
                children: [
                  Expanded(
                    child: _QuickActionButton(
                      icon: Icons.arrow_upward,
                      label: 'Send',
                      color: AppColors.accentPrimary,
                      onTap: () => context.push('/send'),
                    ),
                  ),
                  const SizedBox(width: PSpacing.md),
                  Expanded(
                    child: _QuickActionButton(
                      icon: Icons.arrow_downward,
                      label: 'Receive',
                      color: AppColors.accentSecondary,
                      onTap: () => context.push('/receive'),
                    ),
                  ),
                ],
              ),
            ),
          ),

          // Recent transactions header
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(
                  PSpacing.lg,
                PSpacing.xl,
                PSpacing.lg,
                PSpacing.md,
              ),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text(
                    'Recent activity',
                    style: PTypography.heading3().copyWith(
                      color: AppColors.textPrimary,
                    ),
                  ),
                  PTextButton(
                    label: 'View all',
                    onPressed: () => context.push('/activity'),
                  ),
                ],
              ),
            ),
          ),

          // Recent transactions list (from real data)
          if (transactions.isEmpty)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.all(PSpacing.xl),
                child: Center(
                  child: Column(
                    children: [
                      Icon(
                        Icons.history,
                        size: 48,
                        color: AppColors.textSecondary.withValues(alpha: 0.5),
                      ),
                      const SizedBox(height: PSpacing.md),
                      Text(
                        isSyncing ? 'Syncing activity...' : 'No activity yet.',
                        style: PTypography.bodyMedium().copyWith(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            )
          else
            SliverList(
              delegate: SliverChildBuilderDelegate(
                (context, index) {
                  if (index >= transactions.length) return null;
                  final tx = transactions[index];
                  return _TransactionItemWithLabel(
                    tx: tx,
                    onTap: () => context.push('/transaction/${tx.txid}'),
                  );
                },
                childCount: transactions.length.clamp(0, 10),
              ),
            ),
        ],
      );

    if (!widget.useScaffold) {
      return content;
    }

    return PScaffold(
      title: 'Wallet Home',
      body: content,
    );
  }
}

/// Sync indicator widget with real data
class _SyncIndicator extends StatelessWidget {
  final double progress;
  final int currentHeight;
  final int targetHeight;
  final String stage;
  final String? eta;
  final double blocksPerSecond;
  final Animation<double> animation;
  final bool isSyncing;
  final bool isComplete;

  const _SyncIndicator({
    required this.progress,
    required this.currentHeight,
    required this.targetHeight,
    required this.stage,
    this.eta,
    required this.blocksPerSecond,
    required this.animation,
    required this.isSyncing,
    required this.isComplete,
  });

  @override
  Widget build(BuildContext context) {
    // Determine icon and color based on state
    final icon = isSyncing
        ? Icons.sync
        : isComplete
            ? Icons.check_circle
            : Icons.sync_disabled;
    final iconColor = isSyncing
        ? AppColors.accentPrimary
        : isComplete
            ? AppColors.success
            : AppColors.textSecondary;

    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                // Animated icon only when syncing
                if (isSyncing)
                  RotationTransition(
                    turns: animation,
                    child: Icon(
                      icon,
                      size: 16,
                      color: iconColor,
                    ),
                  )
                else
                  Icon(
                    icon,
                    size: 16,
                    color: iconColor,
                  ),
                const SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Text(
                    stage,
                    style: PTypography.caption().copyWith(
                      color: AppColors.textPrimary,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                if (isSyncing && blocksPerSecond > 0)
                  Text(
                    '${blocksPerSecond.toStringAsFixed(1)} blk/s',
                    style: PTypography.caption().copyWith(
color: AppColors.textSecondary,
                    ),
                  ),
                // Show percentage whenever we have valid progress data (during sync or when monitoring)
                if (targetHeight.toInt() > 0) ...[
                  const SizedBox(width: PSpacing.sm),
                  Text(
                    '${(progress * 100).toStringAsFixed(1)}%',
                    style: PTypography.caption().copyWith(
                      color: AppColors.accentPrimary,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ],
              ],
            ),
            // Progress bar when actively syncing OR when monitoring (caught up but sync running)
            if (isSyncing || isComplete) ...[
              const SizedBox(height: PSpacing.sm),
              ClipRRect(
                borderRadius: BorderRadius.circular(4),
                child: LinearProgressIndicator(
                  value: progress.clamp(0.0, 1.0),
                  backgroundColor: AppColors.surfaceElevated,
                  valueColor: AlwaysStoppedAnimation<Color>(
                    AppColors.accentPrimary,
                  ),
                  minHeight: 4,
                  semanticsLabel: 'Sync progress',
                  semanticsValue: '${(progress * 100).toInt()}%',
                ),
              ),
            ],
            const SizedBox(height: PSpacing.sm),
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text(
                  (targetHeight.toInt() > 0 && currentHeight.toInt() > 0)
                      ? 'Block ${currentHeight.toString()} / ${targetHeight.toString()}'
                      : (currentHeight.toInt() > 0)
                          ? 'Block ${currentHeight.toString()}'
                          : (targetHeight.toInt() > 0)
                              ? 'Block 0 / ${targetHeight.toString()}'
                              : 'Block 0',
                  style: PTypography.caption().copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
                if (eta != null)
                  Text(
                    eta!,
                    style: PTypography.caption().copyWith(
                      color: AppColors.textSecondary,
                    ),
                  )
                else if (isComplete)
                  Text(
                    // Show "Up to date" when caught up - sync is still monitoring for new blocks
                    stage == 'Monitoring' ? 'Up to date' : 'Synced',
                    style: PTypography.caption().copyWith(
                      color: AppColors.success,
                    ),
                  )
                else if (isSyncing)
                  Text(
                    'Calculating...',
                    style: PTypography.caption().copyWith(
                      color: AppColors.textSecondary,
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

/// Quick action button
class _QuickActionButton extends StatelessWidget {
  final IconData icon;
  final String label;
  final Color color;
  final VoidCallback onTap;

  const _QuickActionButton({
    required this.icon,
    required this.label,
    required this.color,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(16),
        child: Padding(
          padding: const EdgeInsets.symmetric(
            horizontal: PSpacing.lg,
            vertical: PSpacing.xl,
          ),
          child: Column(
            children: [
              Container(
                padding: const EdgeInsets.all(PSpacing.md),
                decoration: BoxDecoration(
                  color: color.withValues(alpha: 0.1),
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  icon,
                  color: color,
                  size: 28,
                  semanticLabel: label,
                ),
              ),
              const SizedBox(height: PSpacing.sm),
              Text(
                label,
                style: PTypography.bodyMedium().copyWith(
                  fontWeight: FontWeight.w600,
                  color: AppColors.textPrimary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Transaction item with address book label lookup
class _TransactionItemWithLabel extends ConsumerWidget {
  final TxInfo tx;
  final VoidCallback? onTap;

  const _TransactionItemWithLabel({
    required this.tx,
    this.onTap,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final walletId = ref.watch(activeWalletProvider);
    
    // Look up label from address book if this is a sent transaction
    // Note: TxInfo doesn't have toAddress field - would need transaction details
    String? addressLabel;
    // if (walletId != null && tx.amount < 0 && tx.toAddress != null) {
    //   final addressBookState = ref.watch(addressBookProvider(walletId));
    //   addressLabel = addressBookState.entries
    //       .where((e) => e.address == tx.toAddress)
    //       .map((e) => e.label)
    //       .firstOrNull;
    // }
    
    // Convert PlatformInt64 to int for calculations
    int amountValue;
    if (tx.amount is int) {
      amountValue = tx.amount as int;
    } else {
      amountValue = (tx.amount as dynamic).toInt() as int;
    }
    
    final isReceived = amountValue >= 0;
    final amount = amountValue.abs() / 100000000.0;
    
    // Convert PlatformInt64 timestamp to DateTime
    int timestampValue;
    if (tx.timestamp is int) {
      timestampValue = tx.timestamp as int;
    } else {
      timestampValue = (tx.timestamp as dynamic).toInt() as int;
    }
    final timestamp = DateTime.fromMillisecondsSinceEpoch(timestampValue * 1000);

    return TransactionRowV2(
      isReceived: isReceived,
      isConfirmed: tx.confirmed,
      amountText:
          '${isReceived ? '+' : '-'}${amount.toStringAsFixed(4)} ARRR',
      timestamp: timestamp,
      memo: tx.memo,
      addressLabel: addressLabel,
      onTap: onTap,
    );
  }
}
