import 'dart:async';

import 'package:pirate_wallet/core/swaps/kdf_swap_engine.dart';
import 'package:test/test.dart';

void main() {
  group('KDF swap startup coordination', () {
    test(
      'reuses an in-flight startup for the same wallet and network',
      () async {
        final starter = _StartupRecorder();
        final engine = KdfSwapEngine(startupForTesting: starter.start);
        addTearDown(engine.dispose);

        final first = engine.ensureStarted('wallet-a');
        await Future<void>.delayed(Duration.zero);
        final second = engine.ensureStarted('wallet-a');
        await Future<void>.delayed(Duration.zero);

        expect(starter.starts, hasLength(1));
        expect(starter.starts.single.walletId, 'wallet-a');

        starter.complete(0);
        await Future.wait([first, second]);
      },
    );

    test('supersedes an in-flight startup when wallet changes', () async {
      final starter = _StartupRecorder();
      final engine = KdfSwapEngine(startupForTesting: starter.start);
      addTearDown(engine.dispose);

      final first = engine.ensureStarted('wallet-a');
      await Future<void>.delayed(Duration.zero);
      final second = engine.ensureStarted('wallet-b');
      await Future<void>.delayed(Duration.zero);

      expect(starter.starts, hasLength(2));
      expect(starter.starts[0].walletId, 'wallet-a');
      expect(starter.starts[0].generation, 1);
      expect(starter.starts[1].walletId, 'wallet-b');
      expect(starter.starts[1].generation, 2);

      starter.complete(1);
      await second;
      starter.complete(0);
      await first;
    });

    test(
      'supersedes an in-flight startup when network policy changes',
      () async {
        var policy = const KdfSwapNetworkPolicy.direct();
        final starter = _StartupRecorder();
        final engine = KdfSwapEngine(
          networkPolicyReader: () => policy,
          startupForTesting: starter.start,
        );
        addTearDown(engine.dispose);

        final first = engine.ensureStarted('wallet-a');
        await Future<void>.delayed(Duration.zero);
        policy = const KdfSwapNetworkPolicy.tor();
        final second = engine.ensureStarted('wallet-a');
        await Future<void>.delayed(Duration.zero);

        expect(starter.starts, hasLength(2));
        expect(starter.starts[0].policyName, 'direct');
        expect(starter.starts[1].policyName, 'tor');
        expect(starter.starts[1].generation, 2);

        starter.complete(1);
        await second;
        starter.complete(0);
        await first;
      },
    );
  });

  group('KDF swap error helpers', () {
    test('detects structured insufficient balance errors', () {
      final error = KdfSwapEngineException('trade_preimage failed', {
        'error_type': 'NotSufficientBalance',
        'error_data': {'coin': 'LTC', 'available': '0', 'required': '0.001'},
      });

      expect(isKdfInsufficientBalanceError(error, coin: 'LTC'), isTrue);
      expect(isKdfInsufficientBalanceError(error, coin: 'ARRR'), isFalse);
    });

    test('detects wrapped HTTP insufficient balance errors', () {
      final error = KdfSwapEngineException('trade_preimage failed', {
        'code': 400,
        'message':
            '{"error_type":"NotSufficientBalance","error_data":{"coin":"LTC"}}',
      });

      expect(isKdfInsufficientBalanceError(error, coin: 'LTC'), isTrue);
    });
  });
}

class _StartupRecorder {
  final List<_StartupCall> starts = [];
  final List<Completer<void>> completers = [];

  Future<void> start(
    String walletId,
    KdfSwapNetworkPolicy policy,
    int generation,
  ) {
    starts.add(
      _StartupCall(
        walletId: walletId,
        policyName: policy.name,
        generation: generation,
      ),
    );
    final completer = Completer<void>();
    completers.add(completer);
    return completer.future;
  }

  void complete(int index) {
    completers[index].complete();
  }
}

class _StartupCall {
  const _StartupCall({
    required this.walletId,
    required this.policyName,
    required this.generation,
  });

  final String walletId;
  final String policyName;
  final int generation;
}
