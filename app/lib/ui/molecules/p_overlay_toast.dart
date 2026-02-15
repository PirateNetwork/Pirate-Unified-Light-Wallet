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
  final OverlayPortalController _portalController = OverlayPortalController();
  Timer? _dismissTimer;
  POverlayToastPayload? _payload;

  void show(POverlayToastPayload payload) {
    _dismissTimer?.cancel();
    setState(() {
      _payload = payload;
    });
    _portalController.show();
    _dismissTimer = Timer(payload.duration, hide);
  }

  void hide() {
    _dismissTimer?.cancel();
    _dismissTimer = null;
    if (_portalController.isShowing) {
      _portalController.hide();
    }
    if (_payload != null && mounted) {
      setState(() {
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
    return OverlayPortal.overlayChildLayoutBuilder(
      controller: _portalController,
      overlayLocation: OverlayChildLocation.rootOverlay,
      overlayChildBuilder: (context, layoutInfo) {
        final payload = _payload;
        if (payload == null) {
          return const SizedBox.shrink();
        }

        final bottomInset = MediaQuery.viewInsetsOf(context).bottom;
        final bottomPadding = MediaQuery.paddingOf(context).bottom;
        final maxWidth = math.max(
          280.0,
          math.min(720.0, layoutInfo.overlaySize.width - (PSpacing.md * 2)),
        );

        return SafeArea(
          child: IgnorePointer(
            ignoring: false,
            child: Align(
              alignment: Alignment.bottomCenter,
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  PSpacing.md,
                  PSpacing.md,
                  PSpacing.md,
                  16 + bottomInset + bottomPadding,
                ),
                child: ConstrainedBox(
                  constraints: BoxConstraints(maxWidth: maxWidth),
                  child: Material(
                    color: Colors.transparent,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: AppColors.backgroundElevated,
                        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
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
        );
      },
      child: widget.child,
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
