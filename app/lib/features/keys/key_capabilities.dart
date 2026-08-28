import '../../core/ffi/generated/models.dart';

/// Capabilities carried by one persisted key group.
///
/// Recovery-phrase accounts and diversified addresses are different levels of
/// the key hierarchy. An imported account key can derive addresses inside that
/// account, but it cannot recover sibling accounts without the parent seed.
extension KeyGroupCapabilities on KeyGroupInfo {
  bool get isRecoveryPhraseAccount =>
      keyType == KeyTypeInfo.seed && seedAccountIndex != null;

  bool get canGenerateSaplingAddresses => hasSapling;

  bool get canGenerateIronwoodAddresses => hasIronwood;
}

extension WalletKeyCapabilities on Iterable<KeyGroupInfo> {
  bool get supportsSeedAccountDerivation =>
      any((key) => key.isRecoveryPhraseAccount);
}
