/// Watch-Only Wallet Banner Component
/// 
/// Displays a clear "Incoming only" label for watch-only wallets.
/// Used in wallet headers, transaction screens, and send screens.
library;

import 'package:flutter/material.dart';

import '../../design/deep_space_theme.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../atoms/p_text_button.dart';

/// Banner style variants
enum WatchOnlyBannerStyle {
  /// Full banner with icon and description
  full,
  /// Compact chip/badge
  chip,
  /// Minimal inline text
  inline,
}

/// Watch-only wallet banner
/// 
/// Clearly labels watch-only wallets as "Incoming only" to prevent
/// user confusion about spending capabilities.
class WatchOnlyBanner extends StatelessWidget {
  final WatchOnlyBannerStyle style;
  final VoidCallback? onTap;
  
  const WatchOnlyBanner({
    super.key,
    this.style = WatchOnlyBannerStyle.full,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    switch (style) {
      case WatchOnlyBannerStyle.full:
        return _FullBanner(onTap: onTap);
      case WatchOnlyBannerStyle.chip:
        return _ChipBanner(onTap: onTap);
      case WatchOnlyBannerStyle.inline:
        return _InlineBanner();
    }
  }
}

/// Full banner with icon and description
class _FullBanner extends StatelessWidget {
  final VoidCallback? onTap;
  
  const _FullBanner({this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(AppSpacing.md),
        decoration: BoxDecoration(
          gradient: LinearGradient(
            colors: [
              AppColors.accentPrimary.withValues(alpha: 0.1),
              AppColors.accentSecondary.withValues(alpha: 0.1),
            ],
          ),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: AppColors.accentPrimary.withValues(alpha: 0.3),
          ),
        ),
        child: Row(
          children: [
            Container(
              padding: const EdgeInsets.all(AppSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.accentPrimary.withValues(alpha: 0.2),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(
                Icons.visibility,
                color: AppColors.accentPrimary,
                size: 20,
                semanticLabel: 'Watch-only wallet',
              ),
            ),
            const SizedBox(width: AppSpacing.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    kWatchOnlyLabel,
                    style: AppTypography.bodyBold.copyWith(
                      color: AppColors.accentPrimary,
                    ),
                  ),
                  Text(
                    kWatchOnlyBannerMessage,
                    style: AppTypography.caption.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                ],
              ),
            ),
            if (onTap != null)
              Icon(
                Icons.info_outline,
                color: AppColors.textTertiary,
                size: 20,
              ),
          ],
        ),
      ),
    );
  }
}

/// Compact chip/badge banner
class _ChipBanner extends StatelessWidget {
  final VoidCallback? onTap;
  
  const _ChipBanner({this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpacing.md,
          vertical: AppSpacing.sm,
        ),
        decoration: BoxDecoration(
          color: AppColors.accentPrimary.withValues(alpha: 0.15),
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: AppColors.accentPrimary.withValues(alpha: 0.3),
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.visibility,
              color: AppColors.accentPrimary,
              size: 16,
              semanticLabel: 'Watch-only',
            ),
            const SizedBox(width: AppSpacing.xs),
            Text(
              kWatchOnlyLabel,
              style: AppTypography.caption.copyWith(
                color: AppColors.accentPrimary,
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Minimal inline text banner
class _InlineBanner extends StatelessWidget {
  const _InlineBanner();

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          Icons.visibility,
          color: AppColors.textSecondary,
          size: 14,
          semanticLabel: 'Watch-only',
        ),
        const SizedBox(width: 4),
        Text(
          kWatchOnlyLabel,
          style: AppTypography.caption.copyWith(
            color: AppColors.textSecondary,
            fontStyle: FontStyle.italic,
          ),
        ),
      ],
    );
  }
}

/// Watch-only warning dialog for send attempts
class WatchOnlyWarningDialog extends StatelessWidget {
  const WatchOnlyWarningDialog({super.key});

  static Future<void> show(BuildContext context) {
    return showDialog(
      context: context,
      builder: (_) => const WatchOnlyWarningDialog(),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      backgroundColor: AppColors.surface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(16),
      ),
      title: Row(
        children: [
          Icon(
            Icons.visibility,
            color: AppColors.accentPrimary,
          ),
          const SizedBox(width: AppSpacing.sm),
          Text(
            kWatchOnlyLabel,
            style: AppTypography.h3.copyWith(
              color: AppColors.textPrimary,
            ),
          ),
        ],
      ),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'This is a watch-only wallet. It can only view incoming transactions.',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
          const SizedBox(height: AppSpacing.md),
          Text(
            'To send funds, you need access to the full wallet with the spending key.',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
        ],
      ),
      actions: [
        PTextButton(
          label: 'Got it',
          onPressed: () => Navigator.of(context).pop(),
        ),
      ],
    );
  }
}

