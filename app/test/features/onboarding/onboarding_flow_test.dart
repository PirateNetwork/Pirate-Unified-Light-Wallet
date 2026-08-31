import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart' show WalletId;
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/onboarding/onboarding_flow.dart';

void main() {
  test(
    'retry resumes finalization without creating a duplicate wallet',
    () async {
      final api = _RecordingProvisioningApi();
      var finalizationAttempts = 0;
      final container = ProviderContainer(
        overrides: [
          walletProvisioningApiProvider.overrideWithValue(api),
          finalizeWalletProvisioningProvider.overrideWithValue((
            walletId,
          ) async {
            finalizationAttempts += 1;
            if (finalizationAttempts == 1) {
              throw StateError('transient refresh failure');
            }
          }),
        ],
      );
      addTearDown(container.dispose);

      final controller = container.read(onboardingControllerProvider.notifier)
        ..setMode(OnboardingMode.create)
        ..setMnemonic('test mnemonic')
        ..setBirthdayHeight(3_500_000);

      await expectLater(
        controller.complete('My ARRR Wallet 1'),
        throwsStateError,
      );
      expect(api.restoreCalls, 1);

      await controller.complete('My ARRR Wallet 1');

      expect(api.restoreCalls, 1);
      expect(finalizationAttempts, 2);
      expect(
        container.read(onboardingControllerProvider).currentStep,
        OnboardingStep.complete,
      );
    },
  );
}

class _RecordingProvisioningApi extends WalletProvisioningApi {
  int restoreCalls = 0;

  @override
  Future<WalletId> restoreWallet({
    required String name,
    required String mnemonic,
    int? birthday,
    MnemonicLanguage? mnemonicLanguage,
  }) async {
    restoreCalls += 1;
    return 'wallet-id';
  }

  @override
  Future<WalletId> createWallet({
    required String name,
    required int entropyLen,
    int? birthday,
    MnemonicLanguage? mnemonicLanguage,
  }) => throw UnimplementedError();

  @override
  Future<WalletId> importViewingWallet({
    required String name,
    String? saplingViewingKey,
    String? ironwoodViewingKey,
    required int birthday,
  }) => throw UnimplementedError();
}
