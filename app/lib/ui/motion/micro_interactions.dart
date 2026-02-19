import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'curves.dart';
import 'durations.dart';

/// Micro-interactions for Pirate Wallet
/// Reusable animation patterns
extension MicroInteractions on Widget {
  /// Hover scale effect
  Widget hoverScale({
    double scale = 1.02,
    Duration duration = PDurations.fast,
    Curve curve = PCurves.standard,
  }) {
    return animate().scaleXY(
      begin: 1.0,
      end: scale,
      duration: duration,
      curve: curve,
    );
  }

  /// Press scale effect
  Widget pressScale({
    double scale = 0.98,
    Duration duration = PDurations.extraFast,
    Curve curve = PCurves.snap,
  }) {
    return animate().scaleXY(
      begin: 1.0,
      end: scale,
      duration: duration,
      curve: curve,
    );
  }

  /// Fade in effect
  Widget fadeIn({
    Duration duration = PDurations.normal,
    Curve curve = PCurves.standard,
    Duration delay = Duration.zero,
  }) {
    return animate(delay: delay).fadeIn(duration: duration, curve: curve);
  }

  /// Slide in from bottom
  Widget slideInFromBottom({
    Duration duration = PDurations.medium,
    Curve curve = PCurves.emphasized,
    Duration delay = Duration.zero,
  }) {
    return animate(delay: delay)
        .slideY(begin: 1.0, end: 0.0, duration: duration, curve: curve)
        .fadeIn(duration: duration, curve: curve);
  }

  /// Shimmer effect (loading)
  Widget shimmer({Duration duration = PDurations.verySlow}) {
    return animate(
      onPlay: (controller) => controller.repeat(),
    ).shimmer(duration: duration);
  }

  /// Shake effect (error)
  Widget shake({Duration duration = PDurations.normal}) {
    return animate().shake(duration: duration, hz: 4);
  }

  /// Pulse effect (attention)
  Widget pulse({Duration duration = PDurations.verySlow, int repeat = 3}) {
    return animate(
      onPlay: (controller) {
        controller.repeat(reverse: true, period: duration);
      },
    ).scaleXY(end: 1.1);
  }
}
