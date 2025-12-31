import 'dart:ui';

import 'package:flutter/material.dart';

/// Design tokens for colors - Dark-first palette
///
/// This palette is designed for a premium dark UI with high contrast
/// and accessibility. All colors are optimized for OLED displays and
/// dark environments.
class PColors {
  PColors._();

  // ============================================================================
  // Background Colors - Progressive depth system
  // ============================================================================
  
  /// Base background - Deepest level (#0B0F14)
  /// Use for: App background, full-screen views
  static const Color backgroundBase = Color(0xFF0B0F14);
  
  /// Surface background - First elevation (#111722)
  /// Use for: Cards, sheets, panels
  static const Color backgroundSurface = Color(0xFF111722);
  
  /// Elevated background - Second elevation (#171F2B)
  /// Use for: Modals, dialogs, floating elements
  static const Color backgroundElevated = Color(0xFF171F2B);

  /// Panel background - Third elevation (#223044)
  /// Use for: Panels, emphasis surfaces
  static const Color backgroundPanel = Color(0xFF223044);
  
  /// Overlay background - Semi-transparent
  /// Use for: Overlays, scrims, modal backgrounds
  static const Color backgroundOverlay = Color(0xCC000000);
  
  // ============================================================================
  // Accent Gradients - Primary brand gradients
  // ============================================================================
  
  /// Gradient A Start - Primary (#2B6FF7)
  static const Color gradientAStart = Color(0xFF2B6FF7);
  
  /// Gradient A End - Primary Deep (#1E55C7)
  static const Color gradientAEnd = Color(0xFF1E55C7);
  
  /// Gradient B Start - Secondary (#1FA971)
  static const Color gradientBStart = Color(0xFF1FA971);
  
  /// Gradient B End - Secondary Deep
  static const Color gradientBEnd = Color(0xFF178A5E);

  /// Highlight - Attention without alarm (#F1B24A)
  static const Color highlight = Color(0xFFF1B24A);
  
  // ============================================================================
  // Text Colors - Hierarchical text system
  // ============================================================================
  
  /// Primary text - Highest emphasis (#F7F9FC)
  /// Use for: Headings, important content
  static const Color textPrimary = Color(0xFFF7F9FC);
  
  /// Secondary text - Medium emphasis (#D0D6E0)
  /// Use for: Body text, descriptions
  static const Color textSecondary = Color(0xFFD0D6E0);
  
  /// Tertiary text - Low emphasis (#9BA6B5)
  /// Use for: Labels, placeholders, hints
  static const Color textTertiary = Color(0xFF9BA6B5);
  
  /// Disabled text - Minimal emphasis (#6D7888)
  /// Use for: Disabled states
  static const Color textDisabled = Color(0xFF6D7888);
  
  /// On-accent text - Text on colored backgrounds
  /// Use for: Text on gradient buttons, badges
  static const Color textOnAccent = Color(0xFFFFFFFF);
  
  // ============================================================================
  // Semantic Colors - Status and feedback
  // ============================================================================
  
  /// Success color - Optimized for dark UI (#16A34A)
  static const Color success = Color(0xFF16A34A);
  
  /// Success background - Subtle success background
  static const Color successBackground = Color(0x1A16A34A);
  
  /// Success border - Success border/outline
  static const Color successBorder = Color(0x4016A34A);
  
  /// Warning color - Optimized for dark UI (#F59E0B)
  static const Color warning = Color(0xFFF59E0B);
  
  /// Warning background - Subtle warning background
  static const Color warningBackground = Color(0x1AF59E0B);
  
  /// Warning border - Warning border/outline
  static const Color warningBorder = Color(0x40F59E0B);
  
  /// Error color - Optimized for dark UI (#E24A4A)
  static const Color error = Color(0xFFE24A4A);
  
  /// Error background - Subtle error background
  static const Color errorBackground = Color(0x1AE24A4A);
  
  /// Error border - Error border/outline
  static const Color errorBorder = Color(0x40E24A4A);
  
  /// Info color - Optimized for dark UI (#2B6FF7)
  static const Color info = Color(0xFF2B6FF7);
  
  /// Info background - Subtle info background
  static const Color infoBackground = Color(0x1A2B6FF7);
  
  /// Info border - Info border/outline
  static const Color infoBorder = Color(0x402B6FF7);
  
  // ============================================================================
  // Interactive Colors - UI interaction states
  // ============================================================================
  
  /// Focus ring - Primary for focus indicators
  static const Color focusRing = Color(0xFF2B6FF7);
  
  /// Focus ring with opacity - For subtle focus states
  static const Color focusRingSubtle = Color(0x402B6FF7);
  
  /// Hover overlay - Semi-transparent white for hover states
  static const Color hoverOverlay = Color(0x0DFFFFFF);
  
  /// Pressed overlay - Semi-transparent white for pressed states
  static const Color pressedOverlay = Color(0x1AFFFFFF);
  
  /// Selected background - For selected items
  static const Color selectedBackground = Color(0x1A2B6FF7);
  
  /// Selected border - For selected item borders
  static const Color selectedBorder = Color(0x802B6FF7);
  
  // ============================================================================
  // Border Colors - Dividers and borders
  // ============================================================================
  
  /// Border default - Standard border color (rgba(255, 255, 255, 0.08))
  static const Color borderDefault = Color(0x14FFFFFF);
  
  /// Border subtle - More subtle border (rgba(255, 255, 255, 0.04))
  static const Color borderSubtle = Color(0x0AFFFFFF);
  
  /// Border strong - More prominent border (rgba(255, 255, 255, 0.14))
  static const Color borderStrong = Color(0x24FFFFFF);
  
  /// Divider - Separator line color
  static const Color divider = Color(0x14FFFFFF);
  
  // ============================================================================
  // Shadow Colors - Elevation and depth
  // ============================================================================
  
  /// Shadow color - Base shadow color
  static const Color shadow = Color(0x40000000);
  
  /// Shadow strong - Stronger shadow for higher elevation
  static const Color shadowStrong = Color(0x66000000);
  
  // ============================================================================
  // Special Colors - App-specific use cases
  // ============================================================================
  
  /// QR Code background - Optimized for QR scanning
  static const Color qrBackground = Color(0xFFFFFFFF);
  
  /// QR Code foreground
  static const Color qrForeground = Color(0xFF000000);
  
  /// Chart colors - For data visualization
  static const List<Color> chartColors = [
    Color(0xFF2B6FF7), // Primary
    Color(0xFF1E55C7), // Primary Deep
    Color(0xFF1FA971), // Secondary
    Color(0xFFF1B24A), // Highlight
    Color(0xFF16A34A), // Success
    Color(0xFFF59E0B), // Warning
  ];
  
  // ============================================================================
  // Utility Methods
  // ============================================================================
  
  /// Creates a gradient from Gradient A colors
  static const gradientA = [gradientAStart, gradientAEnd];
  
  /// Creates a gradient from Gradient B colors
  static const gradientB = [gradientBStart, gradientBEnd];
  
  /// Returns appropriate text color for the given background
  static Color textOnBackground(Color background) {
    final luminance = background.computeLuminance();
    return luminance > 0.5 ? Color(0xFF000000) : Color(0xF2FFFFFF);
  }
  
  /// Returns a color with specified opacity
  static Color withOpacity(Color color, double opacity) {
    return color.withValues(alpha: opacity);
  }
}

/// High contrast color overrides for accessibility
class PColorsHighContrast {
  PColorsHighContrast._();
  
  static const Color textPrimary = Color(0xFFFFFFFF);
  static const Color textSecondary = Color(0xFFF0F4F8);
  static const Color borderDefault = Color(0x33FFFFFF);
  static const Color borderStrong = Color(0x4DFFFFFF);
  static const Color focusRing = Color(0xFFFFFFFF);
}

/// Light theme tokens (Ivory)
class PColorsLight {
  PColorsLight._();

  // Backgrounds
  static const Color backgroundBase = Color(0xFFF5F7FA);
  static const Color backgroundSurface = Color(0xFFFFFFFF);
  static const Color backgroundElevated = Color(0xFFEDF1F6);
  static const Color backgroundPanel = Color(0xFFE6ECF2);
  static const Color backgroundOverlay = Color(0x99000000);

  // Accents
  static const Color gradientAStart = Color(0xFF2B6FF7);
  static const Color gradientAEnd = Color(0xFF1E55C7);
  static const Color gradientBStart = Color(0xFF1FA971);
  static const Color gradientBEnd = Color(0xFF178A5E);
  static const Color highlight = Color(0xFFF1B24A);

  // Text
  static const Color textPrimary = Color(0xFF0B1220);
  static const Color textSecondary = Color(0xFF2A3342);
  static const Color textTertiary = Color(0xFF5A667A);
  static const Color textDisabled = Color(0xFF7B8798);
  static const Color textOnAccent = Color(0xFFFFFFFF);

  // Semantic
  static const Color success = Color(0xFF16A34A);
  static const Color successBackground = Color(0x1A16A34A);
  static const Color successBorder = Color(0x4016A34A);
  static const Color warning = Color(0xFFF59E0B);
  static const Color warningBackground = Color(0x1AF59E0B);
  static const Color warningBorder = Color(0x40F59E0B);
  static const Color error = Color(0xFFE24A4A);
  static const Color errorBackground = Color(0x1AE24A4A);
  static const Color errorBorder = Color(0x40E24A4A);
  static const Color info = Color(0xFF2B6FF7);
  static const Color infoBackground = Color(0x1A2B6FF7);
  static const Color infoBorder = Color(0x402B6FF7);

  // Interactive
  static const Color focusRing = Color(0xFF2B6FF7);
  static const Color focusRingSubtle = Color(0x402B6FF7);
  static const Color hoverOverlay = Color(0x0A000000);
  static const Color pressedOverlay = Color(0x14000000);
  static const Color selectedBackground = Color(0x1A2B6FF7);
  static const Color selectedBorder = Color(0x802B6FF7);

  // Borders
  static const Color borderDefault = Color(0x10000000);
  static const Color borderSubtle = Color(0x08000000);
  static const Color borderStrong = Color(0x20000000);
  static const Color divider = Color(0x10000000);

  // Shadows
  static const Color shadow = Color(0x1F000000);
  static const Color shadowStrong = Color(0x33000000);

  // Special
  static const Color qrBackground = Color(0xFFFFFFFF);
  static const Color qrForeground = Color(0xFF000000);

  static const List<Color> chartColors = PColors.chartColors;

  static const gradientA = [gradientAStart, gradientAEnd];
  static const gradientB = [gradientBStart, gradientBEnd];
}

/// Theme-aware app colors (switches between dark and light palettes)
class AppColors {
  AppColors._();

  static Brightness _brightness = Brightness.dark;

  static void syncWithTheme(Brightness brightness) {
    if (_brightness == brightness) {
      return;
    }
    _brightness = brightness;
    WidgetsBinding.instance.renderViewElement?.markNeedsBuild();
  }

  static bool get _isLight => _brightness == Brightness.light;

  // Backgrounds
  static Color get backgroundBase =>
      _isLight ? PColorsLight.backgroundBase : PColors.backgroundBase;
  static Color get backgroundSurface =>
      _isLight ? PColorsLight.backgroundSurface : PColors.backgroundSurface;
  static Color get backgroundElevated =>
      _isLight ? PColorsLight.backgroundElevated : PColors.backgroundElevated;
  static Color get backgroundPanel =>
      _isLight ? PColorsLight.backgroundPanel : PColors.backgroundPanel;
  static Color get backgroundOverlay =>
      _isLight ? PColorsLight.backgroundOverlay : PColors.backgroundOverlay;

  // Accents
  static Color get gradientAStart =>
      _isLight ? PColorsLight.gradientAStart : PColors.gradientAStart;
  static Color get gradientAEnd =>
      _isLight ? PColorsLight.gradientAEnd : PColors.gradientAEnd;
  static Color get gradientBStart =>
      _isLight ? PColorsLight.gradientBStart : PColors.gradientBStart;
  static Color get gradientBEnd =>
      _isLight ? PColorsLight.gradientBEnd : PColors.gradientBEnd;
  static Color get highlight =>
      _isLight ? PColorsLight.highlight : PColors.highlight;

  // Text
  static Color get textPrimary =>
      _isLight ? PColorsLight.textPrimary : PColors.textPrimary;
  static Color get textSecondary =>
      _isLight ? PColorsLight.textSecondary : PColors.textSecondary;
  static Color get textTertiary =>
      _isLight ? PColorsLight.textTertiary : PColors.textTertiary;
  static Color get textDisabled =>
      _isLight ? PColorsLight.textDisabled : PColors.textDisabled;
  static Color get textOnAccent =>
      _isLight ? PColorsLight.textOnAccent : PColors.textOnAccent;

  // Semantic
  static Color get success =>
      _isLight ? PColorsLight.success : PColors.success;
  static Color get successBackground =>
      _isLight ? PColorsLight.successBackground : PColors.successBackground;
  static Color get successBorder =>
      _isLight ? PColorsLight.successBorder : PColors.successBorder;
  static Color get warning =>
      _isLight ? PColorsLight.warning : PColors.warning;
  static Color get warningBackground =>
      _isLight ? PColorsLight.warningBackground : PColors.warningBackground;
  static Color get warningBorder =>
      _isLight ? PColorsLight.warningBorder : PColors.warningBorder;
  static Color get error =>
      _isLight ? PColorsLight.error : PColors.error;
  static Color get errorBackground =>
      _isLight ? PColorsLight.errorBackground : PColors.errorBackground;
  static Color get errorBorder =>
      _isLight ? PColorsLight.errorBorder : PColors.errorBorder;
  static Color get info =>
      _isLight ? PColorsLight.info : PColors.info;
  static Color get infoBackground =>
      _isLight ? PColorsLight.infoBackground : PColors.infoBackground;
  static Color get infoBorder =>
      _isLight ? PColorsLight.infoBorder : PColors.infoBorder;

  // Interactive
  static Color get focusRing =>
      _isLight ? PColorsLight.focusRing : PColors.focusRing;
  static Color get focusRingSubtle =>
      _isLight ? PColorsLight.focusRingSubtle : PColors.focusRingSubtle;
  static Color get hoverOverlay =>
      _isLight ? PColorsLight.hoverOverlay : PColors.hoverOverlay;
  static Color get pressedOverlay =>
      _isLight ? PColorsLight.pressedOverlay : PColors.pressedOverlay;
  static Color get selectedBackground =>
      _isLight ? PColorsLight.selectedBackground : PColors.selectedBackground;
  static Color get selectedBorder =>
      _isLight ? PColorsLight.selectedBorder : PColors.selectedBorder;

  // Borders
  static Color get borderDefault =>
      _isLight ? PColorsLight.borderDefault : PColors.borderDefault;
  static Color get borderSubtle =>
      _isLight ? PColorsLight.borderSubtle : PColors.borderSubtle;
  static Color get borderStrong =>
      _isLight ? PColorsLight.borderStrong : PColors.borderStrong;
  static Color get divider => _isLight ? PColorsLight.divider : PColors.divider;

  // Shadows
  static Color get shadow => _isLight ? PColorsLight.shadow : PColors.shadow;
  static Color get shadowStrong =>
      _isLight ? PColorsLight.shadowStrong : PColors.shadowStrong;

  // Special
  static Color get qrBackground =>
      _isLight ? PColorsLight.qrBackground : PColors.qrBackground;
  static Color get qrForeground =>
      _isLight ? PColorsLight.qrForeground : PColors.qrForeground;

  static List<Color> get chartColors =>
      _isLight ? PColorsLight.chartColors : PColors.chartColors;

  static List<Color> get gradientA =>
      _isLight ? PColorsLight.gradientA : PColors.gradientA;
  static List<Color> get gradientB =>
      _isLight ? PColorsLight.gradientB : PColors.gradientB;

  static LinearGradient get gradientALinear => LinearGradient(
        colors: gradientA,
        begin: const Alignment(0.0, 0.0),
        end: const Alignment(1.0, 1.0),
      );

  static Color textOnBackground(Color background) {
    final luminance = background.computeLuminance();
    return luminance > 0.5 ? const Color(0xFF000000) : const Color(0xF2FFFFFF);
  }

  static Color withOpacity(Color color, double opacity) {
    return color.withValues(alpha: opacity);
  }

  // Back-compat aliases
  static Color get surface => backgroundSurface;
  static Color get surfaceElevated => backgroundElevated;
  static Color get accentPrimary => gradientAStart;
  static Color get accentSecondary => gradientBStart;
  static Color get border => borderDefault;
  static Color get focus => focusRing;
  static Color get hover => hoverOverlay;

  // Deep Space compatibility aliases
  static Color get deepSpace => backgroundBase;
  static Color get void_ => backgroundBase;
  static Color get voidBlack => backgroundBase;
  static Color get nebula => backgroundSurface;
  static Color get cosmos => backgroundElevated;
  static Color get stardust => backgroundPanel;
  static Color get overlay => backgroundOverlay;
  static Color get gradientAMid =>
      Color.lerp(gradientAStart, gradientAEnd, 0.5)!;
  static Color get textMuted => textTertiary;
  static Color get textOnGradient => textOnAccent;
  static Color get glow =>
      gradientAStart.withValues(alpha: _isLight ? 0.28 : 0.4);
  static Color get shadowDeep => shadowStrong;
  static Color get selected => selectedBackground;
}
