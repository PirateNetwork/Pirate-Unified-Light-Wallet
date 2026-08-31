import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/services/wallet_name_suggestion.dart';

void main() {
  test('starts the default wallet sequence at one', () {
    expect(nextArrrWalletNumber(const []), 1);
  });

  test('continues after the highest standard wallet name', () {
    expect(
      nextArrrWalletNumber(const [
        'My ARRR Wallet 1',
        'Savings',
        'my arrr wallet 3',
      ]),
      4,
    );
  });

  test('ignores malformed or unrelated names', () {
    expect(
      nextArrrWalletNumber(const [
        'My ARRR Wallet',
        'My ARRR Wallet 0',
        'My ARRR Wallet old',
        'ARRR Wallet 8',
      ]),
      1,
    );
  });
}
