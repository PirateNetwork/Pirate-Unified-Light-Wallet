import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/theme.dart';

void main() {
  group('scrollbar theme', () {
    test('keeps the dark thumb quiet until interaction', () {
      final theme = PTheme.dark().scrollbarTheme;

      _expectResponsiveScrollbar(theme);
      expect(
        theme.thumbColor!.resolve(const {}),
        isNot(theme.thumbColor!.resolve({WidgetState.hovered})),
      );
      expect(
        theme.thumbColor!.resolve({WidgetState.hovered}),
        isNot(theme.thumbColor!.resolve({WidgetState.dragged})),
      );
    });

    test('keeps the light thumb quiet until interaction', () {
      final theme = PTheme.light().scrollbarTheme;

      _expectResponsiveScrollbar(theme);
      expect(
        theme.thumbColor!.resolve(const {}),
        isNot(theme.thumbColor!.resolve({WidgetState.hovered})),
      );
      expect(
        theme.thumbColor!.resolve({WidgetState.hovered}),
        isNot(theme.thumbColor!.resolve({WidgetState.dragged})),
      );
    });
  });
}

void _expectResponsiveScrollbar(ScrollbarThemeData theme) {
  expect(theme.thickness!.resolve(const {}), 4.0);
  expect(theme.thickness!.resolve({WidgetState.hovered}), 6.0);
  expect(theme.thickness!.resolve({WidgetState.dragged}), 6.0);
  expect(theme.radius, const Radius.circular(4));
  expect(theme.crossAxisMargin, 4.0);
  expect(theme.mainAxisMargin, 8.0);
  expect(theme.minThumbLength, 48.0);
}
