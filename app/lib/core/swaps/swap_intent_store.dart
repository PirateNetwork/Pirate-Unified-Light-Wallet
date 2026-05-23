import 'swap_models.dart';

abstract class SwapIntentStore {
  Future<List<SwapIntent>> load(String walletId);
  Future<void> save(String walletId, List<SwapIntent> intents);

  Future<void> upsert(SwapIntent intent) async {
    final intents = await load(intent.walletId);
    final index = intents.indexWhere((entry) => entry.id == intent.id);
    if (index >= 0) {
      intents[index] = intent;
    } else {
      intents.add(intent);
    }
    await save(intent.walletId, intents);
  }

  Future<void> remove(String walletId, String intentId) async {
    final intents = await load(walletId);
    intents.removeWhere((intent) => intent.id == intentId);
    await save(walletId, intents);
  }
}

class InMemorySwapIntentStore extends SwapIntentStore {
  final Map<String, List<SwapIntent>> _entries = <String, List<SwapIntent>>{};

  @override
  Future<List<SwapIntent>> load(String walletId) async {
    return List<SwapIntent>.from(_entries[walletId] ?? const <SwapIntent>[]);
  }

  @override
  Future<void> save(String walletId, List<SwapIntent> intents) async {
    _entries[walletId] = List<SwapIntent>.from(intents);
  }
}
