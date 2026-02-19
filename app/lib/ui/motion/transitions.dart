import 'package:flutter/material.dart';
import 'curves.dart';

/// Page transitions for Pirate Wallet
class PTransitions {
  PTransitions._();

  /// Fade transition
  static Widget fade(
    BuildContext context,
    Animation<double> animation,
    Widget child,
  ) {
    return FadeTransition(
      opacity: CurveTween(curve: PCurves.standard).animate(animation),
      child: child,
    );
  }

  /// Slide from right transition
  static Widget slideFromRight(
    BuildContext context,
    Animation<double> animation,
    Widget child,
  ) {
    return SlideTransition(
      position: Tween<Offset>(
        begin: const Offset(1.0, 0.0),
        end: Offset.zero,
      ).animate(CurvedAnimation(parent: animation, curve: PCurves.emphasized)),
      child: child,
    );
  }

  /// Slide from bottom transition
  static Widget slideFromBottom(
    BuildContext context,
    Animation<double> animation,
    Widget child,
  ) {
    return SlideTransition(
      position: Tween<Offset>(
        begin: const Offset(0.0, 1.0),
        end: Offset.zero,
      ).animate(CurvedAnimation(parent: animation, curve: PCurves.emphasized)),
      child: child,
    );
  }

  /// Scale transition
  static Widget scale(
    BuildContext context,
    Animation<double> animation,
    Widget child,
  ) {
    return ScaleTransition(
      scale: CurveTween(curve: PCurves.emphasized).animate(animation),
      child: child,
    );
  }

  /// Fade + Scale transition
  static Widget fadeScale(
    BuildContext context,
    Animation<double> animation,
    Widget child,
  ) {
    return FadeTransition(
      opacity: CurveTween(curve: PCurves.standard).animate(animation),
      child: ScaleTransition(
        scale: Tween<double>(begin: 0.9, end: 1.0).animate(
          CurvedAnimation(parent: animation, curve: PCurves.emphasized),
        ),
        child: child,
      ),
    );
  }
}
