/// Home screen - Main wallet dashboard
library;

import 'dart:ui';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/transaction_row_v2.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/balance_hero.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../ui/organisms/p_sliver_header.dart';
import '../../core/ffi/generated/models.dart'
    show
        SyncStage,
        SyncStatus,
        TunnelMode_I2p,
        TunnelMode_Socks5,
        TunnelMode_Tor,
        TxInfo;
import '../../core/providers/wallet_providers.dart';
import '../../core/providers/price_providers.dart';
import '../settings/providers/transport_providers.dart';
import '../settings/providers/preferences_providers.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// Home screen
class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  bool _hideBalance = false;

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    final screenWidth = mediaQuery.size.width;
    final textScale = MediaQuery.textScalerOf(context).scale(1.0);
    final gutter = PSpacing.responsiveGutter(screenWidth);
    final headerVerticalPadding = PSpacing.isDesktop(screenWidth)
        ? PSpacing.md
        : PSpacing.sm;
    final baseHeaderExtent = PSpacing.isMobile(screenWidth)
        ? 280.0
        : PSpacing.isTablet(screenWidth)
        ? 300.0
        : 320.0;
    final extraHeaderHeight = textScale > 1.0 ? (textScale - 1.0) * 32.0 : 0.0;
    final headerExtent =
        baseHeaderExtent + mediaQuery.padding.top + extraHeaderHeight;
    final enableBackdropBlur =
        !mediaQuery.disableAnimations && !PSpacing.isMobile(screenWidth);

    final content = CustomScrollView(
      slivers: [
        SliverPersistentHeader(
          pinned: true,
          delegate: PSliverHeaderDelegate(
            maxExtentHeight: headerExtent,
            minExtentHeight: headerExtent,
            builder: (context, shrinkOffset, {required overlapsContent}) {
              return _HomeHeader(
                padding: EdgeInsets.fromLTRB(
                  gutter,
                  headerVerticalPadding,
                  gutter,
                  headerVerticalPadding,
                ),
                enableBackdropBlur: enableBackdropBlur,
                hideBalance: _hideBalance,
                onToggleVisibility: () {
                  setState(() {
                    _hideBalance = !_hideBalance;
                  });
                },
              );
            },
          ),
        ),
        SliverPadding(
          padding: EdgeInsets.fromLTRB(
            gutter,
            PSpacing.md,
            gutter,
            PSpacing.md,
          ),
          sliver: const SliverToBoxAdapter(child: _HomeSyncIndicator()),
        ),
        SliverToBoxAdapter(
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              PSpacing.md,
              gutter,
              PSpacing.md,
            ),
            child: Row(
              children: [
                Expanded(
                  child: _QuickActionButton(
                    icon: Icons.arrow_upward,
                    label: 'Send'.tr,
                    color: AppColors.accentPrimary,
                    onTap: () => context.push('/send'),
                  ),
                ),
                const SizedBox(width: PSpacing.md),
                Expanded(
                  child: _QuickActionButton(
                    icon: Icons.arrow_downward,
                    label: 'Receive'.tr,
                    color: AppColors.accentSecondary,
                    onTap: () => context.push('/receive'),
                  ),
                ),
              ],
            ),
          ),
        ),
        SliverToBoxAdapter(
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              PSpacing.xl,
              gutter,
              PSpacing.md,
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    'Recent activity'.tr,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: PTypography.heading3().copyWith(
                      color: AppColors.textPrimary,
                    ),
                  ),
                ),
                PTextButton(
                  label: 'View all'.tr,
                  onPressed: () => context.push('/activity'),
                ),
              ],
            ),
          ),
        ),
        _HomeTransactionsSection(gutter: gutter),
      ],
    );

    if (!widget.useScaffold) {
      return content;
    }

    return PScaffold(title: 'Wallet Home'.tr, body: content);
  }
}

SyncStatus _buildDecoySyncStatus(int height) {
  final safeHeight = height > 0 ? height : 1;
  final blockHeight = BigInt.from(safeHeight);
  return SyncStatus(
    localHeight: blockHeight,
    targetHeight: blockHeight,
    percent: 100.0,
    eta: null,
    stage: SyncStage.verify,
    lastCheckpoint: null,
    blocksPerSecond: 0.0,
    notesDecrypted: BigInt.zero,
    lastBatchMs: BigInt.zero,
  );
}

class _HomeHeader extends ConsumerWidget {
  const _HomeHeader({
    required this.padding,
    required this.enableBackdropBlur,
    required this.hideBalance,
    required this.onToggleVisibility,
  });

  final EdgeInsets padding;
  final bool enableBackdropBlur;
  final bool hideBalance;
  final VoidCallback onToggleVisibility;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final syncStatusAsync = ref.watch(syncProgressStreamProvider);
    final balanceAsync = ref.watch(balanceStreamProvider);
    final tunnelMode = ref.watch(tunnelModeProvider);
    final torStatus = ref.watch(torStatusProvider);
    final transportConfig = ref.watch(transportConfigProvider);
    final isDecoy = ref.watch(decoyModeProvider);
    final currency = ref.watch(currencyPreferenceProvider);
    final priceQuote = ref.watch(arrrPriceQuoteProvider).asData?.value;
    final primaryFiat = ref.watch(balancePrimaryFiatProvider);
    final decoyHeight = ref
        .watch(decoySyncHeightProvider)
        .maybeWhen(data: (height) => height, orElse: () => 0);

    final syncStatus = syncStatusAsync.when(
      data: (status) => status,
      loading: () => null,
      error: (_, _) => null,
      skipLoadingOnRefresh: false,
    );

    final decoySyncStatus = isDecoy ? _buildDecoySyncStatus(decoyHeight) : null;
    final i2pEndpoint = transportConfig.i2pEndpoint.trim();
    final i2pEndpointReady =
        tunnelMode is! TunnelMode_I2p || i2pEndpoint.isNotEmpty;
    final usesPrivacyTunnel =
        (tunnelMode is TunnelMode_Tor) ||
        (tunnelMode is TunnelMode_I2p) ||
        (tunnelMode is TunnelMode_Socks5);
    final tunnelReady =
        (tunnelMode is! TunnelMode_Tor || torStatus.isReady) &&
        i2pEndpointReady;
    final tunnelBlocked = !isDecoy && usesPrivacyTunnel && !tunnelReady;
    final displaySyncStatus = isDecoy
        ? decoySyncStatus
        : (tunnelBlocked ? null : syncStatus);

    final isSyncing = !tunnelBlocked && (displaySyncStatus?.isSyncing ?? false);
    final isComplete =
        !tunnelBlocked && (displaySyncStatus?.isComplete ?? false);

    final balanceData = balanceAsync.when(
      data: (b) => b,
      loading: () => null,
      error: (_, _) => null,
    );
    final totalBalance = balanceData?.total ?? BigInt.zero;
    final spendableBalance = balanceData?.spendable ?? BigInt.zero;
    final pendingBalance = balanceData?.pending ?? BigInt.zero;
    final displayBalance = (isSyncing || !isComplete)
        ? spendableBalance
        : totalBalance;
    final balanceArrr = displayBalance.toDouble() / 100000000.0;
    final pendingArrr = pendingBalance.toDouble() / 100000000.0;
    final arrrText = ArrrPriceFormatter.formatArrr(balanceArrr);
    final fiatAmount = priceQuote == null
        ? null
        : balanceArrr * priceQuote.pricePerArrr;
    final fiatText = fiatAmount == null
        ? null
        : ArrrPriceFormatter.formatCurrency(currency, fiatAmount);
    final showFiatPrimary = primaryFiat && fiatText != null;
    final primaryText = showFiatPrimary ? fiatText : arrrText;
    final secondaryText = fiatText == null
        ? null
        : (showFiatPrimary ? arrrText : fiatText);
    String? balanceHelper;
    if (balanceArrr <= 0) {
      balanceHelper = 'Share your address to get paid.';
    } else if (pendingBalance > BigInt.zero) {
      balanceHelper = 'Pending: ${pendingArrr.toStringAsFixed(8)} ARRR';
    }

    final headerSurface = DecoratedBox(
      decoration: BoxDecoration(
        color: AppColors.backgroundBase.withValues(alpha: 0.85),
      ),
      child: SafeArea(
        bottom: true,
        child: Padding(
          padding: padding,
          child: Column(
            mainAxisAlignment: MainAxisAlignment.start,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const WalletSwitcherButton(),
                  const Spacer(),
                  ConnectionStatusIndicator(
                    full: true,
                    compact: true,
                    onTap: () => context.push('/settings/privacy-shield'),
                  ),
                ],
              ),
              const SizedBox(height: PSpacing.sm),
              Expanded(
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    final isMobile = PSpacing.isMobile(
                      MediaQuery.sizeOf(context).width,
                    );
                    final compact = constraints.maxHeight < 240;
                    return Align(
                      alignment: isMobile
                          ? Alignment.topCenter
                          : Alignment.topLeft,
                      child: SizedBox(
                        width: constraints.maxWidth,
                        child: BalanceHero(
                          compact: compact,
                          balanceText: primaryText,
                          secondaryText: secondaryText,
                          helperText: balanceHelper,
                          isHidden: hideBalance,
                          onToggleVisibility: onToggleVisibility,
                          onSwapDisplay: secondaryText == null
                              ? null
                              : () {
                                  ref
                                      .read(balancePrimaryFiatProvider.notifier)
                                      .setPrimaryFiat(enabled: !primaryFiat);
                                },
                        ),
                      ),
                    );
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );

    final headerContent = enableBackdropBlur
        ? BackdropFilter(
            filter: ImageFilter.blur(sigmaX: 10, sigmaY: 10),
            child: headerSurface,
          )
        : headerSurface;

    return RepaintBoundary(child: ClipRect(child: headerContent));
  }
}

class _HomeSyncIndicator extends ConsumerWidget {
  const _HomeSyncIndicator();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final syncStatusAsync = ref.watch(syncProgressStreamProvider);
    final tunnelMode = ref.watch(tunnelModeProvider);
    final torStatus = ref.watch(torStatusProvider);
    final transportConfig = ref.watch(transportConfigProvider);
    final isDecoy = ref.watch(decoyModeProvider);
    final decoyHeight = ref
        .watch(decoySyncHeightProvider)
        .maybeWhen(data: (height) => height, orElse: () => 0);
    final reduceMotion = MediaQuery.of(context).disableAnimations;

    final syncStatus = syncStatusAsync.when(
      data: (status) => status,
      loading: () => null,
      error: (_, _) => null,
      skipLoadingOnRefresh: false,
    );

    final decoySyncStatus = isDecoy ? _buildDecoySyncStatus(decoyHeight) : null;
    final i2pEndpoint = transportConfig.i2pEndpoint.trim();
    final i2pEndpointReady =
        tunnelMode is! TunnelMode_I2p || i2pEndpoint.isNotEmpty;
    final usesPrivacyTunnel =
        (tunnelMode is TunnelMode_Tor) ||
        (tunnelMode is TunnelMode_I2p) ||
        (tunnelMode is TunnelMode_Socks5);
    final tunnelReady =
        (tunnelMode is! TunnelMode_Tor || torStatus.isReady) &&
        i2pEndpointReady;
    final tunnelBlocked = !isDecoy && usesPrivacyTunnel && !tunnelReady;

    final displaySyncStatus = isDecoy
        ? decoySyncStatus
        : (tunnelBlocked ? null : syncStatus);
    final currentHeight = displaySyncStatus?.localHeight ?? BigInt.zero;
    final targetHeight = displaySyncStatus?.targetHeight ?? BigInt.zero;
    final isSyncing = !tunnelBlocked && (displaySyncStatus?.isSyncing ?? false);
    final isComplete =
        !tunnelBlocked && (displaySyncStatus?.isComplete ?? false);

    final rawPercent = displaySyncStatus?.percent ?? 0.0;
    final displayPercent = (targetHeight > BigInt.zero)
        ? (isComplete
              ? rawPercent.clamp(0.0, 100.0)
              : rawPercent.clamp(0.0, 99.9))
        : 0.0;
    final syncProgress = displayPercent / 100.0;
    final stage = isComplete
        ? 'Synced'
        : displaySyncStatus?.stageName ??
              (displaySyncStatus != null ? 'Syncing' : 'Not synced');
    final eta = isComplete
        ? null
        : displaySyncStatus?.etaFormatted ??
              (isSyncing ? 'Calculating...' : null);

    return RepaintBoundary(
      child: _SyncIndicator(
        progress: syncProgress,
        currentHeight: currentHeight.toInt(),
        targetHeight: targetHeight.toInt(),
        stage: stage,
        eta: eta,
        blocksPerSecond: displaySyncStatus?.blocksPerSecond ?? 0.0,
        isSyncing: isSyncing,
        isComplete: isComplete,
        reduceMotion: reduceMotion,
      ),
    );
  }
}

class _HomeTransactionsSection extends ConsumerWidget {
  const _HomeTransactionsSection({required this.gutter});

  final double gutter;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final transactionsAsync = ref.watch(transactionsProvider);
    final isSyncing = ref.watch(
      syncProgressStreamProvider.select(
        (value) => value.maybeWhen(
          data: (status) => status?.isSyncing ?? false,
          orElse: () => false,
        ),
      ),
    );

    final transactions = transactionsAsync.when(
      data: (txs) => txs,
      loading: () => <TxInfo>[],
      error: (_, _) => <TxInfo>[],
    );

    if (transactions.isEmpty) {
      return SliverToBoxAdapter(
        child: Padding(
          padding: EdgeInsets.fromLTRB(
            gutter,
            PSpacing.xl,
            gutter,
            PSpacing.xl,
          ),
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
      );
    }

    final itemCount = transactions.length > 10 ? 10 : transactions.length;
    return SliverPadding(
      padding: EdgeInsets.fromLTRB(gutter, 0, gutter, PSpacing.lg),
      sliver: SliverList(
        delegate: SliverChildBuilderDelegate((context, index) {
          if (index >= itemCount) return null;
          final tx = transactions[index];
          return _TransactionItemWithLabel(
            key: ValueKey(tx.txid),
            tx: tx,
            onTap: () =>
                context.push('/transaction/${tx.txid}?amount=${tx.amount}'),
          );
        }, childCount: itemCount),
      ),
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
  final bool isSyncing;
  final bool isComplete;
  final bool reduceMotion;

  const _SyncIndicator({
    required this.progress,
    required this.currentHeight,
    required this.targetHeight,
    required this.stage,
    this.eta,
    required this.blocksPerSecond,
    required this.isSyncing,
    required this.isComplete,
    required this.reduceMotion,
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

    return Semantics(
      container: true,
      liveRegion: true,
      label: 'Wallet sync status'.tr,
      value: '$stage, ${(progress * 100).toStringAsFixed(1)} percent complete',
      child: PCard(
        child: Padding(
          padding: const EdgeInsets.all(PSpacing.md),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  // Animated icon only when syncing
                  if (isSyncing)
                    RepeatingAnimationBuilder<double>(
                      animatable: Tween<double>(begin: 0, end: 1),
                      duration: const Duration(seconds: 2),
                      paused: reduceMotion || !isSyncing,
                      builder: (context, value, child) {
                        return Transform.rotate(
                          angle: value * 2 * math.pi,
                          child: child,
                        );
                      },
                      child: Icon(icon, size: 16, color: iconColor),
                    )
                  else
                    Icon(icon, size: 16, color: iconColor),
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
                  if (targetHeight > 0) ...[
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
                  ),
                ),
              ],
              const SizedBox(height: PSpacing.sm),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      (targetHeight > 0 && currentHeight > 0)
                          ? 'Block $currentHeight / $targetHeight'
                          : (currentHeight > 0)
                          ? 'Block $currentHeight'
                          : (targetHeight > 0)
                          ? 'Block 0 / $targetHeight'
                          : 'Block 0',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: PTypography.caption().copyWith(
                        color: AppColors.textSecondary,
                      ),
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
                      'Calculating...'.tr,
                      style: PTypography.caption().copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                ],
              ),
            ],
          ),
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
                child: Icon(icon, color: color, size: 28, semanticLabel: label),
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

  const _TransactionItemWithLabel({super.key, required this.tx, this.onTap});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
    final amountValue = tx.amount;
    final isReceived = amountValue >= 0;
    final amount = amountValue.abs() / 100000000.0;

    // Convert PlatformInt64 timestamp to DateTime
    final timestampValue = tx.timestamp;
    final timestamp = DateTime.fromMillisecondsSinceEpoch(
      timestampValue * 1000,
    );

    return TransactionRowV2(
      isReceived: isReceived,
      isConfirmed: tx.confirmed,
      amountText: '${isReceived ? '+' : '-'}${amount.toStringAsFixed(4)} ARRR',
      timestamp: timestamp,
      memo: tx.memo,
      addressLabel: addressLabel,
      onTap: onTap,
    );
  }
}
