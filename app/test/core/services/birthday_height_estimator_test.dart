import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/services/birthday_height_estimator.dart';

void main() {
  group('BirthdayHeightEstimator', () {
    test('uses checkpointed Pirate blocks for 2026 dates', () {
      final height = BirthdayHeightEstimator.estimateForMonth(
        year: 2026,
        month: 4,
      );

      expect(height, 3894484);
    });

    test('extrapolates after the latest checkpoint with a safety margin', () {
      final height = BirthdayHeightEstimator.estimateForMonth(
        year: 2026,
        month: 6,
        latestHeight: 4000000,
      );

      expect(height, 3981862);
    });

    test('returns genesis for pre-genesis dates', () {
      final height = BirthdayHeightEstimator.estimateForMonth(
        year: 2018,
        month: 1,
      );

      expect(height, 1);
    });
  });
}
