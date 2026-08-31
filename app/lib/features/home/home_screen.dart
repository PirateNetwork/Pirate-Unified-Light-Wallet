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
import '../../core/platform/platform_utils.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/transaction_row_v2.dart';
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
import 'widgets/home_header_controls.dart';
import 'widgets/home_sync_indicator.dart';

/// Home screen
class HomeScreen extends ConsumerStatefulWidget {
  const HomeScreen({super.key, this.useScaffold = true});

  static const Key headerKey = Key('home-dashboard-header');
  static const Key headerSurfaceKey = Key('home-dashboard-header-surface');
  static const Key recentActivityTitleKey = Key('recent-activity-title');

  final bool useScaffold;

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends ConsumerState<HomeScreen> {
  bool _hideBalance = false;

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    final screenSize = mediaQuery.size;
    final screenWidth = mediaQuery.size.width;
    final compactLandscape =
        !isDesktopPlatform && PSpacing.isCompactLandscape(screenSize);
    final compactDesktopViewport =
        isDesktopPlatform && PSpacing.isCompactDesktopViewport(screenSize);
    final balance = ref.watch(balanceStreamProvider).asData?.value;
    final hasBalanceHelper =
        balance == null ||
        balance.total <= BigInt.zero ||
        balance.pending > BigInt.zero;
    final textScale = MediaQuery.textScalerOf(context).scale(1.0);
    final gutter = PSpacing.responsiveGutter(screenWidth);
    final availableHeaderWidth = math.max(0.0, screenWidth - (gutter * 2));
    final stackedHeaderControls = HomeHeaderControls.shouldStack(
      availableHeaderWidth,
    );
    final headerVerticalPadding = compactLandscape || compactDesktopViewport
        ? PSpacing.xs
        : PSpacing.isDesktop(screenWidth)
        ? PSpacing.md
        : PSpacing.sm;
    final standardHeaderExtent = compactLandscape
        ? 224.0
        : compactDesktopViewport
        ? hasBalanceHelper
              ? 252.0
              : 228.0
        : PSpacing.isMobile(screenWidth)
        ? 280.0
        : PSpacing.isTablet(screenWidth)
        ? 300.0
        : hasBalanceHelper
        ? 320.0
        : 284.0;
    final baseHeaderExtent =
        standardHeaderExtent +
        (stackedHeaderControls ? PSpacing.xl + PSpacing.sm : 0.0);
    final extraHeaderHeight = textScale > 1.0 ? (textScale - 1.0) * 32.0 : 0.0;
    final headerExtent =
        baseHeaderExtent + mediaQuery.padding.top + extraHeaderHeight;
    final enableBackdropBlur =
        !mediaQuery.disableAnimations && !PSpacing.isHandset(screenSize);

    final content = CustomScrollView(
      slivers: [
        SliverPersistentHeader(
          key: HomeScreen.headerKey,
          pinned: !compactLandscape,
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
                showConnectionStatus: widget.useScaffold || !isDesktopPlatform,
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
                    key: HomeScreen.recentActivityTitleKey,
                    'Recent activity'.tr,
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
    required this.showConnectionStatus,
  });

  final EdgeInsets padding;
  final bool enableBackdropBlur;
  final bool hideBalance;
  final VoidCallback onToggleVisibility;
  final bool showConnectionStatus;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final balanceAsync = ref.watch(balanceStreamProvider);
    final currency = ref.watch(currencyPreferenceProvider);
    final priceQuote = ref.watch(arrrPriceQuoteProvider).asData?.value;
    final primaryFiat = ref.watch(balancePrimaryFiatProvider);

    final balanceData = balanceAsync.when(
      data: (b) => b,
      loading: () => null,
      error: (_, _) => null,
    );
    final totalBalance = balanceData?.total ?? BigInt.zero;
    // Use backend pending value so "pending change" is shown consistently (matches
    // spendability rules used by send flow).
    final pendingBalance = balanceData?.pending ?? BigInt.zero;
    // Keep balance display stable during incremental sync at tip.
    // Spendability is enforced in send flow, not by zeroing the home balance.
    final displayBalance = totalBalance;
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
      balanceHelper = 'Share your address to get paid.'.tr;
    } else if (pendingBalance > BigInt.zero) {
      balanceHelper = 'Pending: {amount} ARRR'.trArgs({
        'amount': pendingArrr.toStringAsFixed(8),
      });
    }

    final headerSurface = DecoratedBox(
      key: HomeScreen.headerSurfaceKey,
      decoration: BoxDecoration(color: AppColors.backgroundBase),
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: padding,
          child: Column(
            mainAxisAlignment: MainAxisAlignment.start,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              HomeHeaderControls(
                onConnectionTap: () => context.push('/settings/privacy-shield'),
                showConnectionStatus: showConnectionStatus,
              ),
              const SizedBox(height: PSpacing.sm),
              Expanded(
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    final isMobile = PSpacing.isHandset(
                      MediaQuery.sizeOf(context),
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
        ? 'Synced'.tr
        : displaySyncStatus?.stageName ??
              (displaySyncStatus != null ? 'Syncing'.tr : 'Not synced'.tr);
    final eta = isComplete
        ? null
        : displaySyncStatus?.etaFormatted ??
              (isSyncing ? 'Calculating...'.tr : null);

    return RepaintBoundary(
      child: HomeSyncIndicator(
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
                  isSyncing ? 'Syncing activity...'.tr : 'No activity yet.'.tr,
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
          return Padding(
            padding: EdgeInsets.only(
              bottom: index == itemCount - 1 ? 0 : PSpacing.sm,
            ),
            child: _TransactionItemWithLabel(
              key: ValueKey(tx.txid),
              tx: tx,
              isConfirmed: _isConfirmedTx(tx, currentHeight),
              onTap: () => context.push(
                '/transaction/${tx.txid}?amount=${tx.amount}',
                extra: tx,
              ),
            ),
          );
        }, childCount: itemCount),
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
      onTap: onTap,
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
    );
  }
}

/// Transaction item with address book label lookup
class _TransactionItemWithLabel extends ConsumerWidget {
  final TxInfo tx;
  final bool isConfirmed;
  final VoidCallback? onTap;

  const _TransactionItemWithLabel({
    super.key,
    required this.tx,
    required this.isConfirmed,
    this.onTap,
  });

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
      isConfirmed: isConfirmed,
      isExpired: tx.expired,
      amountText: '${isReceived ? '+' : '-'}${amount.toStringAsFixed(4)} ARRR',
      timestamp: timestamp,
      memo: tx.memo,
      addressLabel: addressLabel,
      onTap: onTap,
    );
  }
}
