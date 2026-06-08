import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/swaps/kdf_swap_engine.dart';
import 'package:pirate_wallet/core/swaps/swap_models.dart';
import 'package:pirate_wallet/core/swaps/swap_providers.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:test/test.dart';

const _walletId = 'wallet-for-kdf-warmup';
const _otherWalletId = 'second-wallet-for-kdf-warmup';
const _walletMeta = WalletMeta(
  id: _walletId,
  name: 'KDF Warmup Wallet',
  createdAt: 0,
  watchOnly: false,
  birthdayHeight: 0,
  networkType: 'mainnet',
);

void main() {
  setUp(() {
    _UnlockedNotifier.initialUnlocked = true;
    _TestTunnelModeNotifier.mode = const TunnelMode.direct();
    _TestTorStatusNotifier.initialStatus = const TorStatusDetails(
      status: 'ready',
    );
  });

  test('retries KDF warmup and activates supported swap pairs', () async {
    final engine = _FakeKdfSwapEngine(failuresBeforeStart: 1);
    final container = _buildContainer(engine);
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);

    await _waitFor(() => engine.ensureStartedCalls == 1);
    expect(engine.activatedPairs, isEmpty);

    await _waitFor(
      () => engine.activatedPairs.toSet().containsAll(SwapPair.values),
    );
    expect(engine.ensureStartedCalls, greaterThanOrEqualTo(2));
    expect(engine.activatedPairs.toSet(), equals(SwapPair.values.toSet()));
  });

  test('starts KDF warmup when the wallet unlocks', () async {
    _UnlockedNotifier.initialUnlocked = false;
    final engine = _FakeKdfSwapEngine();
    final container = _buildContainer(engine);
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);
    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(engine.ensureStartedCalls, 0);

    final unlockedNotifier =
        container.read(appUnlockedProvider.notifier) as _UnlockedNotifier;
    unlockedNotifier.unlock();

    await _waitFor(() => engine.ensureStartedCalls == 1);
    expect(engine.activatedPairs.toSet(), equals(SwapPair.values.toSet()));
  });

  test('starts KDF warmup when the active wallet loads later', () async {
    final engine = _FakeKdfSwapEngine();
    final container = _buildContainer(
      engine,
      activeWalletBuilder: _DelayedActiveWalletNotifier.new,
    );
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);
    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(engine.ensureStartedCalls, 0);

    final activeWalletNotifier =
        container.read(activeWalletProvider.notifier)
            as _DelayedActiveWalletNotifier;
    activeWalletNotifier.walletId = _walletId;

    await _waitFor(
      () => engine.activatedPairs.toSet().containsAll(SwapPair.values),
    );
    expect(engine.ensureStartedCalls, greaterThanOrEqualTo(1));
    expect(engine.activatedPairs.toSet(), equals(SwapPair.values.toSet()));
  });

  test('cancels stale KDF retry when the active wallet changes', () async {
    final engine = _FakeKdfSwapEngine(
      failuresBeforeStart: 1,
      allowedWalletIds: {_walletId, _otherWalletId},
    );
    final container = _buildContainer(
      engine,
      activeWalletBuilder: _MutableActiveWalletNotifier.new,
    );
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);

    await _waitFor(() => engine.startedWalletIds.length == 1);
    expect(engine.startedWalletIds.single, _walletId);

    final activeWalletNotifier =
        container.read(activeWalletProvider.notifier)
            as _MutableActiveWalletNotifier;
    activeWalletNotifier.walletId = _otherWalletId;

    await _waitFor(() => engine.startedWalletIds.contains(_otherWalletId));
    await Future<void>.delayed(const Duration(milliseconds: 2300));

    expect(
      engine.startedWalletIds.where((walletId) => walletId == _walletId),
      hasLength(1),
    );
    expect(
      engine.startedWalletIds.where((walletId) => walletId == _otherWalletId),
      hasLength(1),
    );
  });

  test('does not dispose KDF while wallet metadata is loading', () async {
    final engine = _FakeKdfSwapEngine();
    final container = _buildContainer(engine, walletMeta: null);
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);

    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(engine.ensureStartedCalls, 0);
    expect(engine.disposeCalls, 0);
  });

  test('disposes KDF when the wallet locks', () async {
    final engine = _FakeKdfSwapEngine();
    final container = _buildContainer(engine);
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);

    await _waitFor(() => engine.ensureStartedCalls == 1);

    final unlockedNotifier =
        container.read(appUnlockedProvider.notifier) as _UnlockedNotifier;
    unlockedNotifier.lock();

    await _waitFor(() => engine.disposeCalls >= 1);
  });

  test('pair warmup activates only the requested pair', () async {
    final engine = _FakeKdfSwapEngine();
    final warmup = KdfSwapWarmupService(engine: engine);

    await warmup.warm(_walletId, pair: SwapPair.arrrLtc);

    expect(engine.ensureStartedCalls, 1);
    expect(engine.activatedPairs, [SwapPair.arrrLtc]);
  });

  test('waits for Tor readiness before starting KDF', () async {
    _TestTunnelModeNotifier.mode = const TunnelMode.tor();
    _TestTorStatusNotifier.initialStatus = const TorStatusDetails(
      status: 'bootstrapping',
    );
    final engine = _FakeKdfSwapEngine();
    final container = _buildContainer(engine);
    addTearDown(container.dispose);

    final subscription = container.listen<void>(
      kdfSwapWarmupProvider,
      (_, _) {},
      fireImmediately: true,
    );
    addTearDown(subscription.close);
    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(engine.ensureStartedCalls, 0);

    final torNotifier =
        container.read(torStatusProvider.notifier) as _TestTorStatusNotifier;
    torNotifier.setReady();

    await _waitFor(() => engine.ensureStartedCalls == 1);
    expect(engine.activatedPairs.toSet(), equals(SwapPair.values.toSet()));
  });
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

ProviderContainer _buildContainer(
  _FakeKdfSwapEngine engine, {
  WalletMeta? walletMeta = _walletMeta,
  ActiveWalletNotifier Function()? activeWalletBuilder,
}) {
  return ProviderContainer(
    overrides: [
      activeWalletProvider.overrideWith(
        activeWalletBuilder ?? _TestActiveWalletNotifier.new,
      ),
      activeWalletMetaProvider.overrideWith((ref) => walletMeta),
      appUnlockedProvider.overrideWith(_UnlockedNotifier.new),
      decoyModeProvider.overrideWith(_DecoyModeNotifier.new),
      walletsProvider.overrideWith((ref) async {
        return const [_walletMeta];
      }),
      allowKomodoSwapApisProvider.overrideWith((ref) => true),
      tunnelModeProvider.overrideWith(_TestTunnelModeNotifier.new),
      torStatusProvider.overrideWith(_TestTorStatusNotifier.new),
      kdfSwapEngineProvider.overrideWith((ref) => engine),
    ],
  );
}

class _FakeKdfSwapEngine extends KdfSwapEngine {
  _FakeKdfSwapEngine({
    this.failuresBeforeStart = 0,
    Set<String>? allowedWalletIds,
  }) : allowedWalletIds = allowedWalletIds ?? const {_walletId};

  int failuresBeforeStart;
  final Set<String> allowedWalletIds;
  int ensureStartedCalls = 0;
  int disposeCalls = 0;
  final List<String> startedWalletIds = [];
  final List<SwapPair> activatedPairs = [];

  @override
  Future<void> ensureStarted(String walletId) async {
    ensureStartedCalls += 1;
    startedWalletIds.add(walletId);
    expect(allowedWalletIds, contains(walletId));
    if (failuresBeforeStart > 0) {
      failuresBeforeStart -= 1;
      throw const KdfSwapEngineException('KDF native engine is still starting');
    }
  }

  @override
  Future<void> activatePair(SwapPair pair) async {
    activatedPairs.add(pair);
  }

  @override
  Future<void> dispose() async {
    disposeCalls += 1;
  }
}

class _TestActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _walletId;
}

class _DelayedActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => null;

  WalletId? get walletId => state;

  set walletId(WalletId? value) {
    state = value;
  }
}

class _MutableActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _walletId;

  WalletId? get walletId => state;

  set walletId(WalletId? value) {
    state = value;
  }
}

class _UnlockedNotifier extends AppUnlockedNotifier {
  static bool initialUnlocked = true;

  @override
  bool build() => initialUnlocked;

  void unlock() {
    state = true;
  }

  void lock() {
    state = false;
  }
}

class _DecoyModeNotifier extends DecoyModeNotifier {
  @override
  bool build() => false;
}

class _TestTunnelModeNotifier extends TunnelModeNotifier {
  static TunnelMode mode = const TunnelMode.direct();

  @override
  TunnelMode build() => mode;
}

class _TestTorStatusNotifier extends TorStatusNotifier {
  static TorStatusDetails initialStatus = const TorStatusDetails(
    status: 'ready',
  );

  @override
  TorStatusDetails build() => initialStatus;

  void setReady() {
    state = const TorStatusDetails(status: 'ready');
  }
}
