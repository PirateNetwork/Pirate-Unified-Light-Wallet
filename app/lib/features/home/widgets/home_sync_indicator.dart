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

    return Semantics(
      container: true,
      liveRegion: true,
      label: 'Wallet sync status'.tr,
      value: '$stage, ${(progress * 100).toStringAsFixed(1)} percent complete',
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
            Row(
              children: [
                Expanded(
                  child: Text(
                    (targetHeight > 0 && currentHeight > 0)
                        ? 'Block {currentHeight} / {targetHeight}'.trArgs({
                            'currentHeight': currentHeight,
                            'targetHeight': targetHeight,
                          })
                        : (currentHeight > 0)
                        ? 'Block {currentHeight}'.trArgs({
                            'currentHeight': currentHeight,
                          })
                        : (targetHeight > 0)
                        ? 'Block 0 / {targetHeight}'.trArgs({
                            'targetHeight': targetHeight,
                          })
                        : 'Block 0'.tr,
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
                    stage == 'Monitoring'.tr ? 'Up to date'.tr : 'Synced'.tr,
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
    );
  }
}
