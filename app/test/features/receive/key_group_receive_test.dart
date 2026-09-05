import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/core/ffi/generated/frb_generated.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/features/receive/receive_viewmodel.dart';

import '../../support/restored_wallet_api.dart';

class _Wallet extends ActiveWalletNotifier {
  @override
  String? build() => 'restored-test-wallet';
}

class _NormalMode extends DecoyModeNotifier {
  @override
  bool build() => false;
}

KeyGroupInfo group({
  int index = 1,
  bool spendable = true,
  bool sapling = true,
  bool ironwood = false,
  KeyTypeInfo type = KeyTypeInfo.seed,
}) => KeyGroupInfo(
  id: 10 + index,
  label: null,
  keyType: type,
  seedAccountIndex: index,
  spendable: spendable,
  hasSapling: sapling,
  hasIronwood: ironwood,
  birthdayHeight: 1,
  createdAt: 1,
);

void main() {
  test(
    'restored groups prepare addresses once and preserve their identity',
    () async {
      final api = RestoredWalletApi();
      RustLib.initMock(api: api);
      final container = ProviderContainer.test(
        overrides: [
          activeWalletProvider.overrideWith(_Wallet.new),
          decoyModeProvider.overrideWith(_NormalMode.new),
        ],
      );
      container.listen(receiveViewModelProvider, (_, _) {});
      // Drain initialization, including the mocked bridge calls.
      await Future<void>.delayed(Duration.zero);
      final state = container.read(receiveViewModelProvider);
      expect(state.isLoading, isFalse);
      expect(state.addressHistory.map((a) => a.keyId), [1, 2, 3]);
      expect(state.addressHistory.map((a) => a.seedAccountIndex), [0, 1, 2]);
      expect(state.addressHistory.map((a) => a.keyLabel), [
        'Seed account 0',
        'Seed account 1',
        'Seed account 2',
      ]);
      expect(api.generatedKeyIds, [2, 3]);
      await container
          .read(receiveViewModelProvider.notifier)
          .refreshAddressHistory(force: true);
      expect(api.generatedKeyIds, [2, 3]);
      expect(
        container.read(receiveViewModelProvider).currentAddress,
        api.addresses[1],
      );
    },
  );

  test('every restored seed group can prepare its first receive address', () {
    for (var index = 0; index < 7; index++) {
      final key = group(index: index);
      expect(needsReceiveAddressPreparation(key), isTrue);
      expect(receiveKeyGroupLabel(key), 'Seed account $index');
    }
    expect(
      needsReceiveAddressPreparation(group(sapling: false, ironwood: true)),
      isTrue,
    );
    expect(
      needsReceiveAddressPreparation(group(type: KeyTypeInfo.importedSpending)),
      isTrue,
    );
    expect(needsReceiveAddressPreparation(group(spendable: false)), isFalse);
    expect(needsReceiveAddressPreparation(group(sapling: false)), isFalse);
  });
}
