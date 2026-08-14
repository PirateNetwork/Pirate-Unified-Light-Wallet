import 'package:flutter/material.dart';

import 'tokens/colors.dart';

class PInputDecorations {
  PInputDecorations._();

  static InputDecorationThemeData elevatedDropdown(BuildContext context) {
    return Theme.of(context).inputDecorationTheme.copyWith(
      filled: true,
      fillColor: AppColors.surfaceElevated,
    );
  }
}
