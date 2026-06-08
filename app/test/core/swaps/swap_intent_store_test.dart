import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/swap_intent_store.dart';
import 'package:pirate_wallet/core/swaps/swap_models.dart';
import 'package:test/test.dart';

void main() {
  SwapPlan plan() => SwapPlan(
    side: SwapSide.buyArrr,
    referencePriceLtcPerArrr: Decimal.parse('0.01'),
    marketArrrAmount: Decimal.parse('10'),
    marketLtcAmount: Decimal.parse('0.1'),
    remainderLtcAmount: Decimal.zero,
    remainderArrrAmount: Decimal.zero,
    slippageCap: Decimal.parse('0.03'),
    realizedSlippage: Decimal.zero,
    fills: const [],
    appFeeArrrAmount: Decimal.zero,
  );

  SwapIntent intent(String id, SwapIntentStatus status, {DateTime? createdAt}) {
    final now = createdAt ?? DateTime.utc(2026);
    return SwapIntent(
      id: id,
      walletId: 'wallet-a',
      side: SwapSide.buyArrr,
      status: status,
      createdAt: now,
      updatedAt: now,
      plan: plan(),
      ltcDepositAddress: 'ltc-address',
      arrReceivingAddress: 'zs-address',
    );
  }

  test(
    'in-memory store persists, updates, and removes intents by wallet',
    () async {
      final store = InMemorySwapIntentStore();
      await store.upsert(intent('one', SwapIntentStatus.waitingForDeposit));
      await store.upsert(intent('two', SwapIntentStatus.limitOrderPlaced));

      expect(await store.load('wallet-a'), hasLength(2));

      await store.upsert(intent('one', SwapIntentStatus.completed));
      final updated = await store.load('wallet-a');
      expect(updated, hasLength(2));
      expect(
        updated.singleWhere((value) => value.id == 'one').status,
        SwapIntentStatus.completed,
      );

      await store.remove('wallet-a', 'one');
      expect((await store.load('wallet-a')).map((value) => value.id), ['two']);
      expect(await store.load('wallet-b'), isEmpty);
    },
  );

  test('intent JSON survives round trip for resume after restart', () {
    final original = intent(
      'resume-me',
      SwapIntentStatus.marketSwapStarted,
    ).copyWith(marketSwapUuid: 'swap-uuid');
    final restored = SwapIntent.fromJson(original.toJson());

    expect(restored.id, original.id);
    expect(restored.walletId, original.walletId);
    expect(restored.status, original.status);
    expect(restored.marketSwapUuid, 'swap-uuid');
    expect(restored.plan.marketArrrAmount, original.plan.marketArrrAmount);
  });

  test('waiting-for-deposit intents expose their expiration window', () {
    final createdAt = DateTime.utc(2026, 5, 24, 12);
    final pending = intent(
      'pending',
      SwapIntentStatus.waitingForDeposit,
      createdAt: createdAt,
    );

    expect(pending.depositExpiresAt, createdAt.add(swapDepositWindow));
    expect(
      pending.isDepositExpired(createdAt.add(const Duration(minutes: 44))),
      isFalse,
    );
    expect(pending.isDepositExpired(createdAt.add(swapDepositWindow)), isTrue);
  });
}
