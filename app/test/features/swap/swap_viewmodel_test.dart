import 'package:decimal/decimal.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/swaps/atomic_swap_service.dart';
import 'package:pirate_wallet/core/swaps/kdf_swap_engine.dart';
import 'package:pirate_wallet/core/swaps/swap_intent_store.dart';
import 'package:pirate_wallet/core/swaps/swap_models.dart';
import 'package:pirate_wallet/core/swaps/swap_orchestrator.dart';
import 'package:pirate_wallet/core/swaps/swap_providers.dart';
import 'package:pirate_wallet/features/swap/swap_viewmodel.dart';
import 'package:test/test.dart';

void main() {
  group('SwapViewModel bootstrap', () {
    test('bootstraps when the active wallet becomes available later', () async {
      final warmup = _FakeWarmupService();
      final orchestrator = _FakeSwapOrchestrator();
      final container = ProviderContainer(
        overrides: [
          activeWalletProvider.overrideWith(_DelayedActiveWalletNotifier.new),
          kdfSwapWarmupServiceProvider.overrideWith((ref) => warmup),
          swapOrchestratorProvider.overrideWith((ref) => orchestrator),
        ],
      );
      addTearDown(container.dispose);

      final subscription = container.listen<SwapViewModelState>(
        swapViewModelProvider,
        (_, _) {},
        fireImmediately: true,
      );
      addTearDown(subscription.close);

      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(warmup.warmedWallets, isEmpty);
      expect(orchestrator.resumeCalls, 0);

      final activeWallet =
          container.read(activeWalletProvider.notifier)
              as _DelayedActiveWalletNotifier;
      activeWallet.walletId = _walletId;

      await _waitFor(() => orchestrator.resumeCalls == 1);
      expect(warmup.warmedWallets, contains(_walletId));
    });

    test('retries bootstrap when warmup initially fails', () async {
      final warmup = _FakeWarmupService(failuresBeforeWarm: 1);
      final orchestrator = _FakeSwapOrchestrator();
      final container = ProviderContainer(
        overrides: [
          activeWalletProvider.overrideWith(_ImmediateActiveWalletNotifier.new),
          kdfSwapWarmupServiceProvider.overrideWith((ref) => warmup),
          swapOrchestratorProvider.overrideWith((ref) => orchestrator),
        ],
      );
      addTearDown(container.dispose);

      final subscription = container.listen<SwapViewModelState>(
        swapViewModelProvider,
        (_, _) {},
        fireImmediately: true,
      );
      addTearDown(subscription.close);

      await _waitFor(() => warmup.warmAttempts == 1);
      expect(orchestrator.resumeCalls, 0);

      await _waitFor(() => orchestrator.resumeCalls == 1);
      expect(warmup.warmAttempts, greaterThanOrEqualTo(2));
      expect(warmup.warmedWallets, contains(_walletId));
    });
  });

  group('SwapViewModel history details', () {
    test('opens a completed swap intent as a read-only completion view', () {
      final container = ProviderContainer(
        overrides: [
          activeWalletProvider.overrideWith(_DelayedActiveWalletNotifier.new),
          kdfSwapWarmupServiceProvider.overrideWith((ref) {
            return _FakeWarmupService();
          }),
          swapOrchestratorProvider.overrideWith((ref) {
            return _FakeSwapOrchestrator();
          }),
        ],
      );
      addTearDown(container.dispose);

      final subscription = container.listen<SwapViewModelState>(
        swapViewModelProvider,
        (_, _) {},
        fireImmediately: true,
      );
      addTearDown(subscription.close);

      final intent = _completedIntent();
      container.read(swapViewModelProvider.notifier).resumeIntent(intent);

      final state = container.read(swapViewModelProvider);
      expect(state.step, SwapUiStep.complete);
      expect(state.activeIntent, intent);
      expect(state.progress, SwapProgressStage.complete);
      expect(state.completionKind, SwapCompletionKind.swap);
      expect(state.payAmountText, '1.25');
      expect(state.receiveAmountText, '99.5');
    });
  });

  group('SwapViewModel deposit flow', () {
    test('checks for funding before starting a buy swap', () async {
      final orchestrator = _FakeSwapOrchestrator();
      final container = ProviderContainer(
        overrides: [
          activeWalletProvider.overrideWith(_ImmediateActiveWalletNotifier.new),
          kdfSwapWarmupServiceProvider.overrideWith((ref) {
            return _FakeWarmupService();
          }),
          swapOrchestratorProvider.overrideWith((ref) => orchestrator),
        ],
      );
      addTearDown(container.dispose);

      final subscription = container.listen<SwapViewModelState>(
        swapViewModelProvider,
        (_, _) {},
        fireImmediately: true,
      );
      addTearDown(subscription.close);

      final vm = container.read(swapViewModelProvider.notifier);
      final intent = _waitingForDepositIntent();
      vm.resumeIntent(intent);

      await _waitFor(() {
        return container.read(swapViewModelProvider).step == SwapUiStep.deposit;
      });

      await vm.checkDepositOrStartSwap();

      var state = container.read(swapViewModelProvider);
      expect(state.step, SwapUiStep.deposit);
      expect(state.depositDetected, isFalse);
      expect(state.progressMessage, contains('No confirmed LTC detected yet'));
      expect(orchestrator.executeBuyCalls, 0);

      orchestrator.relDepositBalance = intent.requestedPayAmount;
      await vm.checkDepositOrStartSwap();

      await _waitFor(() => orchestrator.executeBuyCalls == 1);
      state = container.read(swapViewModelProvider);
      expect(state.step, SwapUiStep.complete);
      expect(state.progress, SwapProgressStage.complete);
    });
  });

  group('SwapViewModelState', () {
    test('copyWith can clear quote and errors', () {
      const initial = SwapViewModelState(quoteError: 'bad', payAmountText: '1');
      final cleared = initial.copyWith(clearQuote: true, clearErrors: true);
      expect(cleared.quote, isNull);
      expect(cleared.quoteError, isNull);
      expect(cleared.payAmountText, '1');
    });

    test('isBuy reflects side', () {
      const buy = SwapViewModelState(side: SwapSide.buyArrr);
      const sell = SwapViewModelState(side: SwapSide.sellArrr);
      expect(buy.isBuy, isTrue);
      expect(sell.isBuy, isFalse);
    });
  });
}

const _walletId = 'late-active-wallet';

SwapIntent _completedIntent() {
  final now = DateTime.utc(2026, 6, 5, 12);
  return SwapIntent(
    id: 'completed-swap',
    walletId: _walletId,
    side: SwapSide.buyArrr,
    status: SwapIntentStatus.completed,
    createdAt: now.subtract(const Duration(minutes: 10)),
    updatedAt: now,
    plan: SwapPlan(
      side: SwapSide.buyArrr,
      referencePriceLtcPerArrr: Decimal.parse('0.01275510'),
      marketArrrAmount: Decimal.parse('100'),
      marketLtcAmount: Decimal.parse('1.25'),
      remainderLtcAmount: Decimal.zero,
      remainderArrrAmount: Decimal.zero,
      slippageCap: Decimal.parse('0.05'),
      realizedSlippage: Decimal.zero,
      fills: const [],
      appFeeArrrAmount: Decimal.parse('0.37'),
    ),
  );
}

SwapIntent _waitingForDepositIntent() {
  final now = DateTime.now().toUtc();
  return SwapIntent(
    id: 'waiting-swap',
    walletId: _walletId,
    side: SwapSide.buyArrr,
    status: SwapIntentStatus.waitingForDeposit,
    createdAt: now,
    updatedAt: now,
    pair: SwapPair.arrrLtc,
    ltcDepositAddress: 'LUCT7q26Lwc9dSzG4DSZsq3mTCbPqkK6G9',
    plan: SwapPlan(
      side: SwapSide.buyArrr,
      referencePriceLtcPerArrr: Decimal.parse('0.01275510'),
      marketArrrAmount: Decimal.parse('100'),
      marketLtcAmount: Decimal.parse('1.25'),
      remainderLtcAmount: Decimal.zero,
      remainderArrrAmount: Decimal.zero,
      slippageCap: Decimal.parse('0.05'),
      realizedSlippage: Decimal.zero,
      fills: const [],
      appFeeArrrAmount: Decimal.parse('0.37'),
    ),
  );
}

Future<void> _waitFor(
  bool Function() predicate, {
  Duration timeout = const Duration(seconds: 5),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    if (predicate()) return;
    await Future<void>.delayed(const Duration(milliseconds: 25));
  }
  throw TestFailure('Timed out waiting for condition.');
}

class _DelayedActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => null;

  WalletId? get walletId => state;

  set walletId(WalletId? value) {
    state = value;
  }
}

class _ImmediateActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _walletId;
}

class _NoopKdfSwapEngine extends KdfSwapEngine {}

class _FakeWarmupService extends KdfSwapWarmupService {
  _FakeWarmupService({this.failuresBeforeWarm = 0})
    : super(engine: _NoopKdfSwapEngine());

  int failuresBeforeWarm;
  int warmAttempts = 0;
  final List<String> warmedWallets = [];

  @override
  Future<void> warm(String walletId, {SwapPair? pair}) async {
    warmAttempts += 1;
    if (failuresBeforeWarm > 0) {
      failuresBeforeWarm -= 1;
      throw const KdfSwapEngineException('KDF native engine is still starting');
    }
    warmedWallets.add(walletId);
  }
}

class _FakeSwapOrchestrator extends SwapOrchestrator {
  _FakeSwapOrchestrator()
    : super(
        service: AtomicSwapService(
          engine: _NoopKdfSwapEngine(),
          intentStore: InMemorySwapIntentStore(),
        ),
      );

  int resumeCalls = 0;
  int executeBuyCalls = 0;
  Decimal relDepositBalance = Decimal.zero;

  @override
  Future<List<SwapIntent>> resumeIncomplete(String walletId) async {
    resumeCalls += 1;
    return const [];
  }

  @override
  Future<List<SwapIntent>> loadHistory(String walletId) async {
    return const [];
  }

  @override
  Future<Decimal> currentRelDepositBalance(
    String walletId, {
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    return relDepositBalance;
  }

  @override
  Future<void> executeBuyFlow({
    required SwapIntent intent,
    required Decimal requiredLtc,
    SwapProgressCallback? onProgress,
    bool Function()? isCancelled,
  }) async {
    executeBuyCalls += 1;
  }

  @override
  Future<Decimal> currentFundingBalance(
    String walletId, {
    required SwapAsset asset,
  }) async {
    return Decimal.zero;
  }
}
