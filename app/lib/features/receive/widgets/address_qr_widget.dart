import 'package:flutter/material.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Widget displaying address as QR code
class AddressQRWidget extends StatelessWidget {
  final String address;
  final String? qrData;
  final VoidCallback onCopy;
  final VoidCallback onShare;
  final String? copyTooltip;
  final String? shareTooltip;

  const AddressQRWidget({
    super.key,
    required this.address,
    required this.onCopy,
    required this.onShare,
    this.qrData,
    this.copyTooltip,
    this.shareTooltip,
  });

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Padding(
        padding: EdgeInsets.all(PSpacing.lg),
        child: Column(
          children: [
            // QR Code
            Container(
              padding: EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.qrBackground,
                borderRadius: BorderRadius.circular(PSpacing.radiusCard),
              ),
              child: QrImageView(
                data: qrData ?? address,
                version: QrVersions.auto,
                size: 240,
                backgroundColor: AppColors.qrBackground,
                padding: EdgeInsets.all(PSpacing.sm),
                errorCorrectionLevel: QrErrorCorrectLevel.H,
              ),
            ),

            SizedBox(height: PSpacing.lg),

            // Address Display
            Container(
              padding: EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.backgroundPanel,
                borderRadius: BorderRadius.circular(PSpacing.radiusInput),
                border: Border.all(color: AppColors.borderSubtle, width: 1),
              ),
              child: SelectableText(
                address,
                style: PTypography.codeMedium().copyWith(
                  fontSize: 12,
                  color: AppColors.textSecondary,
                ),
                textAlign: TextAlign.center,
              ),
            ),

            SizedBox(height: PSpacing.md),

            // Quick Actions
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                IconButton(
                  onPressed: onCopy,
                  icon: const Icon(Icons.copy, size: 20),
                  tooltip: copyTooltip ?? 'Copy address',
                  style: IconButton.styleFrom(
                    backgroundColor: AppColors.backgroundPanel,
                    foregroundColor: AppColors.textPrimary,
                  ),
                ),
                SizedBox(width: PSpacing.sm),
                IconButton(
                  onPressed: onShare,
                  icon: const Icon(Icons.share, size: 20),
                  tooltip: shareTooltip ?? 'Share address',
                  style: IconButton.styleFrom(
                    backgroundColor: AppColors.backgroundPanel,
                    foregroundColor: AppColors.textPrimary,
                  ),
                ),
              ],
            ),

            SizedBox(height: PSpacing.sm),

            // Address Type Badge
            Container(
              padding: EdgeInsets.symmetric(
                horizontal: PSpacing.md,
                vertical: PSpacing.xs,
              ),
              decoration: BoxDecoration(
                color: AppColors.selectedBackground,
                borderRadius: BorderRadius.circular(PSpacing.radiusFull),
                border: Border.all(color: AppColors.selectedBorder),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                    Icons.lock_outlined,
                    size: 14,
                    color: AppColors.focusRing,
                  ),
                  SizedBox(width: PSpacing.xs),
                  Text(
                    'Shielded address'.tr,
                    style: PTypography.labelSmall().copyWith(
                      color: AppColors.focusRing,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
