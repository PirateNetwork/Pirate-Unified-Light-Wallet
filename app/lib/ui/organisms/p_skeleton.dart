import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';

/// Pirate Wallet Skeleton Loader
class PSkeleton extends StatelessWidget {
  const PSkeleton({
    this.width,
    this.height = 16.0,
    this.borderRadius,
    super.key,
  });

  final double? width;
  final double height;
  final double? borderRadius;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(borderRadius ?? PSpacing.radiusSM),
      ),
    )
        .animate(onPlay: (controller) => controller.repeat())
        .shimmer(
          duration: 1500.ms,
          color: AppColors.borderDefault,
        );
  }
}

/// Skeleton for cards
class PSkeletonCard extends StatelessWidget {
  const PSkeletonCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: EdgeInsets.all(PSpacing.cardPadding),
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(PSpacing.radiusLG),
        border: Border.all(
          color: AppColors.borderSubtle,
          width: 1.0,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          PSkeleton(width: 120, height: 20, borderRadius: PSpacing.radiusSM),
          SizedBox(height: PSpacing.sm),
          PSkeleton(width: double.infinity, height: 16),
          SizedBox(height: PSpacing.xs),
          PSkeleton(width: 200, height: 16),
        ],
      ),
    );
  }
}

/// Skeleton for list items
class PSkeletonListTile extends StatelessWidget {
  const PSkeletonListTile({super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.symmetric(
        horizontal: PSpacing.md,
        vertical: PSpacing.sm,
      ),
      child: Row(
        children: [
          PSkeleton(
            width: PSpacing.iconXL,
            height: PSpacing.iconXL,
            borderRadius: PSpacing.radiusFull,
          ),
          SizedBox(width: PSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                PSkeleton(width: 150, height: 16),
                SizedBox(height: PSpacing.xs),
                PSkeleton(width: 100, height: 14),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

