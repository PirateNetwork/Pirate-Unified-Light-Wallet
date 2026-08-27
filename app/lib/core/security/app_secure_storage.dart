import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Secure storage policy shared by every application-owned secret.
///
/// Portable macOS builds are ad-hoc signed and therefore do not have a
/// provisioning profile that can grant a Data Protection Keychain access
/// group. The standard macOS Keychain remains encrypted and available without
/// that entitlement. Other platforms retain the plugin defaults.
const appMacOsSecureStorageOptions = MacOsOptions(
  usesDataProtectionKeychain: false,
);

const appSecureStorage = FlutterSecureStorage(
  mOptions: appMacOsSecureStorageOptions,
);
