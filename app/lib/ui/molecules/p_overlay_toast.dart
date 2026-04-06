import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

final GlobalKey<POverlayToastHostState> rootOverlayToastHostKey =
    GlobalKey<POverlayToastHostState>();

class POverlayToastPayload {
  const POverlayToastPayload({
    required this.message,
    required this.duration,
    required this.borderColor,
    this.icon,
    this.iconColor,
    this.actionLabel,
    this.onAction,
  });

  final String message;
  final Duration duration;
  final Color borderColor;
  final IconData? icon;
  final Color? iconColor;
  final String? actionLabel;
  final VoidCallback? onAction;
}

class POverlayToastHost extends StatefulWidget {
  const POverlayToastHost({required this.child, super.key});

  final Widget child;

  @override
  State<POverlayToastHost> createState() => POverlayToastHostState();
}

class POverlayToastHostState extends State<POverlayToastHost> {
  Timer? _dismissTimer;
  POverlayToastPayload? _payload;
  bool _visible = false;

  void show(POverlayToastPayload payload) {
    _dismissTimer?.cancel();
    setState(() {
      _payload = payload;
      _visible = true;
    });
    _dismissTimer = Timer(payload.duration, hide);
  }

  void hide() {
    _dismissTimer?.cancel();
    _dismissTimer = null;
    if (mounted) {
      setState(() {
        _visible = false;
        _payload = null;
      });
    }
  }

  @override
  void dispose() {
    _dismissTimer?.cancel();
    _dismissTimer = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final payload = _payload;
    final mediaQuery = MediaQuery.of(context);
    final screenWidth = mediaQuery.size.width;
    final bottomInset = mediaQuery.viewInsets.bottom;
    final bottomPadding = mediaQuery.padding.bottom;
    final maxWidth = math.max(
      280.0,
      math.min(720.0, screenWidth - (PSpacing.md * 2)),
    );

    return Stack(
      children: [
        widget.child,
        if (payload != null)
          SafeArea(
            child: Align(
              alignment: Alignment.bottomCenter,
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  PSpacing.md,
                  PSpacing.md,
                  PSpacing.md,
                  16 + bottomInset + bottomPadding,
                ),
                child: AnimatedSlide(
                  offset: _visible ? Offset.zero : const Offset(0, 0.2),
                  duration: const Duration(milliseconds: 180),
                  curve: Curves.easeOutCubic,
                  child: AnimatedOpacity(
                    opacity: _visible ? 1 : 0,
                    duration: const Duration(milliseconds: 180),
                    curve: Curves.easeOut,
                    child: ConstrainedBox(
                      constraints: BoxConstraints(maxWidth: maxWidth),
                      child: Material(
                        color: Colors.transparent,
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            color: AppColors.backgroundElevated,
                            borderRadius: BorderRadius.circular(
                              PSpacing.radiusMD,
                            ),
                            border: Border.all(color: payload.borderColor),
                            boxShadow: [
                              BoxShadow(
                                color: AppColors.shadowStrong,
                                blurRadius: 14,
                                offset: const Offset(0, 6),
                              ),
                            ],
                          ),
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                              horizontal: PSpacing.md,
                              vertical: PSpacing.sm,
                            ),
                            child: Row(
                              children: [
                                if (payload.icon != null) ...[
                                  Icon(
                                    payload.icon,
                                    size: PSpacing.iconMD,
                                    color:
                                        payload.iconColor ??
                                        AppColors.textSecondary,
                                  ),
                                  const SizedBox(width: PSpacing.sm),
                                ],
                                Expanded(
                                  child: Text(
                                    payload.message,
                                    style: PTypography.bodyMedium(
                                      color: AppColors.textPrimary,
                                    ),
                                  ),
                                ),
                                if (payload.actionLabel != null) ...[
                                  const SizedBox(width: PSpacing.sm),
                                  TextButton(
                                    onPressed: () {
                                      payload.onAction?.call();
                                      hide();
                                    },
                                    child: Text(payload.actionLabel!),
                                  ),
                                ],
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }
}

class POverlayToast {
  POverlayToast._();

  static bool get isReady => rootOverlayToastHostKey.currentState != null;

  static void show(POverlayToastPayload payload) {
    rootOverlayToastHostKey.currentState?.show(payload);
  }

  static void hide() {
    rootOverlayToastHostKey.currentState?.hide();
  }
}
