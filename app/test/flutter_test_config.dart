import 'dart:async';
import '../test_driver/golden_test_config.dart';

Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  // Configure golden test comparator
  GoldenTestConfig.configure();

  // Run tests
  await testMain();
}

