import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/input_decorations.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/design/tokens/colors.dart';

void main() {
  test('dark floating labels reflect their current field state', () {
    final style = PTheme.dark().inputDecorationTheme.floatingLabelStyle;

    expect(_resolve(style, const {}).color, PColors.textSecondary);
    expect(
      _resolve(style, const {WidgetState.focused}).color,
      PColors.focusRing,
    );
    expect(
      _resolve(style, const {WidgetState.disabled}).color,
      PColors.textDisabled,
    );
    expect(_resolve(style, const {WidgetState.error}).color, PColors.error);
  });

  test('light floating labels reflect their current field state', () {
    final style = PTheme.light().inputDecorationTheme.floatingLabelStyle;

    expect(_resolve(style, const {}).color, PColorsLight.textSecondary);
    expect(
      _resolve(style, const {WidgetState.focused}).color,
      PColorsLight.focusRing,
    );
    expect(
      _resolve(style, const {WidgetState.disabled}).color,
      PColorsLight.textDisabled,
    );
    expect(
      _resolve(style, const {WidgetState.error}).color,
      PColorsLight.error,
    );
  });

  testWidgets('elevated dropdowns preserve inherited field geometry', (
    tester,
  ) async {
    late InputDecorationThemeData dropdownTheme;
    final theme = PTheme.dark();

    await tester.pumpWidget(
      MaterialApp(
        theme: theme,
        home: Builder(
          builder: (context) {
            dropdownTheme = PInputDecorations.elevatedDropdown(context);
            return const SizedBox();
          },
        ),
      ),
    );

    final inherited = theme.inputDecorationTheme;
    expect(dropdownTheme.fillColor, AppColors.surfaceElevated);
    expect(dropdownTheme.contentPadding, inherited.contentPadding);
    expect(dropdownTheme.enabledBorder, inherited.enabledBorder);
    expect(dropdownTheme.focusedBorder, inherited.focusedBorder);
    expect(
      _resolve(dropdownTheme.floatingLabelStyle, const {}).color,
      PColors.textSecondary,
    );
    expect(
      _resolve(dropdownTheme.floatingLabelStyle, const {
        WidgetState.focused,
      }).color,
      PColors.focusRing,
    );
  });
}

TextStyle _resolve(TextStyle? style, Set<WidgetState> states) {
  expect(style, isA<WidgetStateTextStyle>());
  return (style! as WidgetStateTextStyle).resolve(states);
}
