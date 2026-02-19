import 'package:flutter/widgets.dart';

/// Design tokens for spacing - 4/8px scale system
///
/// Follows a consistent 4px/8px scale for all spacing values
/// to maintain rhythm and consistency across the app.
class PSpacing {
  PSpacing._();

  // ============================================================================
  // Base Spacing Scale - 4px increments
  // ============================================================================

  /// Extra extra small spacing - 4px
  static const double xxs = 4.0;

  /// Extra small spacing - 8px
  static const double xs = 8.0;

  /// Small spacing - 12px
  static const double sm = 12.0;

  /// Medium spacing - 16px (base unit)
  static const double md = 16.0;

  /// Large spacing - 24px
  static const double lg = 24.0;

  /// Extra large spacing - 32px
  static const double xl = 32.0;

  /// Extra extra large spacing - 48px
  static const double xxl = 48.0;

  /// Extra extra extra large spacing - 64px
  static const double xxxl = 64.0;

  // ============================================================================
  // Layout Gutters - Responsive spacing
  // ============================================================================

  /// Minimum gutter for mobile - 16px
  static const double gutterMobile = 16.0;

  /// Standard gutter for tablet - 24px
  static const double gutterTablet = 24.0;

  /// Standard gutter for desktop - 32px
  static const double gutterDesktop = 32.0;

  /// Maximum gutter for large screens - 48px
  static const double gutterLarge = 48.0;

  // ============================================================================
  // Component Spacing - Specific use cases
  // ============================================================================

  /// Button horizontal padding
  static const double buttonPaddingHorizontal = 24.0;

  /// Button vertical padding
  static const double buttonPaddingVertical = 12.0;

  /// Input horizontal padding
  static const double inputPaddingHorizontal = 16.0;

  /// Input vertical padding
  static const double inputPaddingVertical = 12.0;

  /// Card padding
  static const double cardPadding = 20.0;

  /// Dialog padding
  static const double dialogPadding = 24.0;

  /// List item vertical padding
  static const double listItemPaddingVertical = 12.0;

  /// List item horizontal padding
  static const double listItemPaddingHorizontal = 16.0;

  /// Bottom sheet padding
  static const double bottomSheetPadding = 24.0;

  /// Snackbar padding
  static const double snackbarPadding = 16.0;

  /// Tooltip padding
  static const double tooltipPadding = 8.0;

  // ============================================================================
  // Element Spacing - Between elements
  // ============================================================================

  /// Space between form fields
  static const double formFieldGap = 16.0;

  /// Space between sections
  static const double sectionGap = 32.0;

  /// Space between icon and text
  static const double iconTextGap = 8.0;

  /// Space between list items
  static const double listItemGap = 8.0;

  /// Space between chips/tags
  static const double chipGap = 8.0;

  // ============================================================================
  // Border Radius - Corner rounding
  // ============================================================================

  /// Extra small radius - 4px
  static const double radiusXS = 4.0;

  /// Small radius - 8px
  static const double radiusSM = 8.0;

  /// Medium radius - 12px
  static const double radiusMD = 12.0;

  /// Large radius - 16px
  static const double radiusLG = 16.0;

  /// Card radius - 14px
  static const double radiusCard = 14.0;

  /// Input radius - 10px
  static const double radiusInput = 10.0;

  /// Extra large radius - 24px
  static const double radiusXL = 24.0;

  /// Full radius - 9999px (pill shape)
  static const double radiusFull = 9999.0;

  // ============================================================================
  // Elevation - Shadow offsets
  // ============================================================================

  /// Small elevation offset
  static const double elevationSM = 2.0;

  /// Medium elevation offset
  static const double elevationMD = 4.0;

  /// Large elevation offset
  static const double elevationLG = 8.0;

  /// Extra large elevation offset
  static const double elevationXL = 16.0;

  // ============================================================================
  // Icon Sizes - Consistent icon sizing
  // ============================================================================

  /// Extra small icon - 12px
  static const double iconXS = 12.0;

  /// Small icon - 16px
  static const double iconSM = 16.0;

  /// Medium icon - 20px
  static const double iconMD = 20.0;

  /// Large icon - 24px
  static const double iconLG = 24.0;

  /// Extra large icon - 32px
  static const double iconXL = 32.0;

  /// Extra extra large icon - 48px
  static const double iconXXL = 48.0;

  // ============================================================================
  // Desktop Specific - Desktop-only spacing
  // ============================================================================

  /// Desktop window minimum width
  static const double desktopMinWidth = 960.0;

  /// Desktop window minimum height
  static const double desktopMinHeight = 600.0;

  /// Desktop titlebar height
  static const double desktopTitlebarHeight = 40.0;

  /// Desktop sidebar width
  static const double desktopSidebarWidth = 240.0;

  /// Desktop navigation rail width
  static const double desktopNavRailWidth = 72.0;

  /// Desktop content max width
  static const double desktopContentMaxWidth = 1200.0;

  // ============================================================================
  // Responsive Breakpoints
  // ============================================================================

  /// Mobile breakpoint - < 600px
  static const double breakpointMobile = 600.0;

  /// Tablet breakpoint - 600px - 1024px
  static const double breakpointTablet = 1024.0;

  /// Desktop breakpoint - > 1024px
  static const double breakpointDesktop = 1024.0;

  /// Large desktop breakpoint - > 1440px
  static const double breakpointLargeDesktop = 1440.0;

  // ============================================================================
  // Utility Methods
  // ============================================================================

  /// Returns responsive gutter based on screen width
  static double responsiveGutter(double screenWidth) {
    if (screenWidth < breakpointMobile) {
      return gutterMobile;
    } else if (screenWidth < breakpointDesktop) {
      return gutterTablet;
    } else if (screenWidth < breakpointLargeDesktop) {
      return gutterDesktop;
    } else {
      return gutterLarge;
    }
  }

  /// Returns responsive padding based on screen width
  static double responsivePadding(double screenWidth) {
    if (screenWidth < breakpointMobile) {
      return md;
    } else if (screenWidth < breakpointDesktop) {
      return lg;
    } else {
      return xl;
    }
  }

  /// Returns true if screen is mobile size
  static bool isMobile(double screenWidth) {
    return screenWidth < breakpointMobile;
  }

  /// Returns true if screen is tablet size
  static bool isTablet(double screenWidth) {
    return screenWidth >= breakpointMobile && screenWidth < breakpointDesktop;
  }

  /// Returns true if screen is desktop size
  static bool isDesktop(double screenWidth) {
    return screenWidth >= breakpointDesktop;
  }

  /// Returns standard screen padding with responsive gutters.
  static EdgeInsets screenPadding(double screenWidth, {double vertical = lg}) {
    final gutter = responsiveGutter(screenWidth);
    return EdgeInsets.fromLTRB(gutter, vertical, gutter, vertical);
  }
}
