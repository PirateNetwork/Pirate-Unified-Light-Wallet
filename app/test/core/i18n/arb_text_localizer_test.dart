import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/i18n/arb_text_localizer.dart';

void main() {
  group('ArbTextLocalizer.translateWithArgs', () {
    test('replaces named placeholders in the source fallback', () {
      final translated = ArbTextLocalizer.instance.translateWithArgs(
        'Send {amount} {ticker} to {recipient}',
        <String, Object?>{
          'amount': '1.25',
          'ticker': 'ARRR',
          'recipient': 'zs1...',
        },
      );

      expect(translated, 'Send 1.25 ARRR to zs1...');
    });

    test('replaces repeated placeholders and preserves missing ones', () {
      final translated = ArbTextLocalizer.instance.translateWithArgs(
        '{ticker} to {ticker}: {amount} {missing}',
        <String, Object?>{'ticker': 'ARRR', 'amount': 2},
      );

      expect(translated, 'ARRR to ARRR: 2 {missing}');
    });

    test('does not reinterpret placeholders inside argument values', () {
      final translated = ArbTextLocalizer.instance.translateWithArgs(
        '{message} - {ticker}',
        <String, Object?>{
          'message': 'Keep {ticker} literally',
          'ticker': 'ARRR',
        },
      );

      expect(translated, 'Keep {ticker} literally - ARRR');
    });
  });
}
