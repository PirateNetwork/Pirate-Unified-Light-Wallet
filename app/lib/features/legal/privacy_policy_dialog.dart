import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../core/i18n/arb_text_localizer.dart';
import '../../design/deep_space_theme.dart';
import 'legal_content.dart';

const privacyPolicyDialogKey = Key('privacy-policy-dialog');
const privacyPolicyLinkKey = Key('privacy-policy-link');
const privacyPolicyCloseKey = Key('privacy-policy-close');
const privacyPolicyScrollViewKey = Key('privacy-policy-scroll-view');

Future<void> showPrivacyPolicyDialog(BuildContext context) {
  return showDialog<void>(
    context: context,
    builder: (dialogContext) {
      final screenSize = MediaQuery.sizeOf(dialogContext);
      final dialogWidth = math.min(
        640.0,
        screenSize.width - (AppSpacing.md * 2),
      );
      final dialogHeight = math.min(640.0, screenSize.height * 0.8);

      return Dialog(
        key: privacyPolicyDialogKey,
        insetPadding: const EdgeInsets.all(AppSpacing.md),
        backgroundColor: AppColors.backgroundElevated,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
        child: SizedBox(
          width: dialogWidth,
          height: dialogHeight,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  AppSpacing.lg,
                  AppSpacing.md,
                  AppSpacing.sm,
                  AppSpacing.md,
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        'Privacy Policy'.tr,
                        style: AppTypography.h3.copyWith(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ),
                    IconButton(
                      key: privacyPolicyCloseKey,
                      tooltip: 'Close'.tr,
                      onPressed: () => Navigator.of(dialogContext).pop(),
                      icon: const Icon(Icons.close),
                    ),
                  ],
                ),
              ),
              Divider(height: 1, color: AppColors.divider),
              Expanded(
                child: SingleChildScrollView(
                  key: privacyPolicyScrollViewKey,
                  padding: const EdgeInsets.all(AppSpacing.lg),
                  child: SelectionArea(
                    child: Text(
                      localizedPrivacyPolicyText().trim(),
                      style: AppTypography.body.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      );
    },
  );
}

class PrivacyPolicyAgreement extends StatelessWidget {
  const PrivacyPolicyAgreement({super.key});

  @override
  Widget build(BuildContext context) {
    final agreement =
        'By continuing, you agree to the Terms and Privacy Policy'.tr;
    final policyLabel = 'Privacy Policy'.tr;
    final policyStart = agreement.lastIndexOf(policyLabel);
    final prefix = policyStart < 0
        ? '$agreement '
        : agreement.substring(0, policyStart);
    final suffix = policyStart < 0
        ? ''
        : agreement.substring(policyStart + policyLabel.length);
    final baseStyle = AppTypography.caption.copyWith(
      color: AppColors.textTertiary,
    );

    return Text.rich(
      TextSpan(
        style: baseStyle,
        children: [
          TextSpan(text: prefix),
          WidgetSpan(
            alignment: PlaceholderAlignment.baseline,
            baseline: TextBaseline.alphabetic,
            child: Semantics(
              link: true,
              child: InkWell(
                key: privacyPolicyLinkKey,
                onTap: () => showPrivacyPolicyDialog(context),
                child: Text(
                  policyLabel,
                  style: baseStyle.copyWith(
                    color: AppColors.accentPrimary,
                    decoration: TextDecoration.underline,
                    decorationColor: AppColors.accentPrimary,
                  ),
                ),
              ),
            ),
          ),
          if (suffix.isNotEmpty) TextSpan(text: suffix),
        ],
      ),
      textAlign: TextAlign.center,
    );
  }
}
