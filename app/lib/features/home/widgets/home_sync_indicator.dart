import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';

class HomeSyncIndicator extends StatelessWidget {
  const HomeSyncIndicator({
    required this.progress,
    required this.currentHeight,
    required this.targetHeight,
    required this.stage,
    required this.blocksPerSecond,
    required this.isSyncing,
    required this.isComplete,
    required this.reduceMotion,
    this.eta,
    super.key,
  });

  final double progress;
  final int currentHeight;
  final int targetHeight;
  final String stage;
  final String? eta;
  final double blocksPerSecond;
  final bool isSyncing;
  final bool isComplete;
  final bool reduceMotion;

  static const Key currentHeightKey = Key('sync-current-height');
  static const Key targetHeightKey = Key('sync-target-height');
  static const Key etaKey = Key('sync-eta');
  static const Key speedKey = Key('sync-speed');

  @override
  Widget build(BuildContext context) {
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
    final statusText =
        eta ??
        (isComplete
            ? (stage == 'Monitoring'.tr ? 'Up to date'.tr : 'Synced'.tr)
            : isSyncing
            ? 'Calculating...'.tr
            : null);
    final statusColor = isComplete && eta == null
        ? AppColors.success
        : AppColors.textSecondary;
    final blockProgressText = (targetHeight > 0 && currentHeight > 0)
        ? 'Block {currentHeight} / {targetHeight}'.trArgs({
            'currentHeight': currentHeight,
            'targetHeight': targetHeight,
          })
        : (currentHeight > 0)
        ? 'Block {currentHeight}'.trArgs({'currentHeight': currentHeight})
        : (targetHeight > 0)
        ? 'Block 0 / {targetHeight}'.trArgs({'targetHeight': targetHeight})
        : 'Block 0'.tr;
    return Semantics(
      container: true,
      liveRegion: true,
      label: 'Wallet sync status'.tr,
      value: blockProgressText,
      child: PCard(
        padding: const EdgeInsets.all(PSpacing.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
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
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: PTypography.caption().copyWith(
                      color: AppColors.textPrimary,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
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
                  semanticsLabel: 'Sync progress'.tr,
                ),
              ),
            ],
            const SizedBox(height: PSpacing.sm),
            _SyncMetrics(
              currentHeight: currentHeight,
              targetHeight: targetHeight,
              statusText: statusText,
              statusColor: statusColor,
              blocksPerSecond: blocksPerSecond,
            ),
          ],
        ),
      ),
    );
  }
}

class _SyncMetrics extends StatelessWidget {
  const _SyncMetrics({
    required this.currentHeight,
    required this.targetHeight,
    required this.statusText,
    required this.statusColor,
    required this.blocksPerSecond,
  });

  final int currentHeight;
  final int targetHeight;
  final String? statusText;
  final Color statusColor;
  final double blocksPerSecond;

  @override
  Widget build(BuildContext context) {
    final localizations = MaterialLocalizations.of(context);
    final metrics = <_SyncMetric>[
      _SyncMetric(
        label: 'Height'.tr,
        value: localizations.formatDecimal(currentHeight),
        valueKey: HomeSyncIndicator.currentHeightKey,
      ),
      if (targetHeight > 0)
        _SyncMetric(
          label: 'Target'.tr,
          value: localizations.formatDecimal(targetHeight),
          valueKey: HomeSyncIndicator.targetHeightKey,
        ),
      if (statusText != null)
        _SyncMetric(
          label: 'ETA'.tr,
          value: statusText!,
          valueColor: statusColor,
          valueKey: HomeSyncIndicator.etaKey,
        ),
      if (blocksPerSecond > 0)
        _SyncMetric(
          label: 'blk/s',
          value: blocksPerSecond.toStringAsFixed(1),
          valueKey: HomeSyncIndicator.speedKey,
        ),
    ];

    return LayoutBuilder(
      builder: (context, constraints) {
        const gap = PSpacing.md;
        final textScale = MediaQuery.textScalerOf(context).scale(1.0);
        final minimumMetricWidth = 120.0 * textScale.clamp(1.0, 2.0);
        final allMetricsWidth =
            (minimumMetricWidth * metrics.length) +
            (gap * math.max(0, metrics.length - 1));
        final columns = constraints.maxWidth >= allMetricsWidth
            ? metrics.length
            : constraints.maxWidth >= (minimumMetricWidth * 2) + gap
            ? math.min(2, metrics.length)
            : 1;
        final itemWidth = columns == 0
            ? constraints.maxWidth
            : (constraints.maxWidth - (gap * (columns - 1))) / columns;
        final orphanedLastMetric = columns > 1 && metrics.length % columns == 1;

        return Wrap(
          spacing: gap,
          runSpacing: PSpacing.sm,
          children: [
            for (var index = 0; index < metrics.length; index++)
              SizedBox(
                width: orphanedLastMetric && index == metrics.length - 1
                    ? constraints.maxWidth
                    : itemWidth,
                child: metrics[index],
              ),
          ],
        );
      },
    );
  }
}

class _SyncMetric extends StatelessWidget {
  const _SyncMetric({
    required this.label,
    required this.value,
    required this.valueKey,
    this.valueColor,
  });

  final String label;
  final String value;
  final Key valueKey;
  final Color? valueColor;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          label,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: PTypography.labelSmall(color: AppColors.textMuted),
        ),
        const SizedBox(height: PSpacing.xxs),
        Text(
          value,
          key: valueKey,
          style: PTypography.codeSmall(
            color: valueColor ?? AppColors.textSecondary,
          ).copyWith(fontFeatures: const [FontFeature.tabularFigures()]),
        ),
      ],
    );
  }
}
