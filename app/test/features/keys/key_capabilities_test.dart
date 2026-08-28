import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/features/keys/key_capabilities.dart';

KeyGroupInfo _key({
  required KeyTypeInfo type,
  int? accountIndex,
  bool sapling = false,
  bool ironwood = false,
  bool spendable = true,
}) {
  return KeyGroupInfo(
    id: 1,
    keyType: type,
    seedAccountIndex: accountIndex,
    spendable: spendable,
    hasSapling: sapling,
    hasIronwood: ironwood,
    birthdayHeight: 1,
    createdAt: 1,
  );
}

void main() {
  test('only recovery-phrase key groups enable seed account derivation', () {
    final seed = _key(
      type: KeyTypeInfo.seed,
      accountIndex: 0,
      sapling: true,
      ironwood: true,
    );
    final importedSpending = _key(
      type: KeyTypeInfo.importedSpending,
      sapling: true,
    );
    final importedViewing = _key(
      type: KeyTypeInfo.importedViewing,
      ironwood: true,
      spendable: false,
    );

    expect([seed].supportsSeedAccountDerivation, isTrue);
    expect(
      [importedSpending, importedViewing].supportsSeedAccountDerivation,
      isFalse,
    );
    expect(importedSpending.isRecoveryPhraseAccount, isFalse);
    expect(importedViewing.isRecoveryPhraseAccount, isFalse);
  });

  test('imported spending and viewing keys retain address derivation', () {
    final importedSpending = _key(
      type: KeyTypeInfo.importedSpending,
      sapling: true,
    );
    final importedViewing = _key(
      type: KeyTypeInfo.importedViewing,
      ironwood: true,
      spendable: false,
    );

    expect(importedSpending.canGenerateSaplingAddresses, isTrue);
    expect(importedSpending.canGenerateIronwoodAddresses, isFalse);
    expect(importedViewing.canGenerateSaplingAddresses, isFalse);
    expect(importedViewing.canGenerateIronwoodAddresses, isTrue);
  });
}
