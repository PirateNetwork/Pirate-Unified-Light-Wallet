import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';
import '../../core/platform/platform_utils.dart';
import '../../core/swaps/swap_availability.dart';

/// Wallets hub for desktop and deep links.
class PayScreen extends StatelessWidget {
  const PayScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  Widget build(BuildContext context) {
    final size = MediaQuery.of(context).size;
    final isMobile = PSpacing.isHandset(size);
    final appBarActions = [
      ConnectionStatusIndicator(
        full: !isMobile,
        onTap: () => context.push('/settings/privacy-shield'),
      ),
      if (!isMobile) const WalletSwitcherButton(compact: true),
    ];
    final content = _PayContent(
      onSend: () => context.push('/send'),
      onReceive: () => context.push('/receive'),
      onVerify: () => context.push('/payment-disclosure'),
      onSwap: () => context.push('/swap'),
    );
    final desktopPlatform = isDesktopPlatform;

    if (!useScaffold) {
      if (desktopPlatform) {
        return content;
      }
      return PScaffold(
        title: 'Wallets'.tr,
        useSafeArea: false,
        appBar: PAppBar(
          title: 'Wallets'.tr,
          subtitle: 'Send, receive, swap, or verify in a few steps.'.tr,
          actions: appBarActions,
        ),
        body: content,
      );
    }

    return PScaffold(
      title: 'Wallets'.tr,
      appBar: desktopPlatform
          ? null
          : PAppBar(
              title: 'Wallets'.tr,
              subtitle: 'Send, receive, swap, or verify in a few steps.'.tr,
              actions: appBarActions,
            ),
      body: content,
    );
  }
}

/// Mobile Wallets sheet with primary payment tools.
class PaySheet extends StatelessWidget {
  const PaySheet({
    required this.onSend,
    required this.onReceive,
    required this.onVerify,
    required this.onSwap,
    super.key,
  });

  final VoidCallback onSend;
  final VoidCallback onReceive;
  final VoidCallback onVerify;
  final VoidCallback onSwap;

  @override
  Widget build(BuildContext context) {
    final screenSize = MediaQuery.sizeOf(context);
    final compactLandscape = PSpacing.isCompactLandscape(screenSize);
    final maxSheetHeight = screenSize.height * (compactLandscape ? 0.92 : 0.75);
    return Container(
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: const BorderRadius.vertical(
          top: Radius.circular(PSpacing.radiusXL),
        ),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      padding: EdgeInsets.fromLTRB(
        compactLandscape ? PSpacing.md : PSpacing.lg,
        PSpacing.sm,
        compactLandscape ? PSpacing.md : PSpacing.lg,
        compactLandscape ? PSpacing.md : PSpacing.xl,
      ),
      child: SafeArea(
        top: false,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final columns = constraints.maxWidth < 360 ? 1 : 2;
            const spacing = PSpacing.md;
            final tileWidth =
                (constraints.maxWidth - spacing * (columns - 1)) / columns;
            final tileHeight = compactLandscape
                ? (tileWidth * 0.48).clamp(112.0, 140.0)
                : (tileWidth * 0.78).clamp(130.0, 170.0);
            final tiles = [
              _PayActionTile(
                title: 'Send'.tr,
                subtitle: 'Send ARRR'.tr,
                icon: Icons.north_east,
                gradient: LinearGradient(
                  colors: [AppColors.gradientAStart, AppColors.gradientAEnd],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: onSend,
                compact: true,
              ),
              _PayActionTile(
                title: 'Receive'.tr,
                subtitle: 'Receive ARRR'.tr,
                icon: Icons.south_west,
                gradient: LinearGradient(
                  colors: [AppColors.gradientBStart, AppColors.gradientBEnd],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: onReceive,
                compact: true,
              ),
              _PayActionTile(
                title: 'Verify'.tr,
                subtitle: 'Verify a single payment'.tr,
                icon: Icons.verified_user_outlined,
                gradient: LinearGradient(
                  colors: [AppColors.gradientCStart, AppColors.gradientCEnd],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: onVerify,
                compact: true,
              ),
              _PayActionTile(
                title: 'Swap'.tr,
                subtitle: 'Swap ARRR'.tr,
                icon: Icons.swap_horiz,
                gradient: LinearGradient(
                  colors: [AppColors.highlight, AppColors.warning],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: kAtomicSwapsEnabled ? onSwap : null,
                compact: true,
              ),
            ];

            return ConstrainedBox(
              constraints: BoxConstraints(maxHeight: maxSheetHeight),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Center(
                    child: Container(
                      width: 48,
                      height: 4,
                      margin: const EdgeInsets.only(bottom: PSpacing.md),
                      decoration: BoxDecoration(
                        color: AppColors.borderStrong,
                        borderRadius: BorderRadius.circular(
                          PSpacing.radiusFull,
                        ),
                      ),
                    ),
                  ),
                  Text(
                    'Wallets'.tr,
                    style: PTypography.heading4(color: AppColors.textPrimary),
                  ),
                  const SizedBox(height: PSpacing.xs),
                  Text(
                    'Send, receive, swap, or verify in a few steps.'.tr,
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  SizedBox(
                    height: compactLandscape ? PSpacing.sm : PSpacing.lg,
                  ),
                  Flexible(
                    child: SingleChildScrollView(
                      child: Wrap(
                        spacing: spacing,
                        runSpacing: spacing,
                        children: tiles
                            .map(
                              (tile) => SizedBox(
                                width: tileWidth,
                                height: tileHeight,
                                child: tile,
                              ),
                            )
                            .toList(),
                      ),
                    ),
                  ),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}

class _PayContent extends StatelessWidget {
  const _PayContent({
    required this.onSend,
    required this.onReceive,
    required this.onVerify,
    required this.onSwap,
  });

  final VoidCallback onSend;
  final VoidCallback onReceive;
  final VoidCallback onVerify;
  final VoidCallback onSwap;

  @override
  Widget build(BuildContext context) {
    final desktopPlatform = isDesktopPlatform;
    final compactDesktopViewport =
        desktopPlatform &&
        PSpacing.isCompactDesktopViewport(MediaQuery.sizeOf(context));
    final compactLandscape =
        !desktopPlatform &&
        PSpacing.isCompactLandscape(MediaQuery.sizeOf(context));
    final tiles = [
      _PayActionTile(
        title: 'Send'.tr,
        subtitle: 'Send ARRR'.tr,
        icon: Icons.north_east,
        gradient: LinearGradient(
          colors: [AppColors.gradientAStart, AppColors.gradientAEnd],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: onSend,
      ),
      _PayActionTile(
        title: 'Receive'.tr,
        subtitle: 'Receive ARRR'.tr,
        icon: Icons.south_west,
        gradient: LinearGradient(
          colors: [AppColors.gradientBStart, AppColors.gradientBEnd],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: onReceive,
      ),
      _PayActionTile(
        title: 'Verify'.tr,
        subtitle: 'Verify a single payment'.tr,
        icon: Icons.verified_user_outlined,
        gradient: LinearGradient(
          colors: [AppColors.gradientCStart, AppColors.gradientCEnd],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: onVerify,
      ),
      _PayActionTile(
        title: 'Swap'.tr,
        subtitle: 'Swap ARRR'.tr,
        icon: Icons.swap_horiz,
        gradient: LinearGradient(
          colors: [AppColors.highlight, AppColors.warning],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: kAtomicSwapsEnabled ? onSwap : null,
      ),
    ];

    return Padding(
      padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final spacing = constraints.maxWidth >= 900
              ? PSpacing.lg
              : PSpacing.md;

          if (desktopPlatform) {
            const crossAxisCount = 2;
            final tileWidth =
                (constraints.maxWidth - spacing * (crossAxisCount - 1)) /
                crossAxisCount;
            final preferredTileHeight = (tileWidth * 0.54).clamp(150.0, 260.0);
            final availableTileHeight = constraints.hasBoundedHeight
                ? ((constraints.maxHeight - spacing - PSpacing.xl) / 2).clamp(
                    150.0,
                    260.0,
                  )
                : preferredTileHeight;
            var tileHeight = math.min(preferredTileHeight, availableTileHeight);
            if (compactDesktopViewport) {
              tileHeight = math.min(tileHeight, 210);
            }
            final aspectRatio = tileWidth / tileHeight;
            final compactDesktop =
                compactDesktopViewport || tileWidth < 280 || tileHeight < 210;

            return SingleChildScrollView(
              padding: const EdgeInsets.only(bottom: PSpacing.xl),
              child: GridView.builder(
                padding: EdgeInsets.zero,
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                itemCount: tiles.length,
                gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                  crossAxisCount: crossAxisCount,
                  crossAxisSpacing: spacing,
                  mainAxisSpacing: spacing,
                  childAspectRatio: aspectRatio,
                ),
                itemBuilder: (context, index) {
                  return _PayActionTile(
                    title: tiles[index].title,
                    subtitle: tiles[index].subtitle,
                    icon: tiles[index].icon,
                    gradient: tiles[index].gradient,
                    onTap: tiles[index].onTap,
                    compact: compactDesktop,
                    isDesktop: !compactDesktop,
                  );
                },
              ),
            );
          }

          final crossAxisCount = constraints.maxWidth >= 560 ? 2 : 1;
          final tileWidth =
              (constraints.maxWidth - (spacing * (crossAxisCount - 1))) /
              crossAxisCount;
          final tileHeight = compactLandscape
              ? (tileWidth * 0.48).clamp(132.0, 176.0)
              : (tileWidth * 0.86).clamp(168.0, 360.0);
          final aspectRatio = tileWidth / tileHeight;

          return SingleChildScrollView(
            padding: const EdgeInsets.only(bottom: PSpacing.xl),
            child: GridView.builder(
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              itemCount: tiles.length,
              gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                crossAxisCount: crossAxisCount,
                crossAxisSpacing: spacing,
                mainAxisSpacing: spacing,
                childAspectRatio: aspectRatio,
              ),
              itemBuilder: (context, index) {
                return _PayActionTile(
                  title: tiles[index].title,
                  subtitle: tiles[index].subtitle,
                  icon: tiles[index].icon,
                  gradient: tiles[index].gradient,
                  onTap: tiles[index].onTap,
                  compact: compactLandscape,
                );
              },
            ),
          );
        },
      ),
    );
  }
}

class _PayActionTile extends StatefulWidget {
  const _PayActionTile({
    required this.title,
    required this.subtitle,
    required this.icon,
    required this.gradient,
    required this.onTap,
    this.compact = false,
    this.isDesktop = false,
  });

  final String title;
  final String subtitle;
  final IconData icon;
  final Gradient gradient;
  final VoidCallback? onTap;
  final bool compact;
  final bool isDesktop;

  @override
  State<_PayActionTile> createState() => _PayActionTileState();
}

class _PayActionTileState extends State<_PayActionTile> {
  bool _isFocused = false;

  @override
  Widget build(BuildContext context) {
    final enabled = widget.onTap != null;
    final padding = widget.isDesktop
        ? PSpacing.xl
        : widget.compact
        ? PSpacing.sm
        : PSpacing.lg;
    final iconSize = widget.isDesktop
        ? 32.0
        : widget.compact
        ? 24.0
        : 28.0;
    final iconContainerSize = widget.isDesktop
        ? 56.0
        : (widget.compact ? 40.0 : 48.0);
    final titleStyle = widget.isDesktop
        ? PTypography.heading5(
            color: enabled ? AppColors.textOnAccent : AppColors.textDisabled,
          )
        : widget.compact
        ? PTypography.titleMedium(
            color: enabled ? AppColors.textOnAccent : AppColors.textDisabled,
          )
        : PTypography.heading6(
            color: enabled ? AppColors.textOnAccent : AppColors.textDisabled,
          );
    final subtitleColor = enabled
        ? AppColors.textOnAccent
        : AppColors.textDisabled;
    final subtitleStyle = widget.isDesktop
        ? PTypography.bodyMedium(
            color: subtitleColor.withValues(alpha: enabled ? 0.9 : 1),
          )
        : widget.compact
        ? PTypography.bodySmall(
            color: subtitleColor.withValues(alpha: enabled ? 0.8 : 1),
          )
        : PTypography.bodyMedium(
            color: subtitleColor.withValues(alpha: enabled ? 0.85 : 1),
          );

    return Semantics(
      button: true,
      enabled: enabled,
      child: MouseRegion(
        cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: widget.onTap,
            onFocusChange: (focused) {
              if (_isFocused != focused) {
                setState(() => _isFocused = focused);
              }
            },
            borderRadius: BorderRadius.circular(PSpacing.radiusLG),
            child: Ink(
              decoration: BoxDecoration(
                gradient: enabled ? widget.gradient : null,
                color: enabled ? null : AppColors.backgroundSurface,
                borderRadius: BorderRadius.circular(PSpacing.radiusLG),
                border: Border.all(
                  color: _isFocused
                      ? AppColors.textOnAccent
                      : enabled
                      ? AppColors.borderStrong
                      : AppColors.borderSubtle,
                  width: _isFocused ? 3 : (widget.isDesktop ? 1.5 : 1.0),
                ),
                boxShadow: enabled
                    ? [
                        BoxShadow(
                          color: AppColors.shadowStrong,
                          blurRadius: widget.isDesktop ? 20 : 16,
                          offset: Offset(0, widget.isDesktop ? 10 : 8),
                        ),
                      ]
                    : null,
              ),
              child: Padding(
                padding: EdgeInsets.all(padding),
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.start,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Container(
                      width: iconContainerSize,
                      height: iconContainerSize,
                      decoration: BoxDecoration(
                        color: enabled
                            ? AppColors.textOnAccent.withValues(
                                alpha: widget.isDesktop ? 0.16 : 0.14,
                              )
                            : AppColors.backgroundElevated,
                        shape: BoxShape.circle,
                      ),
                      child: Icon(
                        widget.icon,
                        color: enabled
                            ? AppColors.textOnAccent
                            : AppColors.textDisabled,
                        size: iconSize,
                        semanticLabel: widget.title,
                      ),
                    ),
                    SizedBox(
                      height: widget.isDesktop
                          ? PSpacing.lg
                          : (widget.compact ? PSpacing.sm : PSpacing.md),
                    ),
                    const Spacer(),
                    Text(
                      widget.title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: titleStyle,
                    ),
                    SizedBox(
                      height: widget.isDesktop ? PSpacing.sm : PSpacing.xs,
                    ),
                    Text(
                      widget.subtitle,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: subtitleStyle,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
