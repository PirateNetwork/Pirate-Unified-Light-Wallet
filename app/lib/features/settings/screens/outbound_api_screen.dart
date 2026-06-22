library;

import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../design/deep_space_theme.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../providers/preferences_providers.dart';

class OutboundApiScreen extends ConsumerWidget {
  const OutboundApiScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final masterEnabled = ref.watch(externalApiMasterProvider);
    final priceEnabled = ref.watch(externalPriceApiProvider);
    final githubEnabled = ref.watch(externalGithubApiProvider);
    final desktopUpdateEnabled = ref.watch(externalDesktopUpdateApiProvider);
    final komodoSwapEnabled = ref.watch(externalKomodoSwapApiProvider);
    final isDesktop =
        Platform.isWindows || Platform.isMacOS || Platform.isLinux;

    return PScaffold(
      title: 'Outbound API Calls'.tr,
      appBar: PAppBar(
        title: 'Outbound API Calls'.tr,
        subtitle: 'Control non-lightserver internet requests'.tr,
        showBackButton: true,
      ),
      body: ListView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        children: [
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Allow non-lightserver API calls'.tr,
                          style: AppTypography.bodyBold,
                        ),
                        const SizedBox(height: AppSpacing.xs),
                        Text(
                          'Master switch for all outbound connections except your configured lightwalletd server.'
                              .tr,
                          style: AppTypography.caption.copyWith(
                            color: AppColors.textSecondary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: AppSpacing.md),
                  Switch.adaptive(
                    value: masterEnabled,
                    onChanged: (value) {
                      ref
                          .read(externalApiMasterProvider.notifier)
                          .setEnabled(enabled: value);
                    },
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: AppSpacing.md),
          _ApiToggleCard(
            title: 'Live Price Feeds'.tr,
            subtitle:
                'CoinGecko primary + CoinMarketCap fallback for ARRR prices and fiat conversion.'
                    .tr,
            enabled: priceEnabled,
            available: masterEnabled,
            onChanged: (value) {
              ref
                  .read(externalPriceApiProvider.notifier)
                  .setEnabled(enabled: value);
            },
          ),
          const SizedBox(height: AppSpacing.md),
          _ApiToggleCard(
            title: 'Verify Build GitHub Checks'.tr,
            subtitle:
                'When enabled, Verify Build fetches releases and checksum files from GitHub over your selected transport.'
                    .tr,
            enabled: githubEnabled,
            available: masterEnabled,
            onChanged: (value) {
              ref
                  .read(externalGithubApiProvider.notifier)
                  .setEnabled(enabled: value);
            },
          ),
          const SizedBox(height: AppSpacing.md),
          _ApiToggleCard(
            title: 'Komodo Swaps'.tr,
            subtitle:
                'Enables the local KDF swap engine, order books, swap quotes, and funding-balance checks when the selected wallet networking mode supports swaps.'
                    .tr,
            enabled: komodoSwapEnabled,
            available: masterEnabled,
            onChanged: (value) {
              ref
                  .read(externalKomodoSwapApiProvider.notifier)
                  .setEnabled(enabled: value);
            },
          ),
          if (isDesktop) ...[
            const SizedBox(height: AppSpacing.md),
            _ApiToggleCard(
              title: 'Desktop Update Checks'.tr,
              subtitle:
                  'When enabled, periodically checks GitHub for desktop updates over your selected transport.'
                      .tr,
              enabled: desktopUpdateEnabled,
              available: masterEnabled && githubEnabled,
              onChanged: (value) {
                ref
                    .read(externalDesktopUpdateApiProvider.notifier)
                    .setEnabled(enabled: value);
              },
            ),
          ],
          const SizedBox(height: AppSpacing.md),
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Text(
                'Current non-lightserver network features: live price feeds, Verify Build GitHub checks, Komodo swap order books and quotes, and desktop release checks. These outbound calls use your selected transport where supported and can be disabled in Settings.'
                    .tr,
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ApiToggleCard extends StatelessWidget {
  const _ApiToggleCard({
    required this.title,
    required this.subtitle,
    required this.enabled,
    required this.available,
    required this.onChanged,
  });

  final String title;
  final String subtitle;
  final bool enabled;
  final bool available;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final effectiveEnabled = available && enabled;

    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(AppSpacing.md),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: AppTypography.bodyBold.copyWith(
                      color: available
                          ? AppColors.textPrimary
                          : AppColors.textTertiary,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.xs),
                  Text(
                    subtitle,
                    style: AppTypography.caption.copyWith(
                      color: available
                          ? AppColors.textSecondary
                          : AppColors.textTertiary,
                    ),
                  ),
                  if (!available) ...[
                    const SizedBox(height: AppSpacing.xs),
                    Text(
                      'Disabled by master switch'.tr,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.warning,
                      ),
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(width: AppSpacing.md),
            Switch.adaptive(
              value: effectiveEnabled,
              onChanged: available ? onChanged : null,
            ),
          ],
        ),
      ),
    );
  }
}
