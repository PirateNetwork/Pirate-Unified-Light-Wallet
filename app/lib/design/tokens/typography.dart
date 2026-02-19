import 'package:flutter/material.dart';

/// Design tokens for typography - Sora for UI, JetBrains Mono for code
///
/// Defines a comprehensive type scale with optical sizing for different
/// text hierarchies. Fonts are bundled locally to avoid runtime fetching.
class PTypography {
  PTypography._();

  // ============================================================================
  // Font Families
  // ============================================================================

  /// Sora font family for UI text (local asset)
  static String get fontFamilyUI => 'Sora';

  /// JetBrains Mono font family for code/monospace text (local asset)
  static String get fontFamilyMono => 'JetBrainsMono';

  // ============================================================================
  // Font Weights
  // ============================================================================

  static const FontWeight light = FontWeight.w300;
  static const FontWeight regular = FontWeight.w400;
  static const FontWeight medium = FontWeight.w500;
  static const FontWeight semiBold = FontWeight.w600;
  static const FontWeight bold = FontWeight.w700;
  static const FontWeight extraBold = FontWeight.w800;

  // ============================================================================
  // Line Heights - Relative to font size
  // ============================================================================

  /// Tight line height - 1.2 (for headings)
  static const double lineHeightTight = 1.2;

  /// Normal line height - 1.5 (for body text)
  static const double lineHeightNormal = 1.5;

  /// Relaxed line height - 1.7 (for long-form content)
  static const double lineHeightRelaxed = 1.7;

  // ============================================================================
  // Letter Spacing - Optical adjustments
  // ============================================================================

  /// Tight letter spacing - -0.02em
  static const double letterSpacingTight = -0.02;

  /// Normal letter spacing - 0em
  static const double letterSpacingNormal = 0.0;

  /// Wide letter spacing - 0.02em
  static const double letterSpacingWide = 0.02;

  /// Extra wide letter spacing - 0.05em (for all caps)
  static const double letterSpacingExtraWide = 0.05;

  // ============================================================================
  // Display Styles - Largest text (hero sections)
  // ============================================================================

  /// Display Large - 72px/semiBold/tight
  /// Use for: Hero headlines, splash screens
  static TextStyle displayLarge({Color? color}) => TextStyle(
    fontSize: 72.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingTight,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Display Medium - 60px/semiBold/tight
  /// Use for: Large section headers
  static TextStyle displayMedium({Color? color}) => TextStyle(
    fontSize: 60.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingTight,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Display Small - 48px/semiBold/tight
  /// Use for: Page titles, modal headers
  static TextStyle displaySmall({Color? color}) => TextStyle(
    fontSize: 48.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingTight,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Heading Styles - Section headers
  // ============================================================================

  /// Heading 1 - 40px/semiBold/tight
  /// Use for: Main page headings
  static TextStyle heading1({Color? color}) => TextStyle(
    fontSize: 40.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingTight,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Heading 2 - 32px/semiBold/tight
  /// Use for: Section headings
  static TextStyle heading2({Color? color}) => TextStyle(
    fontSize: 32.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingTight,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Heading 3 - 28px/semiBold/normal
  /// Use for: Subsection headings
  static TextStyle heading3({Color? color}) => TextStyle(
    fontSize: 28.0,
    fontWeight: semiBold,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Heading 4 - 24px/semiBold/normal
  /// Use for: Card titles, dialog titles
  static TextStyle heading4({Color? color}) => TextStyle(
    fontSize: 24.0,
    fontWeight: semiBold,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Heading 5 - 20px/semiBold/normal
  /// Use for: List headers, form sections
  static TextStyle heading5({Color? color}) => TextStyle(
    fontSize: 20.0,
    fontWeight: semiBold,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Heading 6 - 18px/semiBold/normal
  /// Use for: Small section headers
  static TextStyle heading6({Color? color}) => TextStyle(
    fontSize: 18.0,
    fontWeight: semiBold,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Title Styles - Inline emphasis
  // ============================================================================

  /// Title Large - 22px/medium/normal
  /// Use for: List item titles, prominent labels
  static TextStyle titleLarge({Color? color}) => TextStyle(
    fontSize: 22.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Title Medium - 18px/medium/normal
  /// Use for: Card subtitles, secondary headers
  static TextStyle titleMedium({Color? color}) => TextStyle(
    fontSize: 18.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Title Small - 16px/medium/normal
  /// Use for: List subtitles, form labels
  static TextStyle titleSmall({Color? color}) => TextStyle(
    fontSize: 16.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Body Styles - Content text
  // ============================================================================

  /// Body Large - 18px/regular/relaxed
  /// Use for: Long-form content, articles
  static TextStyle bodyLarge({Color? color}) => TextStyle(
    fontSize: 18.0,
    fontWeight: regular,
    height: lineHeightRelaxed,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Body Medium - 16px/regular/normal
  /// Use for: Standard body text, descriptions
  static TextStyle bodyMedium({Color? color}) => TextStyle(
    fontSize: 16.0,
    fontWeight: regular,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Body Small - 14px/regular/normal
  /// Use for: Secondary text, captions
  static TextStyle bodySmall({Color? color}) => TextStyle(
    fontSize: 14.0,
    fontWeight: regular,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Label Styles - UI elements
  // ============================================================================

  /// Label Large - 16px/medium/normal
  /// Use for: Button text, tab labels
  static TextStyle labelLarge({Color? color}) => TextStyle(
    fontSize: 16.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingWide,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Label Medium - 14px/medium/normal
  /// Use for: Input labels, chip labels
  static TextStyle labelMedium({Color? color}) => TextStyle(
    fontSize: 14.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingWide,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Label Small - 12px/medium/normal
  /// Use for: Badges, tags, metadata
  static TextStyle labelSmall({Color? color}) => TextStyle(
    fontSize: 12.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingWide,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Caption Styles - Smallest text
  // ============================================================================

  /// Caption - 12px/regular/normal
  /// Use for: Helper text, footnotes
  static TextStyle caption({Color? color}) => TextStyle(
    fontSize: 12.0,
    fontWeight: regular,
    height: lineHeightNormal,
    letterSpacing: letterSpacingWide,
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Overline - 11px/medium/extraWide (ALL CAPS)
  /// Use for: Category labels, section markers
  static TextStyle overline({Color? color}) => TextStyle(
    fontSize: 11.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingExtraWide,
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Monospace Styles - Code and data
  // ============================================================================

  /// Code Large - 16px/regular/relaxed
  /// Use for: Code blocks, terminal output
  static TextStyle codeLarge({Color? color}) => TextStyle(
    fontSize: 16.0,
    fontWeight: regular,
    height: lineHeightRelaxed,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyMono,
  );

  /// Code Medium - 14px/regular/relaxed
  /// Use for: Inline code, addresses, hashes
  static TextStyle codeMedium({Color? color}) => TextStyle(
    fontSize: 14.0,
    fontWeight: regular,
    height: lineHeightRelaxed,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyMono,
  );

  /// Code Small - 12px/regular/relaxed
  /// Use for: Small data displays, tooltips
  static TextStyle codeSmall({Color? color}) => TextStyle(
    fontSize: 12.0,
    fontWeight: regular,
    height: lineHeightRelaxed,
    letterSpacing: letterSpacingNormal,
    color: color,
    fontFamily: fontFamilyMono,
  );

  // ============================================================================
  // Number Styles - Tabular figures
  // ============================================================================

  /// Number Large - 24px/semiBold/tight (tabular)
  /// Use for: Balance displays, large amounts
  static TextStyle numberLarge({Color? color}) => TextStyle(
    fontSize: 24.0,
    fontWeight: semiBold,
    height: lineHeightTight,
    letterSpacing: letterSpacingNormal,
    fontFeatures: [FontFeature.tabularFigures()],
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Number Medium - 18px/medium/normal (tabular)
  /// Use for: Transaction amounts, prices
  static TextStyle numberMedium({Color? color}) => TextStyle(
    fontSize: 18.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    fontFeatures: [FontFeature.tabularFigures()],
    color: color,
    fontFamily: fontFamilyUI,
  );

  /// Number Small - 14px/medium/normal (tabular)
  /// Use for: Small amounts, percentages
  static TextStyle numberSmall({Color? color}) => TextStyle(
    fontSize: 14.0,
    fontWeight: medium,
    height: lineHeightNormal,
    letterSpacing: letterSpacingNormal,
    fontFeatures: [FontFeature.tabularFigures()],
    color: color,
    fontFamily: fontFamilyUI,
  );

  // ============================================================================
  // Utility Methods
  // ============================================================================

  /// Apply color to any text style
  static TextStyle withColor(TextStyle style, Color color) {
    return style.copyWith(color: color);
  }

  /// Make text bold
  static TextStyle makeBold(TextStyle style) {
    return style.copyWith(fontWeight: bold);
  }

  /// Make text italic
  static TextStyle makeItalic(TextStyle style) {
    return style.copyWith(fontStyle: FontStyle.italic);
  }

  /// Add underline
  static TextStyle withUnderline(TextStyle style) {
    return style.copyWith(decoration: TextDecoration.underline);
  }

  /// Add strikethrough
  static TextStyle withStrikethrough(TextStyle style) {
    return style.copyWith(decoration: TextDecoration.lineThrough);
  }

  /// Scale font size
  static TextStyle scale(TextStyle style, double factor) {
    return style.copyWith(fontSize: (style.fontSize ?? 16.0) * factor);
  }
}
