import 'package:flutter/material.dart';
import 'package:flutter/widget_previews.dart';

import '../design/theme.dart';
import '../design/tokens/colors.dart';
import '../design/tokens/spacing.dart';
import '../features/pay/pay_screen.dart';
import '../ui/atoms/p_button.dart';
import '../ui/atoms/p_input.dart';
import '../ui/molecules/p_card.dart';

@Preview(name: 'Pay Sheet', group: 'Wallet Shell', size: Size(390, 780))
Widget paySheetPreview() {
  return MaterialApp(
    theme: PTheme.dark(),
    home: Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: Align(
        alignment: Alignment.bottomCenter,
        child: PaySheet(
          onSend: () {},
          onReceive: () {},
          onBuy: () {},
          onSpend: () {},
        ),
      ),
    ),
  );
}

@Preview(name: 'Send Card', group: 'Send Flow', size: Size(430, 320))
Widget sendCardPreview() {
  return MaterialApp(
    theme: PTheme.dark(),
    home: Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: Padding(
        padding: const EdgeInsets.all(PSpacing.lg),
        child: PCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PInput(
                label: 'Recipient address',
                hint: 'zs1...',
                onChanged: (_) {},
              ),
              const SizedBox(height: PSpacing.md),
              PInput(
                label: 'Amount (ARRR)',
                hint: '0.00000000',
                onChanged: (_) {},
              ),
              const SizedBox(height: PSpacing.md),
              PButton(
                text: 'Review',
                onPressed: () {},
                variant: PButtonVariant.primary,
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

@Preview(name: 'Button States', group: 'Atoms', size: Size(360, 260))
Widget buttonStatesPreview() {
  return MaterialApp(
    theme: PTheme.dark(),
    home: Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: Padding(
        padding: const EdgeInsets.all(PSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PButton(
              text: 'Primary',
              onPressed: () {},
              variant: PButtonVariant.primary,
            ),
            const SizedBox(height: PSpacing.sm),
            PButton(
              text: 'Secondary',
              onPressed: () {},
              variant: PButtonVariant.secondary,
            ),
            const SizedBox(height: PSpacing.sm),
            PButton(
              text: 'Loading',
              onPressed: null,
              loading: true,
              variant: PButtonVariant.primary,
            ),
          ],
        ),
      ),
    ),
  );
}
