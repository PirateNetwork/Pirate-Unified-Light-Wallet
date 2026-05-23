import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'atomic_swap_service.dart';
import 'kdf_swap_engine.dart';
import 'secure_swap_intent_store.dart';
import 'swap_intent_store.dart';
import 'swap_quote_engine.dart';

final swapIntentStoreProvider = Provider<SwapIntentStore>(
  (ref) => SecureStorageSwapIntentStore(),
);

final swapQuoteEngineProvider = Provider<SwapQuoteEngine>(
  (ref) => const SwapQuoteEngine(),
);

final kdfSwapEngineProvider = Provider<KdfSwapEngine>((ref) {
  final engine = KdfSwapEngine();
  ref.onDispose(() => unawaited(engine.dispose()));
  return engine;
});

final atomicSwapServiceProvider = Provider<AtomicSwapService>((ref) {
  return AtomicSwapService(
    engine: ref.watch(kdfSwapEngineProvider),
    intentStore: ref.watch(swapIntentStoreProvider),
    quoteEngine: ref.watch(swapQuoteEngineProvider),
  );
});
