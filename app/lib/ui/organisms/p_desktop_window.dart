/// Desktop Window Components — Custom Titlebar & Vibrancy
///
/// Provides platform-native window chrome with:
/// - Custom titlebar (draggable, window controls)
/// - Vibrancy effect (macOS) / Mica effect (Windows 11)
/// - High-DPI icon support
/// - Keyboard navigation
library;

import 'dart:io';
import 'package:flutter/material.dart';
import '../../design/deep_space_theme.dart';
import '../../design/tokens/spacing.dart';

// =============================================================================
// Custom Titlebar
// =============================================================================

/// Custom window titlebar for desktop platforms
class PDesktopTitlebar extends StatelessWidget {
  const PDesktopTitlebar({
    this.title,
    this.leading,
    this.actions,
    this.showWindowControls = true,
    this.backgroundColor,
    this.height = DeepSpaceDesktop.titlebarHeight,
    super.key,
  });

  final String? title;
  final Widget? leading;
  final List<Widget>? actions;
  final bool showWindowControls;
  final Color? backgroundColor;
  final double height;

  @override
  Widget build(BuildContext context) {
    // Only show custom titlebar on desktop
    if (!Platform.isWindows && !Platform.isMacOS && !Platform.isLinux) {
      return const SizedBox.shrink();
    }

    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanStart: (_) => _startWindowDrag(),
      onDoubleTap: _toggleMaximize,
      child: Container(
        height: height,
        color: backgroundColor ?? AppColors.void_,
        child: Row(
          children: [
            // macOS: Window controls on left
            if (Platform.isMacOS && showWindowControls) ...[
              const SizedBox(width: 8),
              const _MacOSWindowControls(),
              const SizedBox(width: 12),
            ],

            // Leading widget
            if (leading != null) ...[
              const SizedBox(width: 8),
              leading!,
            ],

            // Title (centered on macOS, left on others)
            Expanded(
              child: Platform.isMacOS
                  ? Center(
                      child: _buildTitle(),
                    )
                  : Padding(
                      padding: const EdgeInsets.only(left: 16),
                      child: _buildTitle(),
                    ),
            ),

            // Actions
            if (actions != null && actions!.isNotEmpty) ...[
              ...actions!.map((action) => Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4),
                child: action,
              )),
            ],

            // Windows/Linux: Window controls on right
            if ((Platform.isWindows || Platform.isLinux) && showWindowControls) ...[
              const _WindowsWindowControls(),
            ],
          ],
        ),
      ),
    );
  }

  Widget? _buildTitle() {
    if (title == null) return null;
    return Text(
      title!,
      style: AppTypography.caption.copyWith(
        color: AppColors.textSecondary,
        fontWeight: FontWeight.w500,
        letterSpacing: 0.5,
      ),
      overflow: TextOverflow.ellipsis,
    );
  }

  void _startWindowDrag() {
    // In production, use bitsdojo_window or window_manager
    // For now, placeholder
  }

  void _toggleMaximize() {
    // In production, toggle maximize state
  }
}

/// macOS-style window controls (traffic lights)
class _MacOSWindowControls extends StatefulWidget {
  const _MacOSWindowControls();

  @override
  State<_MacOSWindowControls> createState() => _MacOSWindowControlsState();
}

class _MacOSWindowControlsState extends State<_MacOSWindowControls> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _MacOSButton(
            color: const Color(0xFFFF5F57),
            icon: Icons.close,
            showIcon: _isHovered,
            onTap: _closeWindow,
            semanticLabel: 'Close window',
          ),
          const SizedBox(width: 8),
          _MacOSButton(
            color: const Color(0xFFFFBD2E),
            icon: Icons.remove,
            showIcon: _isHovered,
            onTap: _minimizeWindow,
            semanticLabel: 'Minimize window',
          ),
          const SizedBox(width: 8),
          _MacOSButton(
            color: const Color(0xFF28C840),
            icon: Icons.crop_square,
            showIcon: _isHovered,
            onTap: _maximizeWindow,
            semanticLabel: 'Maximize window',
          ),
        ],
      ),
    );
  }

  void _closeWindow() {}
  void _minimizeWindow() {}
  void _maximizeWindow() {}
}

class _MacOSButton extends StatelessWidget {
  const _MacOSButton({
    required this.color,
    required this.icon,
    required this.showIcon,
    required this.onTap,
    required this.semanticLabel,
  });

  final Color color;
  final IconData icon;
  final bool showIcon;
  final VoidCallback onTap;
  final String semanticLabel;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: semanticLabel,
      button: true,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          width: 12,
          height: 12,
          decoration: BoxDecoration(
            color: color,
            shape: BoxShape.circle,
          ),
          child: showIcon
              ? Icon(
                  icon,
                  size: 8,
                  color: Colors.black.withValues(alpha: 0.5),
                )
              : null,
        ),
      ),
    );
  }
}

/// Windows-style window controls
class _WindowsWindowControls extends StatelessWidget {
  const _WindowsWindowControls();

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _WindowsButton(
          icon: Icons.remove,
          onTap: _minimizeWindow,
          semanticLabel: 'Minimize',
        ),
        _WindowsButton(
          icon: Icons.crop_square_outlined,
          onTap: _maximizeWindow,
          semanticLabel: 'Maximize',
        ),
        _WindowsButton(
          icon: Icons.close,
          onTap: _closeWindow,
          hoverColor: AppColors.error,
          semanticLabel: 'Close',
        ),
      ],
    );
  }

  void _minimizeWindow() {}
  void _maximizeWindow() {}
  void _closeWindow() {}
}

class _WindowsButton extends StatefulWidget {
  const _WindowsButton({
    required this.icon,
    required this.onTap,
    required this.semanticLabel,
    this.hoverColor,
  });

  final IconData icon;
  final VoidCallback onTap;
  final String semanticLabel;
  final Color? hoverColor;

  @override
  State<_WindowsButton> createState() => _WindowsButtonState();
}

class _WindowsButtonState extends State<_WindowsButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: widget.semanticLabel,
      button: true,
      child: MouseRegion(
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() => _isHovered = false),
        child: GestureDetector(
          onTap: widget.onTap,
          child: Container(
            width: 46,
            height: DeepSpaceDesktop.titlebarHeight,
            color: _isHovered
                ? (widget.hoverColor ?? AppColors.hover)
                : Colors.transparent,
            child: Icon(
              widget.icon,
              size: 16,
              color: _isHovered && widget.hoverColor != null
                  ? Colors.white
                  : AppColors.textSecondary,
            ),
          ),
        ),
      ),
    );
  }
}

// =============================================================================
// Vibrancy/Mica Container
// =============================================================================

/// Container with platform-native blur effect
/// - macOS: NSVisualEffectView vibrancy
/// - Windows 11: Mica/Acrylic
/// - Others: Solid color fallback
class PVibrancyContainer extends StatelessWidget {
  const PVibrancyContainer({
    required this.child,
    this.blurAmount = 30.0,
    this.opacity = 0.8,
    this.borderRadius,
    super.key,
  });

  final Widget child;
  final double blurAmount;
  final double opacity;
  final BorderRadius? borderRadius;

  @override
  Widget build(BuildContext context) {
    // Check platform support
    if (!DeepSpaceDesktop.supportsVibrancy) {
      return DecoratedBox(
        decoration: BoxDecoration(
          color: AppColors.nebula,
          borderRadius: borderRadius,
        ),
        child: child,
      );
    }

    // Platform-specific vibrancy
    return ClipRRect(
      borderRadius: borderRadius ?? BorderRadius.zero,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: DeepSpaceDesktop.vibrancyColor,
          borderRadius: borderRadius,
        ),
        // In production, use flutter_acrylic package for actual blur
        child: child,
      ),
    );
  }
}

// =============================================================================
// Responsive Desktop Layout
// =============================================================================

/// Responsive scaffold for desktop with sidebar
class PDesktopScaffold extends StatelessWidget {
  const PDesktopScaffold({
    required this.body,
    this.sidebar,
    this.titlebar,
    this.showTitlebar = true,
    this.sidebarWidth = DeepSpaceDesktop.sidebarWidth,
    this.collapsedSidebarWidth = DeepSpaceDesktop.navRailWidth,
    this.sidebarCollapsed = false,
    super.key,
  });

  final Widget body;
  final Widget? sidebar;
  final Widget? titlebar;
  final bool showTitlebar;
  final double sidebarWidth;
  final double collapsedSidebarWidth;
  final bool sidebarCollapsed;

  @override
  Widget build(BuildContext context) {
    final isDesktop = MediaQuery.of(context).size.width >= PSpacing.breakpointDesktop;

    if (!isDesktop) {
      // Mobile/tablet: No sidebar
      return Scaffold(
        body: body,
      );
    }

    return Scaffold(
      body: Column(
        children: [
          // Titlebar
          if (showTitlebar)
            titlebar ?? const PDesktopTitlebar(),

          // Main content
          Expanded(
            child: Row(
              children: [
                // Sidebar
                if (sidebar != null)
                  AnimatedContainer(
                    duration: DeepSpaceDurations.medium,
                    curve: DeepSpaceCurves.emphasized,
                    width: sidebarCollapsed ? collapsedSidebarWidth : sidebarWidth,
                    child: sidebar,
                  ),

                // Divider
                if (sidebar != null)
                  Container(
                    width: 1,
                    color: AppColors.divider,
                  ),

                // Body
                Expanded(child: body),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

// =============================================================================
// High-DPI Icon Support
// =============================================================================

/// Icon with automatic high-DPI scaling
class PIcon extends StatelessWidget {
  const PIcon(
    this.icon, {
    this.size,
    this.color,
    this.semanticLabel,
    super.key,
  });

  final IconData icon;
  final double? size;
  final Color? color;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    final devicePixelRatio = MediaQuery.of(context).devicePixelRatio;
    final effectiveSize = size ?? 24.0;
    
    // Ensure crisp rendering on high-DPI displays
    final adjustedSize = (effectiveSize * devicePixelRatio).roundToDouble() / devicePixelRatio;

    return Semantics(
      label: semanticLabel,
      child: Icon(
        icon,
        size: adjustedSize,
        color: color ?? AppColors.textPrimary,
      ),
    );
  }
}

/// Asset image with high-DPI support
class PImage extends StatelessWidget {
  const PImage({
    required this.asset,
    this.width,
    this.height,
    this.fit = BoxFit.contain,
    this.semanticLabel,
    super.key,
  });

  final String asset;
  final double? width;
  final double? height;
  final BoxFit fit;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: semanticLabel,
      image: true,
      child: Image.asset(
        asset,
        width: width,
        height: height,
        fit: fit,
        filterQuality: FilterQuality.high,
        isAntiAlias: true,
      ),
    );
  }
}

