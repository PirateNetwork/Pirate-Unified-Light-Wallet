/// Open source licenses screen.
library;

import 'package:flutter/material.dart';

import '../../../design/deep_space_theme.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

class LicensesScreen extends StatelessWidget {
  const LicensesScreen({super.key});

  static const String _mitLicense = '''
MIT License

Copyright (c) 2026 Pirate Chain Developers

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
''';

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Open Source Licenses',
      appBar: const PAppBar(
        title: 'Open Source Licenses',
        subtitle: 'Project and dependency licenses',
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: AppSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.lg),
                child: Text(
                  'Pirate Wallet is open-source and distributed under the MIT License.',
                  style: AppTypography.body.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
              ),
            ),
            const SizedBox(height: AppSpacing.md),
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.lg),
                child: SelectionArea(
                  child: SelectableText(
                    _mitLicense.trim(),
                    style: AppTypography.code,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
