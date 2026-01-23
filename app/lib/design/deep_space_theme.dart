/// Deep Space Theme — Visual Language Specification
///
/// A premium dark-first theme inspired by deep space aesthetics:
/// - Pitch-black backgrounds with subtle blue undertones
/// - Cyan-to-blue Gradient A for primary CTAs and hero headers
/// - 60/120fps optimized animations
/// - WCAG AA accessibility compliance
/// - Desktop: Custom titlebar, Vibrancy/Mica
/// - Mobile: Haptic feedback, high-DPI icons
library;

import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'tokens/colors.dart';
import 'tokens/spacing.dart';
import 'tokens/typography.dart';
export 'tokens/colors.dart' show AppColors;

// =============================================================================
// Deep Space Color Palette
// =============================================================================

/// Deep Space color system - Darker than default for true OLED black
class DeepSpaceColors {
  DeepSpaceColors._();

  // ---------------------------------------------------------------------------
  // Core Backgrounds — Progressive depth with blue undertones
  // ---------------------------------------------------------------------------
  
  /// Void Black — Absolute base (#050508)
  static const Color void_ = Color(0xFF0B0F14);

  /// Alias for legacy naming used in some UI widgets.
  static const Color voidBlack = void_;
  
  /// Deep Space — Primary background (#08090D)
  static const Color deepSpace = Color(0xFF0B0F14);
  
  /// Nebula — Surface level 1 (#0C0E14)
  static const Color nebula = Color(0xFF111722);
  
  /// Cosmos — Surface level 2 (#10121A)
  static const Color cosmos = Color(0xFF171F2B);

  /// Alias for elevated surface color used by some components.
  static const Color surfaceElevated = cosmos;
  
  /// Stardust — Surface level 3 (#161A24)
  static const Color stardust = Color(0xFF223044);
  
  /// Overlay with blur
  static const Color overlay = Color(0xCC000000);

  // ---------------------------------------------------------------------------
  // Gradient A — Primary CTA & Hero (Cyan → Blue)
  // ---------------------------------------------------------------------------
  
  /// Gradient A Start — Electric Cyan (#00F5FF)
  static const Color gradientAStart = Color(0xFF2B6FF7);
  
  /// Gradient A Mid — Bright Cyan (#00D4E8)
  static const Color gradientAMid = Color(0xFF245FD6);
  
  /// Gradient A End — Deep Blue (#5B8DEF)
  static const Color gradientAEnd = Color(0xFF1E55C7);
  
  /// Gradient A Colors list
  static const List<Color> gradientA = [gradientAStart, gradientAMid, gradientAEnd];
  
  /// Gradient A linear gradient (for buttons/headers)
  static LinearGradient get gradientALinear => const LinearGradient(
    colors: [gradientAStart, gradientAEnd],
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
  );
  
  /// Gradient A with glow effect
  static LinearGradient get gradientAGlow => LinearGradient(
    colors: [
      gradientAStart.withValues(alpha: 0.8),
      gradientAEnd.withValues(alpha: 0.6),
    ],
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
  );

  // ---------------------------------------------------------------------------
  // Gradient B — Secondary accents (Purple → Pink)
  // ---------------------------------------------------------------------------
  
  static const Color gradientBStart = Color(0xFF1FA971);
  static const Color gradientBEnd = Color(0xFF178A5E);
  static const List<Color> gradientB = [gradientBStart, gradientBEnd];

  // ---------------------------------------------------------------------------
  // Text Hierarchy
  // ---------------------------------------------------------------------------
  
  /// Primary text — Maximum contrast (#FFFFFF @ 95%)
  static const Color textPrimary = Color(0xFFF7F9FC);
  
  /// Secondary text — Medium emphasis (#FFFFFF @ 70%)
  static const Color textSecondary = Color(0xFFD0D6E0);
  
  /// Tertiary text — Low emphasis (#FFFFFF @ 50%)
  static const Color textTertiary = Color(0xFF9BA6B5);

  /// Alias for muted text.
  static const Color textMuted = textTertiary;
  
  /// Disabled text (#FFFFFF @ 30%)
  static const Color textDisabled = Color(0xFF6D7888);
  
  /// Text on gradient backgrounds
  static const Color textOnGradient = Color(0xFFFFFFFF);

  // ---------------------------------------------------------------------------
  // Semantic Colors
  // ---------------------------------------------------------------------------
  
  static const Color success = Color(0xFF16A34A);
  static const Color successBg = Color(0x1A16A34A);
  static const Color warning = Color(0xFFF59E0B);
  static const Color warningBg = Color(0x1AF59E0B);
  static const Color error = Color(0xFFE24A4A);
  static const Color errorBg = Color(0x1AE24A4A);
  static const Color info = Color(0xFF2B6FF7);
  static const Color infoBg = Color(0x1A2B6FF7);

  // ---------------------------------------------------------------------------
  // Interactive States
  // ---------------------------------------------------------------------------
  
  /// Focus ring — Matches Gradient A start
  static const Color focus = Color(0xFF2B6FF7);
  static const Color focusSubtle = Color(0x402B6FF7);
  
  /// Hover state overlay
  static const Color hover = Color(0x14FFFFFF);
  
  /// Pressed state overlay
  static const Color pressed = Color(0x1FFFFFFF);
  
  /// Selected background
  static const Color selected = Color(0x1A2B6FF7);
  static const Color selectedBorder = Color(0x802B6FF7);

  // ---------------------------------------------------------------------------
  // Borders & Dividers
  // ---------------------------------------------------------------------------
  
  static const Color borderDefault = Color(0x14FFFFFF);
  static const Color borderSubtle = Color(0x0AFFFFFF);
  static const Color borderStrong = Color(0x24FFFFFF);
  static const Color divider = Color(0x14FFFFFF);

  // ---------------------------------------------------------------------------
  // Shadows & Glow
  // ---------------------------------------------------------------------------
  
  static const Color shadow = Color(0x66000000);
  static const Color shadowDeep = Color(0x99000000);
  static const Color glow = Color(0x402B6FF7);
}

// =============================================================================
// Animation Constants (60/120fps optimized)
// =============================================================================

/// Animation durations optimized for 60fps (16.67ms frames)
class DeepSpaceDurations {
  DeepSpaceDurations._();
  
  /// Micro interaction — 2 frames @ 120fps (16ms)
  static const Duration micro = Duration(milliseconds: 80);
  
  /// Fast — 6 frames @ 60fps (100ms)
  static const Duration fast = Duration(milliseconds: 120);
  
  /// Normal — 10 frames @ 60fps (167ms)
  static const Duration normal = Duration(milliseconds: 160);
  
  /// Medium — 18 frames @ 60fps (300ms)
  static const Duration medium = Duration(milliseconds: 220);
  
  /// Slow — 25 frames @ 60fps (417ms)
  static const Duration slow = Duration(milliseconds: 300);
  
  /// Page transition — 20 frames @ 60fps (333ms)
  static const Duration page = Duration(milliseconds: 220);
  
  /// Hero animation — 30 frames @ 60fps (500ms)
  static const Duration hero = Duration(milliseconds: 240);
}

/// Animation curves tuned for premium feel
class DeepSpaceCurves {
  DeepSpaceCurves._();
  
  /// Standard easing — Most UI interactions
  static const Curve standard = Curves.easeOutCubic;
  
  /// Emphasized — Important transitions
  static const Curve emphasized = Curves.easeInOutCubic;
  
  /// Decelerate — Elements entering view
  static const Curve enter = Curves.easeOutCubic;
  
  /// Accelerate — Elements exiting view
  static const Curve exit = Curves.easeInCubic;
  
  /// Spring — Playful micro-interactions
  static const Curve spring = Curves.elasticOut;
  
  /// Smooth — Continuous animations
  static const Curve smooth = Cubic(0.4, 0.0, 0.2, 1.0);
  
  /// Snap — Instant feedback
  static const Curve snap = Cubic(0.0, 0.0, 0.2, 1.0);
}

// =============================================================================
// Accessibility Configuration
// =============================================================================

/// WCAG AA compliant accessibility settings
class DeepSpaceA11y {
  DeepSpaceA11y._();
  
  /// Minimum touch target size (48dp)
  static const double minTouchTarget = 48.0;
  
  /// Focus ring width (2dp minimum)
  static const double focusRingWidth = 2.0;
  
  /// Focus ring offset
  static const double focusRingOffset = 2.0;
  
  /// Minimum contrast ratio for normal text (4.5:1)
  static const double contrastNormal = 4.5;
  
  /// Minimum contrast ratio for large text (3:1)
  static const double contrastLarge = 3.0;
  
  /// Reduced motion duration multiplier
  static const double reducedMotionMultiplier = 0.0;
  
  /// High contrast border width
  static const double highContrastBorderWidth = 2.0;
  
  /// Check if reduced motion is preferred
  static bool get prefersReducedMotion {
    return WidgetsBinding.instance.platformDispatcher.accessibilityFeatures.reduceMotion;
  }
  
  /// Get duration respecting reduced motion preference
  static Duration getMotionDuration(Duration duration) {
    if (prefersReducedMotion) {
      return Duration.zero;
    }
    return duration;
  }
}

// =============================================================================
// Platform-Specific Effects
// =============================================================================

/// Desktop-specific visual effects
class DeepSpaceDesktop {
  DeepSpaceDesktop._();
  
  /// Custom titlebar height
  static const double titlebarHeight = 32.0;
  
  /// Window minimum size
  static const Size minWindowSize = Size(960, 600);
  
  /// Sidebar width
  static const double sidebarWidth = 240.0;
  
  /// Nav rail width (collapsed)
  static const double navRailWidth = 72.0;
  
  /// Check if platform supports vibrancy/Mica
  static bool get supportsVibrancy {
    if (Platform.isMacOS) return true;
    if (Platform.isWindows) return true; // Windows 11 Mica
    return false;
  }
  
  /// Get vibrancy/Mica color (platform-specific)
  static Color get vibrancyColor {
    if (Platform.isMacOS) {
      return DeepSpaceColors.deepSpace.withValues(alpha: 0.7);
    }
    if (Platform.isWindows) {
      return DeepSpaceColors.deepSpace.withValues(alpha: 0.85);
    }
    return DeepSpaceColors.deepSpace;
  }
}

/// Haptic feedback patterns
class DeepSpaceHaptics {
  DeepSpaceHaptics._();
  
  /// Light impact — Button tap
  static void lightImpact() {
    HapticFeedback.lightImpact();
  }
  
  /// Medium impact — Toggle, selection
  static void mediumImpact() {
    HapticFeedback.mediumImpact();
  }
  
  /// Heavy impact — Destructive action confirmation
  static void heavyImpact() {
    HapticFeedback.heavyImpact();
  }
  
  /// Selection click — List selection
  static void selectionClick() {
    HapticFeedback.selectionClick();
  }
  
  /// Vibrate — Error, alert
  static void vibrate() {
    HapticFeedback.vibrate();
  }
}

// =============================================================================
// Theme Builder
// =============================================================================

/// Deep Space Theme configuration
class DeepSpaceTheme {
  DeepSpaceTheme._();

  /// Build the Dark theme
  static ThemeData dark({bool highContrast = false}) {
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      
      // Color Scheme
      colorScheme: ColorScheme.dark(
        brightness: Brightness.dark,
        primary: DeepSpaceColors.gradientAStart,
        onPrimary: DeepSpaceColors.textOnGradient,
        primaryContainer: DeepSpaceColors.cosmos,
        onPrimaryContainer: DeepSpaceColors.textPrimary,
        secondary: DeepSpaceColors.gradientBStart,
        onSecondary: DeepSpaceColors.textOnGradient,
        secondaryContainer: DeepSpaceColors.nebula,
        onSecondaryContainer: DeepSpaceColors.textPrimary,
        tertiary: DeepSpaceColors.info,
        onTertiary: DeepSpaceColors.textOnGradient,
        error: DeepSpaceColors.error,
        onError: DeepSpaceColors.textPrimary,
        errorContainer: DeepSpaceColors.errorBg,
        onErrorContainer: DeepSpaceColors.error,
        surface: DeepSpaceColors.nebula,
        onSurface: DeepSpaceColors.textPrimary,
        surfaceContainerHighest: DeepSpaceColors.stardust,
        onSurfaceVariant: DeepSpaceColors.textSecondary,
        outline: highContrast 
            ? DeepSpaceColors.borderStrong 
            : DeepSpaceColors.borderDefault,
        outlineVariant: DeepSpaceColors.borderSubtle,
        shadow: DeepSpaceColors.shadow,
        scrim: DeepSpaceColors.overlay,
        inverseSurface: DeepSpaceColors.textPrimary,
        onInverseSurface: DeepSpaceColors.deepSpace,
        inversePrimary: DeepSpaceColors.deepSpace,
      ),

      scaffoldBackgroundColor: DeepSpaceColors.deepSpace,
      canvasColor: DeepSpaceColors.deepSpace,
      cardColor: DeepSpaceColors.nebula,
      dividerColor: DeepSpaceColors.divider,
      focusColor: DeepSpaceColors.focusSubtle,
      hoverColor: DeepSpaceColors.hover,
      highlightColor: DeepSpaceColors.pressed,
      splashColor: DeepSpaceColors.pressed,
      disabledColor: DeepSpaceColors.textDisabled,

      // Typography
      fontFamily: PTypography.fontFamilyUI,
      
      textTheme: TextTheme(
        displayLarge: PTypography.displayLarge(color: DeepSpaceColors.textPrimary),
        displayMedium: PTypography.displayMedium(color: DeepSpaceColors.textPrimary),
        displaySmall: PTypography.displaySmall(color: DeepSpaceColors.textPrimary),
        headlineLarge: PTypography.heading1(color: DeepSpaceColors.textPrimary),
        headlineMedium: PTypography.heading2(color: DeepSpaceColors.textPrimary),
        headlineSmall: PTypography.heading3(color: DeepSpaceColors.textPrimary),
        titleLarge: PTypography.titleLarge(color: DeepSpaceColors.textPrimary),
        titleMedium: PTypography.titleMedium(color: DeepSpaceColors.textPrimary),
        titleSmall: PTypography.titleSmall(color: DeepSpaceColors.textSecondary),
        bodyLarge: PTypography.bodyLarge(color: DeepSpaceColors.textSecondary),
        bodyMedium: PTypography.bodyMedium(color: DeepSpaceColors.textSecondary),
        bodySmall: PTypography.bodySmall(color: DeepSpaceColors.textTertiary),
        labelLarge: PTypography.labelLarge(color: DeepSpaceColors.textPrimary),
        labelMedium: PTypography.labelMedium(color: DeepSpaceColors.textSecondary),
        labelSmall: PTypography.labelSmall(color: DeepSpaceColors.textTertiary),
      ),

      // AppBar Theme
      appBarTheme: AppBarTheme(
        elevation: 0,
        scrolledUnderElevation: 0,
        centerTitle: false,
        backgroundColor: DeepSpaceColors.deepSpace,
        foregroundColor: DeepSpaceColors.textPrimary,
        surfaceTintColor: Colors.transparent,
        titleTextStyle: PTypography.heading5(color: DeepSpaceColors.textPrimary),
        toolbarHeight: 64.0,
        systemOverlayStyle: const SystemUiOverlayStyle(
          statusBarColor: Colors.transparent,
          statusBarBrightness: Brightness.dark,
          statusBarIconBrightness: Brightness.light,
          systemNavigationBarColor: DeepSpaceColors.deepSpace,
          systemNavigationBarIconBrightness: Brightness.light,
        ),
        iconTheme: const IconThemeData(
          color: DeepSpaceColors.textPrimary,
          size: 24.0,
        ),
      ),

      // Elevated Button (Primary CTA with Gradient A)
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          elevation: 0,
          padding: const EdgeInsets.symmetric(horizontal: 24.0, vertical: 14.0),
          minimumSize: const Size(DeepSpaceA11y.minTouchTarget, DeepSpaceA11y.minTouchTarget),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12.0),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: DeepSpaceColors.textOnGradient,
          backgroundColor: DeepSpaceColors.gradientAStart,
          disabledForegroundColor: DeepSpaceColors.textDisabled,
          disabledBackgroundColor: DeepSpaceColors.nebula,
        ),
      ),

      // Outlined Button
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          padding: const EdgeInsets.symmetric(horizontal: 24.0, vertical: 14.0),
          minimumSize: const Size(DeepSpaceA11y.minTouchTarget, DeepSpaceA11y.minTouchTarget),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12.0),
          ),
          side: BorderSide(
            color: highContrast
                ? DeepSpaceColors.borderStrong
                : DeepSpaceColors.borderDefault,
            width: highContrast ? 2.0 : 1.5,
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: DeepSpaceColors.textPrimary,
        ),
      ),

      // Text Button
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          padding: const EdgeInsets.symmetric(horizontal: 16.0, vertical: 12.0),
          minimumSize: const Size(DeepSpaceA11y.minTouchTarget, DeepSpaceA11y.minTouchTarget),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8.0),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: DeepSpaceColors.gradientAStart,
        ),
      ),

      // Input Decoration Theme
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: DeepSpaceColors.nebula,
        contentPadding: const EdgeInsets.symmetric(horizontal: 16.0, vertical: 14.0),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: DeepSpaceColors.borderDefault,
            width: 1.0,
          ),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: DeepSpaceColors.borderDefault,
            width: 1.0,
          ),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: DeepSpaceColors.focus,
            width: DeepSpaceA11y.focusRingWidth,
          ),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: DeepSpaceColors.error,
            width: 1.0,
          ),
        ),
        focusedErrorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: DeepSpaceColors.error,
            width: DeepSpaceA11y.focusRingWidth,
          ),
        ),
        labelStyle: PTypography.labelMedium(color: DeepSpaceColors.textSecondary),
        floatingLabelStyle: PTypography.labelMedium(color: DeepSpaceColors.focus),
        hintStyle: PTypography.bodyMedium(color: DeepSpaceColors.textTertiary),
        errorStyle: PTypography.labelSmall(color: DeepSpaceColors.error),
        helperStyle: PTypography.caption(color: DeepSpaceColors.textTertiary),
        prefixIconColor: DeepSpaceColors.textSecondary,
        suffixIconColor: DeepSpaceColors.textSecondary,
      ),

      // Card Theme
      cardTheme: CardThemeData(
        elevation: 0,
        color: DeepSpaceColors.nebula,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusCard),
          side: BorderSide(
            color: DeepSpaceColors.borderSubtle,
            width: 1.0,
          ),
        ),
        margin: const EdgeInsets.all(8.0),
      ),

      // Dialog Theme
      dialogTheme: DialogThemeData(
        elevation: 16,
        backgroundColor: DeepSpaceColors.cosmos,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(24.0),
        ),
        titleTextStyle: PTypography.heading4(color: DeepSpaceColors.textPrimary),
        contentTextStyle: PTypography.bodyMedium(color: DeepSpaceColors.textSecondary),
      ),

      // Bottom Sheet Theme
      bottomSheetTheme: BottomSheetThemeData(
        elevation: 0,
        backgroundColor: DeepSpaceColors.cosmos,
        surfaceTintColor: Colors.transparent,
        modalBackgroundColor: DeepSpaceColors.cosmos,
        shape: const RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(top: Radius.circular(24.0)),
        ),
        dragHandleColor: DeepSpaceColors.borderStrong,
        dragHandleSize: const Size(32.0, 4.0),
      ),

      // Progress Indicator Theme
      progressIndicatorTheme: const ProgressIndicatorThemeData(
        color: DeepSpaceColors.gradientAStart,
        linearTrackColor: DeepSpaceColors.nebula,
        circularTrackColor: DeepSpaceColors.nebula,
      ),

      // Tooltip Theme
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: DeepSpaceColors.stardust,
          borderRadius: BorderRadius.circular(8.0),
          border: Border.all(color: DeepSpaceColors.borderDefault),
          boxShadow: [
            BoxShadow(
              color: DeepSpaceColors.shadowDeep,
              blurRadius: 12.0,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        textStyle: PTypography.labelSmall(color: DeepSpaceColors.textPrimary),
        padding: const EdgeInsets.symmetric(horizontal: 12.0, vertical: 8.0),
        waitDuration: const Duration(milliseconds: 500),
      ),

      // Snackbar Theme
      snackBarTheme: SnackBarThemeData(
        elevation: 8,
        backgroundColor: DeepSpaceColors.stardust,
        contentTextStyle: PTypography.bodyMedium(color: DeepSpaceColors.textPrimary),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(12.0),
        ),
        behavior: SnackBarBehavior.floating,
      ),

      // Switch Theme
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return DeepSpaceColors.textOnGradient;
          }
          return DeepSpaceColors.textSecondary;
        }),
        trackColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return DeepSpaceColors.gradientAStart;
          }
          return DeepSpaceColors.nebula;
        }),
        trackOutlineColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return Colors.transparent;
          }
          return DeepSpaceColors.borderDefault;
        }),
      ),

      // Checkbox Theme
      checkboxTheme: CheckboxThemeData(
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return DeepSpaceColors.gradientAStart;
          }
          return Colors.transparent;
        }),
        checkColor: WidgetStateProperty.all(DeepSpaceColors.textOnGradient),
        side: BorderSide(
          color: DeepSpaceColors.borderDefault,
          width: 1.5,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(4.0),
        ),
      ),

      // Divider Theme
      dividerTheme: const DividerThemeData(
        color: DeepSpaceColors.divider,
        thickness: 1.0,
        space: 1.0,
      ),

      // List Tile Theme (accessible)
      listTileTheme: ListTileThemeData(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16.0, vertical: 12.0),
        minVerticalPadding: 8.0,
        minLeadingWidth: DeepSpaceA11y.minTouchTarget,
        titleTextStyle: PTypography.titleSmall(color: DeepSpaceColors.textPrimary),
        subtitleTextStyle: PTypography.bodySmall(color: DeepSpaceColors.textSecondary),
        iconColor: DeepSpaceColors.textSecondary,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(12.0),
        ),
      ),

      // Chip Theme
      chipTheme: ChipThemeData(
        backgroundColor: DeepSpaceColors.nebula,
        selectedColor: DeepSpaceColors.selected,
        disabledColor: DeepSpaceColors.nebula,
        labelStyle: PTypography.labelSmall(color: DeepSpaceColors.textPrimary),
        padding: const EdgeInsets.symmetric(horizontal: 12.0, vertical: 6.0),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(8.0),
          side: BorderSide(color: DeepSpaceColors.borderDefault),
        ),
      ),

      // Icon Button Theme (accessible)
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          foregroundColor: DeepSpaceColors.textPrimary,
          highlightColor: DeepSpaceColors.hover,
          minimumSize: const Size(DeepSpaceA11y.minTouchTarget, DeepSpaceA11y.minTouchTarget),
        ),
      ),

      // Floating Action Button Theme
      floatingActionButtonTheme: FloatingActionButtonThemeData(
        backgroundColor: DeepSpaceColors.gradientAStart,
        foregroundColor: DeepSpaceColors.textOnGradient,
        elevation: 4,
        highlightElevation: 8,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(16.0),
        ),
      ),
    );
  }
}

// =============================================================================
// Convenience Aliases
// =============================================================================

/// Alias for typography
class AppTypography {
  AppTypography._();
  
  static TextStyle get h1 => PTypography.heading1(color: AppColors.textPrimary);
  static TextStyle get h2 => PTypography.heading2(color: AppColors.textPrimary);
  static TextStyle get h3 => PTypography.heading3(color: AppColors.textPrimary);
  static TextStyle get h4 => PTypography.heading4(color: AppColors.textPrimary);
  static TextStyle get h5 => PTypography.heading5(color: AppColors.textPrimary);
  static TextStyle get bodyLarge => PTypography.bodyLarge(color: AppColors.textSecondary);
  static TextStyle get bodyMedium => PTypography.bodyMedium(color: AppColors.textSecondary);
  static TextStyle get body => bodyMedium;
  static TextStyle get bodySmall => PTypography.bodySmall(color: AppColors.textTertiary);
  static TextStyle get bodyBold =>
      PTypography.bodyMedium(color: AppColors.textPrimary)
          .copyWith(fontWeight: FontWeight.w600);
  static TextStyle get caption => PTypography.caption(color: AppColors.textTertiary);
  static TextStyle get code => PTypography.codeMedium(color: AppColors.textPrimary);
  static TextStyle get mono => PTypography.codeMedium(color: AppColors.textPrimary);
  static TextStyle get labelLarge => PTypography.labelLarge(color: AppColors.textSecondary);
  static TextStyle get labelMedium => PTypography.labelMedium(color: AppColors.textSecondary);
  static TextStyle get labelSmall => PTypography.labelSmall(color: AppColors.textTertiary);
}

/// Alias for spacing
class AppSpacing {
  AppSpacing._();
  
  static const double xxs = PSpacing.xxs;
  static const double xs = PSpacing.xs;
  static const double sm = PSpacing.sm;
  static const double md = PSpacing.md;
  static const double lg = PSpacing.lg;
  static const double xl = PSpacing.xl;
  static const double xxl = PSpacing.xxl;

  static double responsiveGutter(double screenWidth) =>
      PSpacing.responsiveGutter(screenWidth);

  static double responsivePadding(double screenWidth) =>
      PSpacing.responsivePadding(screenWidth);

  static EdgeInsets screenPadding(
    double screenWidth, {
    double vertical = PSpacing.lg,
  }) =>
      PSpacing.screenPadding(screenWidth, vertical: vertical);

  static bool isMobile(double screenWidth) => PSpacing.isMobile(screenWidth);

  static bool isTablet(double screenWidth) => PSpacing.isTablet(screenWidth);

  static bool isDesktop(double screenWidth) => PSpacing.isDesktop(screenWidth);
}

/// App theme alias
class AppTheme {
  AppTheme._();
  
  static ThemeData get darkTheme => DeepSpaceTheme.dark();
  static ThemeData get darkThemeHighContrast => DeepSpaceTheme.dark(highContrast: true);
}

