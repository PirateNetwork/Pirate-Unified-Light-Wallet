import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/swap_history_csv.dart';
import 'package:pirate_wallet/core/swaps/swap_models.dart';
import 'package:test/test.dart';

void main() {
  test('exports swap history as newest-first CSV with escaped cells', () {
    final older = _intent(
      id: 'older',
      updatedAt: DateTime.utc(2026, 6, 5, 9),
      status: SwapIntentStatus.completed,
      lastError: '',
    );
    final newer = _intent(
      id: 'newer',
      updatedAt: DateTime.utc(2026, 6, 5, 12),
      status: SwapIntentStatus.failed,
      lastError: 'Failed, because "network" dropped',
    );

    final csv = buildSwapHistoryCsv([older, newer]);
    final lines = csv.trimRight().split('\n');

    expect(lines.first, startsWith('id,wallet_id,pair,side,status'));
    expect(lines[1], startsWith('newer,wallet-1,ARRR/LTC,buyArrr,failed'));
    expect(lines[2], startsWith('older,wallet-1,ARRR/LTC,buyArrr,completed'));
    expect(csv, contains('"Failed, because ""network"" dropped"'));
  });
}

SwapIntent _intent({
  required String id,
  required DateTime updatedAt,
  required SwapIntentStatus status,
  required String lastError,
}) {
  return SwapIntent(
    id: id,
    walletId: 'wallet-1',
    side: SwapSide.buyArrr,
    status: status,
    createdAt: updatedAt.subtract(const Duration(minutes: 5)),
    updatedAt: updatedAt,
    marketSwapUuid: 'swap-$id',
    lastError: lastError,
    plan: SwapPlan(
      side: SwapSide.buyArrr,
      referencePriceLtcPerArrr: Decimal.parse('0.01'),
      marketArrrAmount: Decimal.parse('100'),
      marketLtcAmount: Decimal.parse('1'),
      remainderLtcAmount: Decimal.zero,
      remainderArrrAmount: Decimal.zero,
      slippageCap: Decimal.parse('0.05'),
      realizedSlippage: Decimal.parse('0.01'),
      fills: const [],
      appFeeArrrAmount: Decimal.parse('0.37'),
    ),
  );
}
