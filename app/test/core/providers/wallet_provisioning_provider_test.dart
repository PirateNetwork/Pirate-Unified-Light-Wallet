import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';

void main() {
  test(
    'restore replaces a cached empty wallet list before returning',
    () async {
      final events = <String>[];
      final api = _FakeWalletProvisioningApi(events);
      final container = _container(api: api, events: events);
      addTearDown(container.dispose);

      expect(await container.read(walletsProvider.future), isEmpty);
      expect(await container.read(walletsExistProvider.future), isFalse);
      events.clear();

      final walletId = await container.read(restoreWalletProvider)(
        name: 'Restored Wallet',
        mnemonic: 'test mnemonic',
        birthday: 3_500_000,
      );

      expect(walletId, _walletId);
      expect(container.read(activeWalletProvider), _walletId);
      expect(
        (await container.read(walletsProvider.future))
            .map((wallet) => wallet.id),
        contains(_walletId),
      );
      expect(await container.read(walletsExistProvider.future), isTrue);
      expect(events, ['restore', 'activate', 'list-wallets', 'wallets-exist']);
    },
  );

  test('restore waits for active-wallet selection before refreshing', () async {
    final events = <String>[];
    final api = _FakeWalletProvisioningApi(events);
    final activationStarted = Completer<void>();
    final releaseActivation = Completer<void>();
    final container = _container(
      api: api,
      events: events,
      activationStarted: activationStarted,
      releaseActivation: releaseActivation,
    );
    addTearDown(container.dispose);

    await container.read(walletsProvider.future);
    await container.read(walletsExistProvider.future);
    events.clear();

    var completed = false;
    final restore = container
        .read(restoreWalletProvider)(
          name: 'Restored Wallet',
          mnemonic: 'test mnemonic',
          birthday: 3_500_000,
        )
        .whenComplete(() => completed = true);

    await activationStarted.future;
    expect(completed, isFalse);
    expect(events, ['restore', 'activate']);

    releaseActivation.complete();
    await restore;

    expect(completed, isTrue);
    expect(events, ['restore', 'activate', 'list-wallets', 'wallets-exist']);
  });

  test('finalization tolerates a briefly stale wallet registry', () async {
    final events = <String>[];
    final api = _FakeWalletProvisioningApi(events);
    final container = _container(
      api: api,
      events: events,
      hiddenRegistryReads: 2,
    );
    addTearDown(container.dispose);

    final walletId = await container.read(restoreWalletProvider)(
      name: 'Restored Wallet',
      mnemonic: 'test mnemonic',
      birthday: 3_500_000,
    );

    expect(walletId, _walletId);
    expect(events.where((event) => event == 'list-wallets'), hasLength(3));
    expect(await container.read(walletsExistProvider.future), isTrue);
  });
}

const _walletId = 'restored-wallet-id';

ProviderContainer _container({
  required _FakeWalletProvisioningApi api,
  required List<String> events,
  Completer<void>? activationStarted,
  Completer<void>? releaseActivation,
  int hiddenRegistryReads = 0,
}) {
  var registryReads = 0;
  return ProviderContainer(
    overrides: [
      walletProvisioningApiProvider.overrideWithValue(api),
      activeWalletProvider.overrideWith(
        () => _RecordingActiveWalletNotifier(
          events,
          activationStarted: activationStarted,
          releaseActivation: releaseActivation,
        ),
      ),
      walletsProvider.overrideWith((ref) async {
        events.add('list-wallets');
        registryReads += 1;
        if (registryReads <= hiddenRegistryReads) {
          return const <WalletMeta>[];
        }
        return List<WalletMeta>.unmodifiable(api.wallets);
      }),
      walletsExistProvider.overrideWith((ref) async {
        events.add('wallets-exist');
        return api.wallets.isNotEmpty;
      }),
      walletProvisioningRefreshDelaysProvider.overrideWithValue(
        const <Duration>[
          Duration.zero,
          Duration.zero,
          Duration.zero,
          Duration.zero,
        ],
      ),
    ],
  );
}

class _RecordingActiveWalletNotifier extends ActiveWalletNotifier {
  _RecordingActiveWalletNotifier(
    this.events, {
    this.activationStarted,
    this.releaseActivation,
  });

  final List<String> events;
  final Completer<void>? activationStarted;
  final Completer<void>? releaseActivation;

  @override
  String? build() => null;

  @override
  Future<void> setActiveWallet(String id) async {
    events.add('activate');
    activationStarted?.complete();
    await releaseActivation?.future;
    state = id;
  }
}

class _FakeWalletProvisioningApi extends WalletProvisioningApi {
  _FakeWalletProvisioningApi(this.events);

  final List<String> events;
  final List<WalletMeta> wallets = [];

  @override
  Future<String> restoreWallet({
    required String name,
    required String mnemonic,
    int? birthday,
    MnemonicLanguage? mnemonicLanguage,
  }) async {
    events.add('restore');
    wallets.add(
      WalletMeta(
        id: _walletId,
        name: name,
        createdAt: 1,
        watchOnly: false,
        birthdayHeight: birthday ?? 1,
        networkType: 'mainnet',
      ),
    );
    return _walletId;
  }

  @override
  Future<String> createWallet({
    required String name,
    required int entropyLen,
    int? birthday,
    MnemonicLanguage? mnemonicLanguage,
  }) {
    throw UnimplementedError();
  }

  @override
  Future<String> importViewingWallet({
    required String name,
    String? saplingViewingKey,
    String? ironwoodViewingKey,
    required int birthday,
  }) {
    throw UnimplementedError();
  }
}
