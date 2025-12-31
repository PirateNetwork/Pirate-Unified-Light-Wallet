/// Utilities for golden testing
library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:golden_toolkit/golden_toolkit.dart';

/// Configure golden test environment
Future<void> configureGoldenTests() async {
  await loadAppFonts();
  
  // Configure golden file comparator
  if (autoUpdateGoldenFiles) {
    debugPrint('⚠️ Golden files will be updated');
  }
}

/// Expect widget to match golden file with specific size
Future<void> expectGoldenMatches(
  WidgetTester tester,
  String goldenFile, {
  Size size = const Size(375, 812),
}) async {
  // Set device size
  tester.binding.window.physicalSizeTestValue = size;
  tester.binding.window.devicePixelRatioTestValue = 1.0;

  // Rebuild with new size
  await tester.pumpAndSettle();

  // Compare with golden
  await expectLater(
    find.byType(MaterialApp),
    matchesGoldenFile('goldens/$goldenFile'),
  );

  // Reset size
  addTearDown(tester.binding.window.clearPhysicalSizeTestValue);
}

/// Standard device configurations for testing
class DeviceSizes {
  static const Size iphoneSE = Size(375, 667);
  static const Size iphoneX = Size(375, 812);
  static const Size iphone14Pro = Size(393, 852);
  static const Size ipadPro11 = Size(834, 1194);
  static const Size desktop1080p = Size(1920, 1080);
  static const Size desktop4k = Size(3840, 2160);
}

/// Test multiple device sizes
Future<void> testMultipleDevices(
  WidgetTester tester,
  Widget widget,
  String baseFileName, {
  List<({String name, Size size})>? devices,
}) async {
  final testDevices = devices ??
      [
        (name: 'mobile', size: DeviceSizes.iphoneX),
        (name: 'tablet', size: DeviceSizes.ipadPro11),
        (name: 'desktop', size: DeviceSizes.desktop1080p),
      ];

  for (final device in testDevices) {
    await tester.pumpWidget(widget);
    
    await expectGoldenMatches(
      tester,
      '${baseFileName}_${device.name}.png',
      size: device.size,
    );
  }
}

