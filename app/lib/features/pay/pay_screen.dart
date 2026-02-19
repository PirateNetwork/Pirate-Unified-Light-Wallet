import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';

bool _isDesktopPlatform() {
  if (kIsWeb) return false;
  return defaultTargetPlatform == TargetPlatform.windows ||
      defaultTargetPlatform == TargetPlatform.macOS ||
      defaultTargetPlatform == TargetPlatform.linux;
}

void _showComingSoon(BuildContext context) {
  showDialog<void>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      backgroundColor: AppColors.backgroundSurface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(PSpacing.radiusLG),
      ),
      title: Text(
        'Coming Soon'.tr,
        style: PTypography.heading5(color: AppColors.textPrimary),
      ),
      content: Text(
        'This feature is under development.'.tr,
        style: PTypography.bodyMedium(color: AppColors.textSecondary),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(),
          child: Text(
            'OK'.tr,
            style: PTypography.labelMedium(color: AppColors.accentPrimary),
          ),
        ),
      ],
    ),
  );
}

/// Pay entry screen for desktop and deep links.
class PayScreen extends StatelessWidget {
  const PayScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  Widget build(BuildContext context) {
    final size = MediaQuery.of(context).size;
    final isMobile = PSpacing.isMobile(size.width);
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
      onBuy: () => _showComingSoon(context),
      onSpend: () => _showComingSoon(context),
    );
    final isDesktopPlatform = _isDesktopPlatform();

    if (!useScaffold) {
      if (isDesktopPlatform) {
        return content;
      }
      return PScaffold(
        title: 'Pay'.tr,
        useSafeArea: false,
        appBar: PAppBar(
          title: 'Pay'.tr,
          subtitle: 'Send, receive, buy, or spend in a few taps.'.tr,
          actions: appBarActions,
        ),
        body: content,
      );
    }

    return PScaffold(
      title: 'Pay'.tr,
      appBar: isDesktopPlatform
          ? null
          : PAppBar(
              title: 'Pay'.tr,
              subtitle: 'Send, receive, buy, or spend in a few taps.'.tr,
              actions: appBarActions,
            ),
      body: content,
    );
  }
}

/// Mobile pay sheet with primary actions.
class PaySheet extends StatelessWidget {
  const PaySheet({
    required this.onSend,
    required this.onReceive,
    required this.onBuy,
    required this.onSpend,
    super.key,
  });

  final VoidCallback onSend;
  final VoidCallback onReceive;
  final VoidCallback onBuy;
  final VoidCallback onSpend;

  @override
  Widget build(BuildContext context) {
    final maxSheetHeight = MediaQuery.of(context).size.height * 0.75;
    return Container(
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: const BorderRadius.vertical(
          top: Radius.circular(PSpacing.radiusXL),
        ),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      padding: const EdgeInsets.fromLTRB(
        PSpacing.lg,
        PSpacing.sm,
        PSpacing.lg,
        PSpacing.xl,
      ),
      child: SafeArea(
        top: false,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final columns = constraints.maxWidth < 360 ? 1 : 2;
            const spacing = PSpacing.md;
            final tileWidth =
                (constraints.maxWidth - spacing * (columns - 1)) / columns;
            final tileHeight = (tileWidth * 0.78).clamp(130.0, 170.0);
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
                title: 'Buy'.tr,
                subtitle: 'Buy ARRR'.tr,
                icon: Icons.shopping_bag,
                gradient: LinearGradient(
                  colors: [AppColors.highlight, AppColors.warning],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: onBuy,
                compact: true,
              ),
              _PayActionTile(
                title: 'Spend'.tr,
                subtitle: 'Spend ARRR'.tr,
                icon: Icons.credit_card,
                gradient: LinearGradient(
                  colors: [AppColors.info, AppColors.gradientAEnd],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                onTap: onSpend,
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
                    'Pay'.tr,
                    style: PTypography.heading4(color: AppColors.textPrimary),
                  ),
                  const SizedBox(height: PSpacing.xs),
                  Text(
                    'Send, receive, buy, or spend ARRR.'.tr,
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  const SizedBox(height: PSpacing.lg),
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
    required this.onBuy,
    required this.onSpend,
  });

  final VoidCallback onSend;
  final VoidCallback onReceive;
  final VoidCallback onBuy;
  final VoidCallback onSpend;

  @override
  Widget build(BuildContext context) {
    final isDesktopPlatform = _isDesktopPlatform();
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
        title: 'Buy'.tr,
        subtitle: 'Buy ARRR'.tr,
        icon: Icons.shopping_bag,
        gradient: LinearGradient(
          colors: [AppColors.highlight, AppColors.warning],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: onBuy,
      ),
      _PayActionTile(
        title: 'Spend'.tr,
        subtitle: 'Spend ARRR'.tr,
        icon: Icons.credit_card,
        gradient: LinearGradient(
          colors: [AppColors.info, AppColors.gradientAEnd],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        onTap: onSpend,
      ),
    ];

    return Padding(
      padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final spacing = constraints.maxWidth >= 900
              ? PSpacing.lg
              : PSpacing.md;

          if (isDesktopPlatform) {
            const crossAxisCount = 2;
            final tileWidth =
                (constraints.maxWidth - spacing * (crossAxisCount - 1)) /
                crossAxisCount;
            final viewportHeight = constraints.hasBoundedHeight
                ? constraints.maxHeight
                : (tileWidth * 1.6) + spacing;
            final tileHeight = ((viewportHeight - spacing) / 2).clamp(
              150.0,
              520.0,
            );
            final aspectRatio = tileWidth / tileHeight;
            final compactDesktop = tileWidth < 280 || tileHeight < 190;

            return GridView.builder(
              padding: EdgeInsets.zero,
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
            );
          }

          final crossAxisCount = constraints.maxWidth >= 560 ? 2 : 1;
          final tileWidth =
              (constraints.maxWidth - (spacing * (crossAxisCount - 1))) /
              crossAxisCount;
          final tileHeight = (tileWidth * 0.86).clamp(168.0, 360.0);
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
                );
              },
            ),
          );
        },
      ),
    );
  }
}

class _PayActionTile extends StatelessWidget {
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
  final VoidCallback onTap;
  final bool compact;
  final bool isDesktop;

  @override
  Widget build(BuildContext context) {
    final padding = isDesktop
        ? PSpacing.xl
        : compact
        ? PSpacing.md
        : PSpacing.lg;
    final iconSize = isDesktop
        ? 32.0
        : compact
        ? 24.0
        : 28.0;
    final iconContainerSize = isDesktop ? 56.0 : (compact ? 40.0 : 48.0);
    final titleStyle = isDesktop
        ? PTypography.heading5(color: AppColors.textOnAccent)
        : compact
        ? PTypography.titleMedium(color: AppColors.textOnAccent)
        : PTypography.heading6(color: AppColors.textOnAccent);
    final subtitleStyle = isDesktop
        ? PTypography.bodyMedium(
            color: AppColors.textOnAccent.withValues(alpha: 0.9),
          )
        : compact
        ? PTypography.bodySmall(
            color: AppColors.textOnAccent.withValues(alpha: 0.8),
          )
        : PTypography.bodyMedium(
            color: AppColors.textOnAccent.withValues(alpha: 0.85),
          );

    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(PSpacing.radiusLG),
        child: Ink(
          decoration: BoxDecoration(
            gradient: gradient,
            borderRadius: BorderRadius.circular(PSpacing.radiusLG),
            border: Border.all(
              color: AppColors.borderStrong,
              width: isDesktop ? 1.5 : 1.0,
            ),
            boxShadow: [
              BoxShadow(
                color: AppColors.shadowStrong,
                blurRadius: isDesktop ? 20 : 16,
                offset: Offset(0, isDesktop ? 10 : 8),
              ),
            ],
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
                    color: AppColors.textOnAccent.withValues(
                      alpha: isDesktop ? 0.16 : 0.14,
                    ),
                    shape: BoxShape.circle,
                  ),
                  child: Icon(
                    icon,
                    color: AppColors.textOnAccent,
                    size: iconSize,
                    semanticLabel: title,
                  ),
                ),
                SizedBox(
                  height: isDesktop
                      ? PSpacing.lg
                      : (compact ? PSpacing.sm : PSpacing.md),
                ),
                const Spacer(),
                Text(
                  title,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: titleStyle,
                ),
                SizedBox(height: isDesktop ? PSpacing.sm : PSpacing.xs),
                Text(
                  subtitle,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: subtitleStyle,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
