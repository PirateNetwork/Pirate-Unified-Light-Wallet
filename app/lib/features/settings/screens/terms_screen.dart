/// Terms and privacy screen (placeholder)
library;

import 'package:flutter/material.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

class TermsScreen extends StatelessWidget {
  const TermsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Terms and Privacy',
      appBar: const PAppBar(
        title: 'Terms and Privacy',
        subtitle: 'We will publish the final text soon',
        showBackButton: true,
      ),
      body: Padding(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: PCard(
          child: Padding(
            padding: const EdgeInsets.all(AppSpacing.lg),
            child: Text(
              'TBD',
              style: AppTypography.h3.copyWith(
                color: AppColors.textPrimary,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
