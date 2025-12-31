/// Motion durations for Pirate Wallet
/// Consistent timing for all animations
class PDurations {
  PDurations._();

  /// Instant - 0ms (no animation)
  static const Duration instant = Duration.zero;

  /// Extra fast - 80ms (micro-interactions)
  static const Duration extraFast = Duration(milliseconds: 80);

  /// Fast - 120ms (hover states, focus)
  static const Duration fast = Duration(milliseconds: 120);

  /// Normal - 160ms (button presses, simple transitions)
  static const Duration normal = Duration(milliseconds: 160);

  /// Medium - 220ms (page transitions, modals)
  static const Duration medium = Duration(milliseconds: 220);

  /// Navigation forward - 220ms
  static const Duration navForward = Duration(milliseconds: 220);

  /// Navigation back - 200ms
  static const Duration navBack = Duration(milliseconds: 200);

  /// Slow - 300ms (complex animations)
  static const Duration slow = Duration(milliseconds: 300);

  /// Extra slow - 400ms (hero animations)
  static const Duration extraSlow = Duration(milliseconds: 400);

  /// Very slow - 600ms (loading states)
  static const Duration verySlow = Duration(milliseconds: 600);
}

