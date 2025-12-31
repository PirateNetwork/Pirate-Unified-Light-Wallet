import 'package:flutter/animation.dart';

/// Motion curves for Pirate Wallet
/// Custom easing curves for premium feel
class PCurves {
  PCurves._();

  /// Standard easing - Default for most animations
  static const Curve standard = Curves.easeOutCubic;

  /// Emphasized easing - For important transitions
  static const Curve emphasized = Curves.easeInOutCubic;

  /// Decelerated easing - For entrances
  static const Curve decelerated = Curves.easeOutCubic;

  /// Accelerated easing - For exits
  static const Curve accelerated = Curves.easeInCubic;

  /// Spring curve - For playful interactions
  static const Curve spring = Curves.easeOutCubic;

  /// Bounce curve - For attention-grabbing
  static const Curve bounce = Curves.easeOutCubic;

  /// Snap curve - For instant feedback
  static const Curve snap = Curves.easeInOutCubic;

  /// Smooth curve - For smooth continuous motion
  static const Curve smooth = Curves.easeInOutQuart;
}

