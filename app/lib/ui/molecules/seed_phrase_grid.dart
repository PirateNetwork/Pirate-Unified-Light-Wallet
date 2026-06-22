import 'package:flutter/material.dart';

import '../../core/i18n/arb_text_localizer.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

class SeedPhraseGrid extends StatelessWidget {
  const SeedPhraseGrid({super.key, required this.words});

  final List<String> words;

  @override
  Widget build(BuildContext context) {
    final textScale = MediaQuery.textScalerOf(context).scale(1.0);

    return LayoutBuilder(
      builder: (context, constraints) {
        final columns = _columnsForWidth(constraints.maxWidth, textScale);
        final gap = columns >= 4 ? PSpacing.xs : PSpacing.sm;

        return GridView.builder(
          padding: EdgeInsets.zero,
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: columns,
            crossAxisSpacing: gap,
            mainAxisSpacing: gap,
            mainAxisExtent: _cellHeight(columns, textScale),
          ),
          itemCount: words.length,
          itemBuilder: (context, index) {
            return _SeedWordCell(
              index: index,
              word: words[index],
              compact: columns >= 4,
            );
          },
        );
      },
    );
  }

  int _columnsForWidth(double width, double textScale) {
    if (textScale >= 1.25) {
      if (width >= 680) return 4;
      if (width >= 360) return 3;
      return 2;
    }
    if (width >= 680) return 6;
    if (width >= 360) return 4;
    if (width >= 300) return 3;
    return 2;
  }

  double _cellHeight(int columns, double textScale) {
    final scale = textScale.clamp(1.0, 1.35);
    final baseHeight = switch (columns) {
      >= 6 => 40.0,
      >= 4 => 42.0,
      _ => 46.0,
    };
    return baseHeight * scale;
  }
}

class _SeedWordCell extends StatelessWidget {
  const _SeedWordCell({
    required this.index,
    required this.word,
    required this.compact,
  });

  final int index;
  final String word;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final numberStyle = PTypography.caption(
      color: AppColors.textSecondary,
    ).copyWith(fontFeatures: const [FontFeature.tabularFigures()]);
    final wordStyle = PTypography.labelMedium(
      color: AppColors.textPrimary,
    ).copyWith(fontWeight: PTypography.semiBold);

    return Semantics(
      label: 'Seed word {number}: {word}'.trArgs({
        'number': index + 1,
        'word': word,
      }),
      child: Container(
        alignment: Alignment.center,
        padding: EdgeInsets.symmetric(
          horizontal: compact ? PSpacing.xs : PSpacing.sm,
          vertical: PSpacing.xxs,
        ),
        decoration: BoxDecoration(
          color: AppColors.backgroundElevated,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          border: Border.all(color: AppColors.borderDefault),
        ),
        child: Row(
          children: [
            SizedBox(
              width: 24,
              child: Text('${index + 1}.', style: numberStyle),
            ),
            const SizedBox(width: PSpacing.xxs),
            Expanded(
              child: FittedBox(
                fit: BoxFit.scaleDown,
                alignment: Alignment.centerLeft,
                child: Text(
                  word,
                  maxLines: 1,
                  softWrap: false,
                  style: wordStyle,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
