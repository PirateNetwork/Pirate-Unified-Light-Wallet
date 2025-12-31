import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'tokens/colors.dart';
import 'tokens/spacing.dart';
import 'tokens/typography.dart';

/// Pirate Wallet theme system
/// 
/// Builds Material ThemeData from design tokens with dark-first approach
class PTheme {
  PTheme._();

  // ============================================================================
  // Main Theme Builders
  // ============================================================================

  /// Dark theme (default) - Premium dark UI
  static ThemeData dark({bool highContrast = false}) {
    final colors = highContrast ? PColorsHighContrast :  PColors;
    
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      
      // ========================================================================
      // Color Scheme
      // ========================================================================
      colorScheme: ColorScheme.dark(
        brightness: Brightness.dark,
        primary: PColors.gradientAStart,
        onPrimary: PColors.textOnAccent,
        primaryContainer: PColors.backgroundElevated,
        onPrimaryContainer: PColors.textPrimary,
        secondary: PColors.gradientBStart,
        onSecondary: PColors.textOnAccent,
        secondaryContainer: PColors.backgroundSurface,
        onSecondaryContainer: PColors.textPrimary,
        tertiary: PColors.info,
        onTertiary: PColors.textOnAccent,
        error: PColors.error,
        onError: PColors.textPrimary,
        errorContainer: PColors.errorBackground,
        onErrorContainer: PColors.error,
        surface: PColors.backgroundSurface,
        onSurface: PColors.textPrimary,
        surfaceContainerHighest: PColors.backgroundElevated,
        onSurfaceVariant: PColors.textSecondary,
        outline: highContrast
            ? PColorsHighContrast.borderDefault
            : PColors.borderDefault,
        outlineVariant: PColors.borderSubtle,
        shadow: PColors.shadow,
        scrim: PColors.backgroundOverlay,
        inverseSurface: PColors.textPrimary,
        onInverseSurface: PColors.backgroundBase,
        inversePrimary: PColors.backgroundBase,
      ),

      scaffoldBackgroundColor: PColors.backgroundBase,
      canvasColor: PColors.backgroundBase,
      cardColor: PColors.backgroundSurface,
      dividerColor: PColors.divider,
      focusColor: PColors.focusRingSubtle,
      hoverColor: PColors.hoverOverlay,
      highlightColor: PColors.pressedOverlay,
      splashColor: PColors.pressedOverlay,
      disabledColor: PColors.textDisabled,
      
      // ========================================================================
      // Typography
      // ========================================================================
      fontFamily: PTypography.fontFamilyUI,
      
      textTheme: TextTheme(
        displayLarge: PTypography.displayLarge(color: PColors.textPrimary),
        displayMedium: PTypography.displayMedium(color: PColors.textPrimary),
        displaySmall: PTypography.displaySmall(color: PColors.textPrimary),
        headlineLarge: PTypography.heading1(color: PColors.textPrimary),
        headlineMedium: PTypography.heading2(color: PColors.textPrimary),
        headlineSmall: PTypography.heading3(color: PColors.textPrimary),
        titleLarge: PTypography.titleLarge(color: PColors.textPrimary),
        titleMedium: PTypography.titleMedium(color: PColors.textPrimary),
        titleSmall: PTypography.titleSmall(color: PColors.textSecondary),
        bodyLarge: PTypography.bodyLarge(color: PColors.textSecondary),
        bodyMedium: PTypography.bodyMedium(color: PColors.textSecondary),
        bodySmall: PTypography.bodySmall(color: PColors.textTertiary),
        labelLarge: PTypography.labelLarge(color: PColors.textPrimary),
        labelMedium: PTypography.labelMedium(color: PColors.textSecondary),
        labelSmall: PTypography.labelSmall(color: PColors.textTertiary),
      ),
      
      // ========================================================================
      // AppBar Theme
      // ========================================================================
      appBarTheme: AppBarTheme(
        elevation: 0,
        centerTitle: false,
        backgroundColor: PColors.backgroundBase,
        foregroundColor: PColors.textPrimary,
        surfaceTintColor: Colors.transparent,
        titleTextStyle: PTypography.heading5(color: PColors.textPrimary),
        toolbarHeight: 64.0,
        systemOverlayStyle: SystemUiOverlayStyle.light,
        iconTheme: IconThemeData(
          color: PColors.textPrimary,
          size: PSpacing.iconLG,
        ),
      ),
      
      // ========================================================================
      // Button Themes
      // ========================================================================
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          elevation: 0,
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColors.textOnAccent,
          backgroundColor: PColors.gradientAStart,
          disabledForegroundColor: PColors.textDisabled,
          disabledBackgroundColor: PColors.backgroundSurface,
        ),
      ),
      
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          elevation: 0,
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColors.textOnAccent,
          backgroundColor: PColors.gradientAStart,
        ),
      ),
      
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          side: BorderSide(
            color: highContrast
                ? PColorsHighContrast.borderDefault
                : PColors.borderDefault,
            width: 1.5,
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColors.textPrimary,
        ),
      ),
      
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.md,
            vertical: PSpacing.sm,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColors.gradientAStart,
        ),
      ),
      
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          foregroundColor: PColors.textPrimary,
          highlightColor: PColors.hoverOverlay,
          padding: EdgeInsets.all(PSpacing.sm),
        ),
      ),
      
      // ========================================================================
      // Input Decoration Theme
      // ========================================================================
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: PColors.backgroundSurface,
        contentPadding: EdgeInsets.symmetric(
          horizontal: PSpacing.inputPaddingHorizontal,
          vertical: PSpacing.inputPaddingVertical,
        ),
        
        // Border styles
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: PColors.borderDefault,
            width: 1.0,
          ),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: PColors.borderDefault,
            width: 1.0,
          ),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: highContrast
                ? PColorsHighContrast.focusRing
                : PColors.focusRing,
            width: 2.0,
          ),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: PColors.error,
            width: 1.0,
          ),
        ),
        focusedErrorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: PColors.error,
            width: 2.0,
          ),
        ),
        disabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: BorderSide(
            color: PColors.borderSubtle,
            width: 1.0,
          ),
        ),
        
        // Text styles
        labelStyle: PTypography.labelMedium(color: PColors.textSecondary),
        floatingLabelStyle: PTypography.labelMedium(
          color: highContrast
              ? PColorsHighContrast.focusRing
              : PColors.focusRing,
        ),
        hintStyle: PTypography.bodyMedium(color: PColors.textTertiary),
        errorStyle: PTypography.labelSmall(color: PColors.error),
        helperStyle: PTypography.caption(color: PColors.textTertiary),
        
        // Icons
        prefixIconColor: PColors.textSecondary,
        suffixIconColor: PColors.textSecondary,
      ),
      
      // ========================================================================
      // Card Theme
      // ========================================================================
      cardTheme: CardThemeData(
        elevation: 0,
        color: PColors.backgroundSurface,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusCard),
          side: BorderSide(
            color: PColors.borderSubtle,
            width: 1.0,
          ),
        ),
        margin: EdgeInsets.all(PSpacing.sm),
      ),
      
      // ========================================================================
      // Dialog Theme
      // ========================================================================
      dialogTheme: DialogThemeData(
        elevation: 8,
        backgroundColor: PColors.backgroundElevated,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusXL),
        ),
        titleTextStyle: PTypography.heading4(color: PColors.textPrimary),
        contentTextStyle: PTypography.bodyMedium(color: PColors.textSecondary),
      ),
      
      // ========================================================================
      // Bottom Sheet Theme
      // ========================================================================
      bottomSheetTheme: BottomSheetThemeData(
        elevation: 0,
        backgroundColor: PColors.backgroundElevated,
        surfaceTintColor: Colors.transparent,
        modalBackgroundColor: PColors.backgroundElevated,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(
            top: Radius.circular(PSpacing.radiusXL),
          ),
        ),
      ),
      
      // ========================================================================
      // List Tile Theme
      // ========================================================================
      listTileTheme: ListTileThemeData(
        contentPadding: EdgeInsets.symmetric(
          horizontal: PSpacing.listItemPaddingHorizontal,
          vertical: PSpacing.listItemPaddingVertical,
        ),
        titleTextStyle: PTypography.titleSmall(color: PColors.textPrimary),
        subtitleTextStyle: PTypography.bodySmall(color: PColors.textSecondary),
        leadingAndTrailingTextStyle: PTypography.labelMedium(
          color: PColors.textSecondary,
        ),
        iconColor: PColors.textSecondary,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        ),
      ),
      
      // ========================================================================
      // Chip Theme
      // ========================================================================
      chipTheme: ChipThemeData(
        backgroundColor: PColors.backgroundSurface,
        selectedColor: PColors.selectedBackground,
        disabledColor: PColors.backgroundSurface,
        labelStyle: PTypography.labelSmall(color: PColors.textPrimary),
        secondaryLabelStyle: PTypography.labelSmall(
          color: PColors.textSecondary,
        ),
        padding: EdgeInsets.symmetric(
          horizontal: PSpacing.sm,
          vertical: PSpacing.xxs,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          side: BorderSide(
            color: PColors.borderDefault,
            width: 1.0,
          ),
        ),
      ),
      
      // ========================================================================
      // Switch Theme
      // ========================================================================
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColors.textOnAccent;
          }
          return PColors.textSecondary;
        }),
        trackColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColors.gradientAStart;
          }
          return PColors.backgroundSurface;
        }),
        trackOutlineColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return Colors.transparent;
          }
          return PColors.borderDefault;
        }),
      ),
      
      // ========================================================================
      // Checkbox Theme
      // ========================================================================
      checkboxTheme: CheckboxThemeData(
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColors.gradientAStart;
          }
          return Colors.transparent;
        }),
        checkColor: WidgetStateProperty.all(PColors.textOnAccent),
        side: BorderSide(
          color: PColors.borderDefault,
          width: 1.5,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusXS),
        ),
      ),
      
      // ========================================================================
      // Radio Theme
      // ========================================================================
      radioTheme: RadioThemeData(
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColors.gradientAStart;
          }
          return PColors.borderDefault;
        }),
      ),
      
      // ========================================================================
      // Tooltip Theme
      // ========================================================================
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: PColors.backgroundElevated,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          border: Border.all(
            color: PColors.borderDefault,
            width: 1.0,
          ),
          boxShadow: [
            BoxShadow(
              color: PColors.shadowStrong,
              blurRadius: 8.0,
              offset: Offset(0, 4),
            ),
          ],
        ),
        textStyle: PTypography.labelSmall(color: PColors.textPrimary),
        padding: EdgeInsets.all(PSpacing.tooltipPadding),
        waitDuration: Duration(milliseconds: 500),
      ),
      
      // ========================================================================
      // Snackbar Theme
      // ========================================================================
      snackBarTheme: SnackBarThemeData(
        elevation: 8,
        backgroundColor: PColors.backgroundElevated,
        contentTextStyle: PTypography.bodyMedium(color: PColors.textPrimary),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        ),
        behavior: SnackBarBehavior.floating,
      ),
      
      // ========================================================================
      // Progress Indicator Theme
      // ========================================================================
      progressIndicatorTheme: ProgressIndicatorThemeData(
        color: PColors.gradientAStart,
        linearTrackColor: PColors.backgroundSurface,
        circularTrackColor: PColors.backgroundSurface,
      ),
      
      // ========================================================================
      // Divider Theme
      // ========================================================================
      dividerTheme: DividerThemeData(
        color: PColors.divider,
        thickness: 1.0,
        space: 1.0,
      ),
    );
  }
  
  /// Light theme - Ivory
  static ThemeData light() {
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.light,

      // ========================================================================
      // Color Scheme
      // ========================================================================
      colorScheme: const ColorScheme.light(
        brightness: Brightness.light,
        primary: PColorsLight.gradientAStart,
        onPrimary: PColorsLight.textOnAccent,
        primaryContainer: PColorsLight.backgroundElevated,
        onPrimaryContainer: PColorsLight.textPrimary,
        secondary: PColorsLight.gradientBStart,
        onSecondary: PColorsLight.textOnAccent,
        secondaryContainer: PColorsLight.backgroundSurface,
        onSecondaryContainer: PColorsLight.textPrimary,
        tertiary: PColorsLight.info,
        onTertiary: PColorsLight.textOnAccent,
        error: PColorsLight.error,
        onError: PColorsLight.textPrimary,
        errorContainer: PColorsLight.errorBackground,
        onErrorContainer: PColorsLight.error,
        surface: PColorsLight.backgroundSurface,
        onSurface: PColorsLight.textPrimary,
        surfaceContainerHighest: PColorsLight.backgroundElevated,
        onSurfaceVariant: PColorsLight.textSecondary,
        outline: PColorsLight.borderDefault,
        outlineVariant: PColorsLight.borderSubtle,
        shadow: PColorsLight.shadow,
        scrim: PColorsLight.backgroundOverlay,
        inverseSurface: PColorsLight.textPrimary,
        onInverseSurface: PColorsLight.backgroundBase,
        inversePrimary: PColorsLight.backgroundBase,
      ),

      scaffoldBackgroundColor: PColorsLight.backgroundBase,
      canvasColor: PColorsLight.backgroundBase,
      cardColor: PColorsLight.backgroundSurface,
      dividerColor: PColorsLight.divider,
      focusColor: PColorsLight.focusRingSubtle,
      hoverColor: PColorsLight.hoverOverlay,
      highlightColor: PColorsLight.pressedOverlay,
      splashColor: PColorsLight.pressedOverlay,
      disabledColor: PColorsLight.textDisabled,

      // ========================================================================
      // Typography
      // ========================================================================
      fontFamily: PTypography.fontFamilyUI,

      textTheme: TextTheme(
        displayLarge: PTypography.displayLarge(color: PColorsLight.textPrimary),
        displayMedium: PTypography.displayMedium(color: PColorsLight.textPrimary),
        displaySmall: PTypography.displaySmall(color: PColorsLight.textPrimary),
        headlineLarge: PTypography.heading1(color: PColorsLight.textPrimary),
        headlineMedium: PTypography.heading2(color: PColorsLight.textPrimary),
        headlineSmall: PTypography.heading3(color: PColorsLight.textPrimary),
        titleLarge: PTypography.titleLarge(color: PColorsLight.textPrimary),
        titleMedium: PTypography.titleMedium(color: PColorsLight.textPrimary),
        titleSmall: PTypography.titleSmall(color: PColorsLight.textSecondary),
        bodyLarge: PTypography.bodyLarge(color: PColorsLight.textSecondary),
        bodyMedium: PTypography.bodyMedium(color: PColorsLight.textSecondary),
        bodySmall: PTypography.bodySmall(color: PColorsLight.textTertiary),
        labelLarge: PTypography.labelLarge(color: PColorsLight.textPrimary),
        labelMedium: PTypography.labelMedium(color: PColorsLight.textSecondary),
        labelSmall: PTypography.labelSmall(color: PColorsLight.textTertiary),
      ),

      // ========================================================================
      // AppBar Theme
      // ========================================================================
      appBarTheme: AppBarTheme(
        elevation: 0,
        centerTitle: false,
        backgroundColor: PColorsLight.backgroundBase,
        foregroundColor: PColorsLight.textPrimary,
        surfaceTintColor: Colors.transparent,
        titleTextStyle: PTypography.heading5(color: PColorsLight.textPrimary),
        toolbarHeight: 64.0,
        systemOverlayStyle: SystemUiOverlayStyle.dark,
        iconTheme: const IconThemeData(
          color: PColorsLight.textPrimary,
          size: PSpacing.iconLG,
        ),
      ),

      // ========================================================================
      // Button Themes
      // ========================================================================
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          elevation: 0,
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColorsLight.textOnAccent,
          backgroundColor: PColorsLight.gradientAStart,
          disabledForegroundColor: PColorsLight.textDisabled,
          disabledBackgroundColor: PColorsLight.backgroundSurface,
        ),
      ),

      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          elevation: 0,
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColorsLight.textOnAccent,
          backgroundColor: PColorsLight.gradientAStart,
        ),
      ),

      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.buttonPaddingHorizontal,
            vertical: PSpacing.buttonPaddingVertical,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          ),
          side: const BorderSide(
            color: PColorsLight.borderDefault,
            width: 1.5,
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColorsLight.textPrimary,
        ),
      ),

      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          padding: EdgeInsets.symmetric(
            horizontal: PSpacing.md,
            vertical: PSpacing.sm,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          ),
          textStyle: PTypography.labelLarge(),
          foregroundColor: PColorsLight.gradientAStart,
        ),
      ),

      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          foregroundColor: PColorsLight.textPrimary,
          highlightColor: PColorsLight.hoverOverlay,
          padding: EdgeInsets.all(PSpacing.sm),
        ),
      ),

      // ========================================================================
      // Input Decoration Theme
      // ========================================================================
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: PColorsLight.backgroundSurface,
        contentPadding: EdgeInsets.symmetric(
          horizontal: PSpacing.inputPaddingHorizontal,
          vertical: PSpacing.inputPaddingVertical,
        ),

        // Border styles
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.borderDefault,
            width: 1.0,
          ),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.borderDefault,
            width: 1.0,
          ),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.focusRing,
            width: 2.0,
          ),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.error,
            width: 1.0,
          ),
        ),
        focusedErrorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.error,
            width: 2.0,
          ),
        ),
        disabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusInput),
          borderSide: const BorderSide(
            color: PColorsLight.borderSubtle,
            width: 1.0,
          ),
        ),

        // Text styles
        labelStyle: PTypography.labelMedium(color: PColorsLight.textSecondary),
        floatingLabelStyle: PTypography.labelMedium(
          color: PColorsLight.focusRing,
        ),
        hintStyle: PTypography.bodyMedium(color: PColorsLight.textTertiary),
        errorStyle: PTypography.labelSmall(color: PColorsLight.error),
        helperStyle: PTypography.caption(color: PColorsLight.textTertiary),

        // Icons
        prefixIconColor: PColorsLight.textSecondary,
        suffixIconColor: PColorsLight.textSecondary,
      ),

      // ========================================================================
      // Card Theme
      // ========================================================================
      cardTheme: CardThemeData(
        elevation: 0,
        color: PColorsLight.backgroundSurface,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusCard),
          side: const BorderSide(
            color: PColorsLight.borderSubtle,
            width: 1.0,
          ),
        ),
        margin: EdgeInsets.all(PSpacing.sm),
      ),

      // ========================================================================
      // Dialog Theme
      // ========================================================================
      dialogTheme: DialogThemeData(
        elevation: 8,
        backgroundColor: PColorsLight.backgroundElevated,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusXL),
        ),
        titleTextStyle: PTypography.heading4(color: PColorsLight.textPrimary),
        contentTextStyle: PTypography.bodyMedium(color: PColorsLight.textSecondary),
      ),

      // ========================================================================
      // Bottom Sheet Theme
      // ========================================================================
      bottomSheetTheme: BottomSheetThemeData(
        elevation: 0,
        backgroundColor: PColorsLight.backgroundElevated,
        surfaceTintColor: Colors.transparent,
        modalBackgroundColor: PColorsLight.backgroundElevated,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(
            top: Radius.circular(PSpacing.radiusXL),
          ),
        ),
      ),

      // ========================================================================
      // List Tile Theme
      // ========================================================================
      listTileTheme: ListTileThemeData(
        contentPadding: EdgeInsets.symmetric(
          horizontal: PSpacing.listItemPaddingHorizontal,
          vertical: PSpacing.listItemPaddingVertical,
        ),
        titleTextStyle: PTypography.titleSmall(color: PColorsLight.textPrimary),
        subtitleTextStyle: PTypography.bodySmall(color: PColorsLight.textSecondary),
        leadingAndTrailingTextStyle: PTypography.labelMedium(
          color: PColorsLight.textSecondary,
        ),
        iconColor: PColorsLight.textSecondary,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        ),
      ),

      // ========================================================================
      // Chip Theme
      // ========================================================================
      chipTheme: ChipThemeData(
        backgroundColor: PColorsLight.backgroundSurface,
        selectedColor: PColorsLight.selectedBackground,
        disabledColor: PColorsLight.backgroundSurface,
        labelStyle: PTypography.labelSmall(color: PColorsLight.textPrimary),
        secondaryLabelStyle: PTypography.labelSmall(
          color: PColorsLight.textSecondary,
        ),
        padding: EdgeInsets.symmetric(
          horizontal: PSpacing.sm,
          vertical: PSpacing.xxs,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          side: const BorderSide(
            color: PColorsLight.borderDefault,
            width: 1.0,
          ),
        ),
      ),

      // ========================================================================
      // Switch Theme
      // ========================================================================
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColorsLight.textOnAccent;
          }
          return PColorsLight.textSecondary;
        }),
        trackColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColorsLight.gradientAStart;
          }
          return PColorsLight.backgroundSurface;
        }),
        trackOutlineColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return Colors.transparent;
          }
          return PColorsLight.borderDefault;
        }),
      ),

      // ========================================================================
      // Checkbox Theme
      // ========================================================================
      checkboxTheme: CheckboxThemeData(
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColorsLight.gradientAStart;
          }
          return Colors.transparent;
        }),
        checkColor: WidgetStateProperty.all(PColorsLight.textOnAccent),
        side: const BorderSide(
          color: PColorsLight.borderDefault,
          width: 1.5,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusXS),
        ),
      ),

      // ========================================================================
      // Radio Theme
      // ========================================================================
      radioTheme: RadioThemeData(
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return PColorsLight.gradientAStart;
          }
          return PColorsLight.borderDefault;
        }),
      ),

      // ========================================================================
      // Tooltip Theme
      // ========================================================================
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: PColorsLight.backgroundElevated,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          border: Border.all(
            color: PColorsLight.borderDefault,
            width: 1.0,
          ),
          boxShadow: const [
            BoxShadow(
              color: PColorsLight.shadowStrong,
              blurRadius: 8.0,
              offset: Offset(0, 4),
            ),
          ],
        ),
        textStyle: PTypography.labelSmall(color: PColorsLight.textPrimary),
        padding: EdgeInsets.all(PSpacing.tooltipPadding),
        waitDuration: const Duration(milliseconds: 500),
      ),

      // ========================================================================
      // Snackbar Theme
      // ========================================================================
      snackBarTheme: SnackBarThemeData(
        elevation: 8,
        backgroundColor: PColorsLight.backgroundElevated,
        contentTextStyle: PTypography.bodyMedium(color: PColorsLight.textPrimary),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        ),
        behavior: SnackBarBehavior.floating,
      ),

      // ========================================================================
      // Progress Indicator Theme
      // ========================================================================
      progressIndicatorTheme: const ProgressIndicatorThemeData(
        color: PColorsLight.gradientAStart,
        linearTrackColor: PColorsLight.backgroundSurface,
        circularTrackColor: PColorsLight.backgroundSurface,
      ),

      // ========================================================================
      // Divider Theme
      // ========================================================================
      dividerTheme: const DividerThemeData(
        color: PColorsLight.divider,
        thickness: 1.0,
        space: 1.0,
      ),
    );
  }
}

