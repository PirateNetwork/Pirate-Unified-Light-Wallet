/// Open source licenses screen (placeholder)
library;

import 'package:flutter/material.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

class LicensesScreen extends StatelessWidget {
  const LicensesScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Open Source Licenses',
      appBar: const PAppBar(
        title: 'Open Source Licenses',
        subtitle: 'We will publish the final text soon',
        showBackButton: true,
      ),
      body: Padding(
        padding: const EdgeInsets.all(AppSpacing.lg),
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
