/// Terms and privacy screen.
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
        subtitle: 'Self-custody wallet terms and privacy',
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _SectionCard(
              title: 'Terms of Use',
              body: '''
Pirate Wallet is self-custody software. You control your seed phrase, private keys, and funds.
You are solely responsible for securing recovery material, choosing trusted devices, and confirming transaction details before sending.

Use of this wallet is at your own risk. Transactions on Pirate Chain are irreversible once confirmed.
The app is provided "as is" without warranty of uptime, fitness, or recovery guarantees.
''',
            ),
            const SizedBox(height: AppSpacing.md),
            _SectionCard(
              title: 'Security Responsibility',
              body: '''
Never share your seed phrase or private keys.
If recovery material is lost, funds may be permanently unrecoverable.
If recovery material is exposed, funds may be stolen.

Before sending funds, verify the destination address and amount carefully.
''',
            ),
            const SizedBox(height: AppSpacing.md),
            _SectionCard(
              title: 'Privacy Policy',
              body: '''
Pirate Wallet is designed to minimize data collection.
The app does not require account registration and does not send seed phrases, private keys, or plaintext passphrases to our servers.

The wallet communicates with your configured lightwalletd endpoint to sync chain data and submit transactions.
If optional external features are enabled (for example market-price lookups), those requests are sent to the selected third-party providers and may include standard network metadata such as IP address and request timing.

You can disable optional outbound API calls in Settings.
''',
            ),
            const SizedBox(height: AppSpacing.md),
            _SectionCard(
              title: 'Compliance Notice',
              body: '''
You are responsible for complying with local laws and regulations when using this software.
''',
            ),
            const SizedBox(height: AppSpacing.lg),
            Text(
              'Last updated: February 15, 2026',
              style: AppTypography.caption.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({required this.title, required this.body});

  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(AppSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              title,
              style: AppTypography.h4.copyWith(color: AppColors.textPrimary),
            ),
            const SizedBox(height: AppSpacing.sm),
            SelectionArea(
              child: Text(
                body.trim(),
                style: AppTypography.body.copyWith(
                  color: AppColors.textPrimary,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
