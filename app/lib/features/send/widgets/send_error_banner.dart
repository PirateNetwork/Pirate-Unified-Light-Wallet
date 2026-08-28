import 'package:flutter/material.dart';

import '../../../design/deep_space_theme.dart';

/// An accessible, wrapping status banner for send validation failures.
class SendErrorBanner extends StatelessWidget {
  const SendErrorBanner({required this.message, super.key});

  static const messageKey = Key('send-error-message');
  static const semanticsKey = Key('send-error-semantics');

  final String message;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      key: semanticsKey,
      container: true,
      liveRegion: true,
      label: message,
      child: ExcludeSemantics(
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: AppColors.error.withValues(alpha: 0.10),
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: AppColors.error.withValues(alpha: 0.32)),
          ),
          child: Padding(
            padding: const EdgeInsets.all(AppSpacing.md),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(Icons.error_outline, color: AppColors.error, size: 20),
                const SizedBox(width: AppSpacing.sm),
                Expanded(
                  child: Text(
                    message,
                    key: messageKey,
                    style: AppTypography.bodySmall.copyWith(
                      color: AppColors.textPrimary,
                      height: 1.45,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
