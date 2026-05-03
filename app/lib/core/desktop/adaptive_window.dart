import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:screen_retriever/screen_retriever.dart';

const Size kDesktopPreferredWindowSize = Size(1180, 760);
const Size kDesktopPreferredMinimumSize = Size(860, 560);

const double _desktopVisibleMargin = 48.0;
const double _desktopMaxVisibleFraction = 0.92;
const double _desktopPreferredAspectRatio = 1180.0 / 760.0;

class DesktopWindowSpec {
  const DesktopWindowSpec({
    required this.initialSize,
    required this.minimumSize,
  });

  final Size initialSize;
  final Size minimumSize;
}

Future<DesktopWindowSpec> resolveDesktopWindowSpecForCurrentDisplay() async {
  try {
    final primaryDisplay = await screenRetriever.getPrimaryDisplay();
    final allDisplays = await screenRetriever.getAllDisplays();
    final cursorPoint = await screenRetriever.getCursorScreenPoint();

    final currentDisplay = allDisplays.firstWhere(
      (display) {
        final visibleSize = display.visibleSize ?? display.size;
        final visiblePosition = display.visiblePosition ?? Offset.zero;
        return Rect.fromLTWH(
          visiblePosition.dx,
          visiblePosition.dy,
          visibleSize.width,
          visibleSize.height,
        ).contains(cursorPoint);
      },
      orElse: () => primaryDisplay,
    );

    return resolveDesktopWindowSpec(
      currentDisplay.visibleSize ?? currentDisplay.size,
    );
  } catch (_) {
    return resolveDesktopWindowSpec(const Size(1920, 1080));
  }
}

@visibleForTesting
DesktopWindowSpec resolveDesktopWindowSpec(Size visibleDisplaySize) {
  if (visibleDisplaySize.width <= 0 || visibleDisplaySize.height <= 0) {
    return const DesktopWindowSpec(
      initialSize: kDesktopPreferredWindowSize,
      minimumSize: kDesktopPreferredMinimumSize,
    );
  }

  final availableWidth = _availableExtent(visibleDisplaySize.width);
  final availableHeight = _availableExtent(visibleDisplaySize.height);

  var width = math.min(kDesktopPreferredWindowSize.width, availableWidth);
  var height = math.min(kDesktopPreferredWindowSize.height, availableHeight);

  if (width / height > _desktopPreferredAspectRatio) {
    width = height * _desktopPreferredAspectRatio;
  } else {
    height = width / _desktopPreferredAspectRatio;
  }

  final minimumWidth = math.min(kDesktopPreferredMinimumSize.width, width);
  final minimumHeight = math.min(kDesktopPreferredMinimumSize.height, height);

  return DesktopWindowSpec(
    initialSize: Size(width.floorToDouble(), height.floorToDouble()),
    minimumSize: Size(
      math.min(width, math.max(640.0, minimumWidth)).floorToDouble(),
      math.min(height, math.max(480.0, minimumHeight)).floorToDouble(),
    ),
  );
}

double _availableExtent(double visibleExtent) {
  if (visibleExtent <= 0) {
    return 0;
  }

  final insetExtent = visibleExtent - _desktopVisibleMargin;
  final proportionalExtent = visibleExtent * _desktopMaxVisibleFraction;
  return math.max(320.0, math.min(insetExtent, proportionalExtent));
}
