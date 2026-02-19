import 'package:flutter/material.dart';
import 'tokens/spacing.dart';
import 'tokens/typography.dart';
import 'tokens/colors.dart';

/// Compatibility shims to support older Pirate* APIs used across the app.

class PirateSpacing {
  static const double xxs = PSpacing.xxs;
  static const double xs = PSpacing.xs;
  static const double sm = PSpacing.sm;
  static const double md = PSpacing.md;
  static const double lg = PSpacing.lg;
  static const double xl = PSpacing.xl;
  static const double xxl = PSpacing.xxl;
  static const double xxxl = PSpacing.xxxl;

  static double responsiveGutter(double screenWidth) =>
      PSpacing.responsiveGutter(screenWidth);

  static double responsivePadding(double screenWidth) =>
      PSpacing.responsivePadding(screenWidth);

  static EdgeInsets screenPadding(
    double screenWidth, {
    double vertical = PSpacing.lg,
  }) => PSpacing.screenPadding(screenWidth, vertical: vertical);

  static bool isMobile(double screenWidth) => PSpacing.isMobile(screenWidth);

  static bool isTablet(double screenWidth) => PSpacing.isTablet(screenWidth);

  static bool isDesktop(double screenWidth) => PSpacing.isDesktop(screenWidth);
}

class PirateTypography {
  static TextStyle get h1 => PTypography.heading1();
  static TextStyle get h2 => PTypography.heading2();
  static TextStyle get h3 => PTypography.heading3();
  static TextStyle get h4 => PTypography.heading4();

  static TextStyle get body => PTypography.bodyMedium();
  static TextStyle get bodySmall => PTypography.bodySmall();
  static TextStyle get bodyLarge => PTypography.bodyLarge();
}

class PirateColors {
  PirateColors._();

  // Background colors
  static const Color backgroundBase = PColors.backgroundBase;
  static const Color backgroundSurface = PColors.backgroundSurface;
  static const Color backgroundElevated = PColors.backgroundElevated;
  static const Color backgroundPanel = PColors.backgroundPanel;
  static const Color backgroundOverlay = PColors.backgroundOverlay;

  // Accent gradients
  static const Color gradientAStart = PColors.gradientAStart;
  static const Color gradientAEnd = PColors.gradientAEnd;
  static const Color gradientBStart = PColors.gradientBStart;
  static const Color gradientBEnd = PColors.gradientBEnd;
  static const Color highlight = PColors.highlight;

  // Text colors
  static const Color textPrimary = PColors.textPrimary;
  static const Color textSecondary = PColors.textSecondary;
  static const Color textTertiary = PColors.textTertiary;
  static const Color textDisabled = PColors.textDisabled;
  static const Color textOnAccent = PColors.textOnAccent;

  // Semantic colors
  static const Color success = PColors.success;
  static const Color successBackground = PColors.successBackground;
  static const Color successBorder = PColors.successBorder;
  static const Color warning = PColors.warning;
  static const Color warningBackground = PColors.warningBackground;
  static const Color warningBorder = PColors.warningBorder;
  static const Color error = PColors.error;
  static const Color errorBackground = PColors.errorBackground;
  static const Color errorBorder = PColors.errorBorder;
  static const Color info = PColors.info;
  static const Color infoBackground = PColors.infoBackground;
  static const Color infoBorder = PColors.infoBorder;

  // Interactive colors
  static const Color focusRing = PColors.focusRing;
  static const Color focusRingSubtle = PColors.focusRingSubtle;
  static const Color hoverOverlay = PColors.hoverOverlay;
  static const Color pressedOverlay = PColors.pressedOverlay;
  static const Color selectedBackground = PColors.selectedBackground;
  static const Color selectedBorder = PColors.selectedBorder;

  // Border colors
  static const Color borderDefault = PColors.borderDefault;
  static const Color borderSubtle = PColors.borderSubtle;
  static const Color borderStrong = PColors.borderStrong;
  static const Color border = PColors.borderDefault; // Alias for border

  // Aliases for common usage
  static const Color accent1 = PColors.gradientAStart;
  static const Color accent2 = PColors.gradientBStart;
  static const Color surface = PColors.backgroundSurface;
  static const Color background = PColors.backgroundBase;
}

class PirateTheme {
  static const Color accentColor = PColors.gradientAStart;
  static const Color accentSecondary = PColors.gradientBStart;
}
