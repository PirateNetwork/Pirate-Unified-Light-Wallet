import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';

void main() {
  test('keeps handset classification stable across rotation', () {
    const portraitPhone = Size(390, 844);
    const landscapePhone = Size(844, 390);

    expect(PSpacing.isHandset(portraitPhone), isTrue);
    expect(PSpacing.isHandset(landscapePhone), isTrue);
    expect(PSpacing.isCompactLandscape(portraitPhone), isFalse);
    expect(PSpacing.isCompactLandscape(landscapePhone), isTrue);
  });

  test('does not classify a tablet as a handset', () {
    const portraitTablet = Size(768, 1024);
    const landscapeTablet = Size(1024, 768);

    expect(PSpacing.isHandset(portraitTablet), isFalse);
    expect(PSpacing.isHandset(landscapeTablet), isFalse);
    expect(PSpacing.isCompactLandscape(landscapeTablet), isFalse);
  });

  test('uses compact desktop density only for constrained viewports', () {
    expect(PSpacing.isCompactDesktopViewport(const Size(1097, 706)), isTrue);
    expect(PSpacing.isCompactDesktopViewport(const Size(1180, 760)), isFalse);
    expect(PSpacing.isCompactDesktopViewport(const Size(1440, 900)), isFalse);
  });
}
