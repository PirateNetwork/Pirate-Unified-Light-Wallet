library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

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

    return PScaffold(
      title: 'Outbound API Calls',
      appBar: const PAppBar(
        title: 'Outbound API Calls',
        subtitle: 'Control non-lightserver internet requests',
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
                          'Allow non-lightserver API calls',
                          style: AppTypography.bodyBold,
                        ),
                        const SizedBox(height: AppSpacing.xs),
                        Text(
                          'Master switch for all outbound connections except your configured lightwalletd server.',
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
            title: 'Live Price Feeds',
            subtitle:
                'CoinGecko primary + CoinMarketCap fallback for ARRR prices and fiat conversion.',
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
            title: 'Verify Build GitHub Checks',
            subtitle:
                'Fetches releases and checksum files from GitHub when using Verify Build.',
            enabled: githubEnabled,
            available: masterEnabled,
            onChanged: (value) {
              ref
                  .read(externalGithubApiProvider.notifier)
                  .setEnabled(enabled: value);
            },
          ),
          const SizedBox(height: AppSpacing.md),
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Text(
                'Current non-lightserver network features: live price feeds and Verify Build GitHub checks. Other wallet traffic stays on your selected lightwalletd transport.',
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
                      'Disabled by master switch',
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
