import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../ffi/generated/models.dart'
    show
        TunnelMode,
        TunnelMode_Direct,
        TunnelMode_I2p,
        TunnelMode_Socks5,
        TunnelMode_Tor;
import '../providers/wallet_providers.dart';
import '../../features/settings/providers/preferences_providers.dart';
import '../../features/settings/providers/transport_providers.dart';
import 'atomic_swap_service.dart';
import 'kdf_swap_engine.dart';
import 'secure_swap_intent_store.dart';
import 'swap_intent_store.dart';
import 'swap_models.dart';
import 'swap_orchestrator.dart';
import 'swap_quote_engine.dart';

final swapIntentStoreProvider = Provider<SwapIntentStore>(
  (ref) => SecureStorageSwapIntentStore(),
);

final swapQuoteEngineProvider = Provider<SwapQuoteEngine>(
  (ref) => const SwapQuoteEngine(),
);

final kdfSwapEngineProvider = Provider<KdfSwapEngine>((ref) {
  final tunnelMode = ref.watch(tunnelModeProvider);
  final swapApisAllowed = ref.watch(allowKomodoSwapApisProvider);
  final policy = swapApisAllowed
      ? _kdfNetworkPolicyForTunnelMode(tunnelMode)
      : const KdfSwapNetworkPolicy.blocked(
          'Komodo Swaps',
          'Komodo swap outbound API calls are disabled in Settings. Enable Komodo Swaps under Outbound API Calls to use swaps.',
        );
  final engine = KdfSwapEngine(networkPolicyReader: () => policy);
  ref.onDispose(() => unawaited(engine.dispose()));
  return engine;
});

KdfSwapNetworkPolicy _kdfNetworkPolicyForTunnelMode(TunnelMode mode) {
  return switch (mode) {
    TunnelMode_Direct() => const KdfSwapNetworkPolicy.direct(),
    TunnelMode_Tor() => const KdfSwapNetworkPolicy.tor(),
    TunnelMode_I2p() => const KdfSwapNetworkPolicy.blocked(
      'I2P',
      'Swaps are not available over I2P yet because we do not have an I2P-compatible KDF/light server route. Switch wallet networking to Tor, SOCKS5, or Direct before using swaps.',
    ),
    TunnelMode_Socks5() => const KdfSwapNetworkPolicy.socks5(),
  };
}

final atomicSwapServiceProvider = Provider<AtomicSwapService>((ref) {
  return AtomicSwapService(
    engine: ref.watch(kdfSwapEngineProvider),
    intentStore: ref.watch(swapIntentStoreProvider),
    quoteEngine: ref.watch(swapQuoteEngineProvider),
  );
});

final swapOrchestratorProvider = Provider<SwapOrchestrator>((ref) {
  return SwapOrchestrator(service: ref.watch(atomicSwapServiceProvider));
});

final kdfSwapWarmupServiceProvider = Provider<KdfSwapWarmupService>((ref) {
  return KdfSwapWarmupService(engine: ref.watch(kdfSwapEngineProvider));
});

final kdfSwapWarmupProvider = Provider<void>((ref) {
  final walletId = ref.watch(activeWalletProvider);
  final walletMeta = ref.watch(activeWalletMetaProvider);
  final isUnlocked = ref.watch(appUnlockedProvider);
  final isDecoy = ref.watch(decoyModeProvider);
  final swapApisAllowed = ref.watch(allowKomodoSwapApisProvider);
  final tunnelMode = ref.watch(tunnelModeProvider);
  final torReady = ref.watch(
    torStatusProvider.select((status) => status.isReady),
  );
  final warmup = ref.watch(kdfSwapWarmupServiceProvider);
  Timer? retryTimer;
  var disposed = false;

  ref.onDispose(() {
    disposed = true;
    retryTimer?.cancel();
  });

  if (walletId == null ||
      (walletMeta?.watchOnly ?? false) ||
      !isUnlocked ||
      isDecoy ||
      !swapApisAllowed ||
      tunnelMode is TunnelMode_I2p) {
    unawaited(warmup.dispose());
    return;
  }

  if (walletMeta == null || (tunnelMode is TunnelMode_Tor && !torReady)) {
    return;
  }

  var retryAttempt = 0;
  void scheduleWarmup(Duration delay) {
    retryTimer?.cancel();
    retryTimer = Timer(delay, () {
      unawaited(() async {
        try {
          await warmup.warmAll(walletId);
        } catch (_) {
          if (disposed) return;
          scheduleWarmup(_kdfWarmupRetryDelay(retryAttempt++));
        }
      }());
    });
  }

  scheduleWarmup(Duration.zero);
});

class KdfSwapWarmupService {
  KdfSwapWarmupService({required KdfSwapEngine engine}) : _engine = engine;

  final KdfSwapEngine _engine;

  Future<void> warm(String walletId, {SwapPair? pair}) async {
    await _engine.ensureStarted(walletId);
    if (pair != null) {
      await _engine.activatePair(pair);
    }
  }

  Future<void> warmAll(String walletId) async {
    await _engine.ensureStarted(walletId);
    for (final pair in SwapPair.values) {
      await _engine.activatePair(pair);
    }
  }

  Future<void> dispose() => _engine.dispose();
}

Duration _kdfWarmupRetryDelay(int attempt) {
  if (attempt < 2) return const Duration(seconds: 2);
  if (attempt < 5) return const Duration(seconds: 5);
  return const Duration(seconds: 15);
}
