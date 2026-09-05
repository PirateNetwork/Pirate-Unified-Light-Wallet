import 'package:mocktail/mocktail.dart';
import 'package:pirate_wallet/core/ffi/generated/api.dart';
import 'package:pirate_wallet/core/ffi/generated/frb_generated.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';

/// Deterministic legacy restore data. No real wallet or network is accessed.
class RestoredWalletApi extends Fake implements RustLibApi {
  int visibleKeyCount = 3;
  int keyListReads = 0;
  final generatedKeyIds = <int>[];
  final keys = [
    for (var index = 0; index < 3; index++)
      KeyGroupInfo(
        id: index + 1,
        label: null,
        keyType: KeyTypeInfo.seed,
        seedAccountIndex: index,
        spendable: true,
        hasSapling: true,
        hasIronwood: false,
        birthdayHeight: 100,
        createdAt: 1788220800,
      ),
  ];
  final addresses = <int, String>{1: 'zs1sampleprimaryaddressnotforpayments'};

  @override
  Future<List<KeyGroupInfo>> crateApiListKeyGroups({
    required String walletId,
  }) async {
    keyListReads++;
    return keys.take(visibleKeyCount).toList();
  }

  @override
  Future<String> crateApiCurrentReceiveAddress({
    required String walletId,
  }) async => addresses[1]!;

  @override
  Future<List<KeyAddressInfo>> crateApiListAddressesForKey({
    required String walletId,
    required int keyId,
  }) async => [
    if (addresses[keyId] case final address?)
      KeyAddressInfo(
        keyId: keyId,
        address: address,
        diversifierIndex: 0,
        createdAt: 1788220800,
        colorTag: AddressBookColorTag.none,
      ),
  ];

  @override
  Future<String> crateApiGenerateAddressForKey({
    required String walletId,
    required int keyId,
    required bool useIronwood,
  }) async {
    generatedKeyIds.add(keyId);
    return addresses[keyId] =
        'zs1samplerestoredaccount${keyId}addressnotforpayments';
  }

  @override
  Future<List<AddressBalanceInfo>> crateApiListAddressBalances({
    required String walletId,
    int? keyId,
  }) async => [
    for (final entry in addresses.entries)
      if (keyId == null || keyId == entry.key)
        AddressBalanceInfo(
          address: entry.value,
          keyId: entry.key,
          addressId: entry.key,
          diversifierIndex: 0,
          createdAt: 1788220800,
          balance: BigInt.from(entry.key * 2500000000),
          spendable: BigInt.from(entry.key * 2500000000),
          pending: BigInt.zero,
          colorTag: AddressBookColorTag.none,
        ),
  ];

  @override
  Future<List<AddressDisplayPreferenceInfo>>
  crateApiListAddressDisplayPreferences({required String walletId}) async => [];

  @override
  Future<List<AddressBookEntryFfi>> crateApiListAddressBook({
    required String walletId,
  }) async => [];

  @override
  Future<FeeInfo> crateApiGetFeeInfo() async => FeeInfo(
    defaultFee: BigInt.from(10000),
    minFee: BigInt.from(10000),
    maxFee: BigInt.from(1000000),
    feePerOutput: BigInt.zero,
    memoFeeMultiplier: 1,
  );
}
