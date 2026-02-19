library;

import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../design/deep_space_theme.dart';
import '../../../l10n/app_localizations.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../providers/preferences_providers.dart';

class OutboundApiScreen extends ConsumerWidget {
  const OutboundApiScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final masterEnabled = ref.watch(externalApiMasterProvider);
    final priceEnabled = ref.watch(externalPriceApiProvider);
    final githubEnabled = ref.watch(externalGithubApiProvider);
    final desktopUpdateEnabled = ref.watch(externalDesktopUpdateApiProvider);
    final isDesktop =
        Platform.isWindows || Platform.isMacOS || Platform.isLinux;

    return PScaffold(
      title: l10n.outboundApiTitle,
      appBar: PAppBar(
        title: l10n.outboundApiTitle,
        subtitle: l10n.outboundApiSubtitle,
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
                          l10n.allowNonLightserverApiCalls,
                          style: AppTypography.bodyBold,
                        ),
                        const SizedBox(height: AppSpacing.xs),
                        Text(
                          l10n.allowNonLightserverApiCallsHelp,
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
            title: l10n.livePriceFeedsTitle,
            subtitle: l10n.livePriceFeedsSubtitle,
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
            title: l10n.verifyBuildGithubChecksTitle,
            subtitle: l10n.verifyBuildGithubChecksSubtitle,
            enabled: githubEnabled,
            available: masterEnabled,
            onChanged: (value) {
              ref
                  .read(externalGithubApiProvider.notifier)
                  .setEnabled(enabled: value);
            },
          ),
          if (isDesktop) ...[
            const SizedBox(height: AppSpacing.md),
            _ApiToggleCard(
              title: l10n.desktopUpdateChecksTitle,
              subtitle: l10n.desktopUpdateChecksSubtitle,
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
                l10n.outboundApiFeaturesSummary,
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
    final l10n = AppLocalizations.of(context);
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
                      l10n.disabledByMasterSwitch,
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
