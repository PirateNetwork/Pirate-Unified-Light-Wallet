import 'dart:io';
import 'package:flutter/material.dart';
import 'package:bitsdojo_window/bitsdojo_window.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../atoms/p_icon_button.dart';

/// Pirate Wallet Scaffold with custom titlebar for desktop
class PScaffold extends StatelessWidget {
  const PScaffold({
    required this.body,
    this.title,
    this.appBar,
    this.drawer,
    this.floatingActionButton,
    this.bottomNavigationBar,
    this.useSafeArea = true,
    super.key,
  });

  final Widget body;
  final String? title;
  final PreferredSizeWidget? appBar;
  final Widget? drawer;
  final Widget? floatingActionButton;
  final Widget? bottomNavigationBar;
  final bool useSafeArea;

  bool get _isDesktop => Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  @override
  Widget build(BuildContext context) {
    final content = useSafeArea ? SafeArea(child: body) : body;

    if (_isDesktop) {
      return Scaffold(
        backgroundColor: AppColors.backgroundBase,
        body: Column(
          children: [
            // Custom titlebar for desktop
            _CustomTitleBar(title: title ?? 'Pirate Wallet'),

            if (appBar != null)
              SizedBox(
                height: appBar!.preferredSize.height,
                child: appBar,
              ),
            // Main content
            Expanded(
              child: content,
            ),
          ],
        ),
        drawer: drawer,
        floatingActionButton: floatingActionButton,
        bottomNavigationBar: bottomNavigationBar,
      );
    }

    return Scaffold(
      backgroundColor: AppColors.backgroundBase,
      appBar: appBar,
      body: content,
      drawer: drawer,
      floatingActionButton: floatingActionButton,
      bottomNavigationBar: bottomNavigationBar,
    );
  }
}

/// Custom titlebar for desktop with window controls
class _CustomTitleBar extends StatelessWidget {
  const _CustomTitleBar({required this.title});

  final String title;

  @override
  Widget build(BuildContext context) {
    return WindowTitleBarBox(
      child: Container(
        height: PSpacing.desktopTitlebarHeight,
        decoration: BoxDecoration(
          color: AppColors.backgroundBase,
          border: Border(
            bottom: BorderSide(
              color: AppColors.borderSubtle,
              width: 1.0,
            ),
          ),
        ),
        child: Row(
          children: [
            // Draggable area
            Expanded(
              child: MoveWindow(
                child: Padding(
                  padding: EdgeInsets.only(left: PSpacing.md),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      title,
                      style: PTypography.labelLarge(color: AppColors.textPrimary),
                    ),
                  ),
                ),
              ),
            ),
            // Window controls
            _WindowControls(),
          ],
        ),
      ),
    );
  }
}

/// Window control buttons (minimize, maximize, close)
class _WindowControls extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        _WindowButton(
          icon: Icons.remove,
          onPressed: () => appWindow.minimize(),
        ),
        _WindowButton(
          icon: Icons.crop_square,
          onPressed: () => appWindow.maximizeOrRestore(),
        ),
        _WindowButton(
          icon: Icons.close,
          onPressed: () => appWindow.close(),
          isClose: true,
        ),
      ],
    );
  }
}

/// Individual window button
class _WindowButton extends StatefulWidget {
  const _WindowButton({
    required this.icon,
    required this.onPressed,
    this.isClose = false,
  });

  final IconData icon;
  final VoidCallback onPressed;
  final bool isClose;

  @override
  State<_WindowButton> createState() => _WindowButtonState();
}

class _WindowButtonState extends State<_WindowButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: GestureDetector(
        onTap: widget.onPressed,
        child: Container(
          width: PSpacing.desktopTitlebarHeight,
          height: PSpacing.desktopTitlebarHeight,
          decoration: BoxDecoration(
            color: _isHovered
                ? (widget.isClose ? AppColors.error : AppColors.hoverOverlay)
                : Colors.transparent,
          ),
          child: Icon(
            widget.icon,
            size: PSpacing.iconSM,
            color: _isHovered && widget.isClose
                ? AppColors.textOnAccent
                : AppColors.textPrimary,
          ),
        ),
      ),
    );
  }
}

