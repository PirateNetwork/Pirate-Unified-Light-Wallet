/// Platform channel for OS keystore integration
///
/// Handles communication with native keystores:
/// - Android Keystore/StrongBox
/// - iOS Secure Enclave/Keychain
/// - macOS Keychain
/// - Windows DPAPI
/// - Linux libsecret

import 'dart:io';
import 'package:flutter/services.dart';

/// OS Keystore manager
class KeystoreChannel {
  static const MethodChannel _channel = MethodChannel('com.pirate.wallet/keystore');

  /// Store sealed key in OS keystore
  static Future<bool> storeKey({
    required String keyId,
    required List<int> encryptedKey,
  }) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'storeKey',
        {
          'keyId': keyId,
          'encryptedKey': encryptedKey,
        },
      );
      return result ?? false;
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to store key: ${e.message}');
    }
  }

  /// Retrieve sealed key from OS keystore
  static Future<List<int>?> retrieveKey(String keyId) async {
    try {
      final result = await _channel.invokeMethod<List<dynamic>>(
        'retrieveKey',
        {'keyId': keyId},
      );
      return result?.cast<int>();
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to retrieve key: ${e.message}');
    }
  }

  /// Delete key from OS keystore
  static Future<bool> deleteKey(String keyId) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'deleteKey',
        {'keyId': keyId},
      );
      return result ?? false;
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to delete key: ${e.message}');
    }
  }

  /// Check if key exists
  static Future<bool> keyExists(String keyId) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'keyExists',
        {'keyId': keyId},
      );
      return result ?? false;
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to check key: ${e.message}');
    }
  }

  /// Seal master key with OS keystore
  /// 
  /// On Android: Uses Keystore with StrongBox if available
  /// On iOS: Uses Secure Enclave if available, fallback to Keychain
  /// On macOS: Uses Keychain
  /// On Windows: Uses DPAPI
  /// On Linux: Uses libsecret
  static Future<List<int>> sealMasterKey(List<int> masterKey) async {
    try {
      final result = await _channel.invokeMethod<List<dynamic>>(
        'sealMasterKey',
        {'masterKey': masterKey},
      );
      if (result == null) {
        throw KeystoreException('Failed to seal master key');
      }
      return result.cast<int>();
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to seal master key: ${e.message}');
    }
  }

  /// Unseal master key from OS keystore
  static Future<List<int>> unsealMasterKey(List<int> sealedKey) async {
    try {
      final result = await _channel.invokeMethod<List<dynamic>>(
        'unsealMasterKey',
        {'sealedKey': sealedKey},
      );
      if (result == null) {
        throw KeystoreException('Failed to unseal master key');
      }
      return result.cast<int>();
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to unseal master key: ${e.message}');
    }
  }

  /// Get keystore capabilities for current platform
  static Future<KeystoreCapabilities> getCapabilities() async {
    try {
      final result = await _channel.invokeMethod<Map<dynamic, dynamic>>(
        'getCapabilities',
      );
      
      if (result == null) {
        return KeystoreCapabilities.none();
      }
      
      return KeystoreCapabilities(
        hasSecureHardware: result['hasSecureHardware'] as bool? ?? false,
        hasStrongBox: result['hasStrongBox'] as bool? ?? false,
        hasSecureEnclave: result['hasSecureEnclave'] as bool? ?? false,
        hasBiometrics: result['hasBiometrics'] as bool? ?? false,
      );
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to get capabilities: ${e.message}');
    }
  }

  /// Enable or disable biometric-gated keystore access on supported platforms.
  static Future<void> setBiometricsEnabled(bool enabled) async {
    if (!Platform.isAndroid && !Platform.isIOS) {
      return;
    }
    try {
      await _channel.invokeMethod<bool>(
        'setBiometricsEnabled',
        {'enabled': enabled},
      );
    } on PlatformException catch (e) {
      throw KeystoreException('Failed to update biometrics: ${e.message}');
    }
  }

  /// Check if platform supports secure storage
  static bool get isPlatformSupported {
    return Platform.isAndroid ||
        Platform.isIOS ||
        Platform.isMacOS ||
        Platform.isWindows ||
        Platform.isLinux;
  }
}

/// Keystore capabilities
class KeystoreCapabilities {
  /// Has secure hardware (TEE/Secure Enclave)
  final bool hasSecureHardware;
  
  /// Has StrongBox (Android)
  final bool hasStrongBox;
  
  /// Has Secure Enclave (iOS/macOS)
  final bool hasSecureEnclave;
  
  /// Has biometric authentication
  final bool hasBiometrics;

  KeystoreCapabilities({
    required this.hasSecureHardware,
    required this.hasStrongBox,
    required this.hasSecureEnclave,
    required this.hasBiometrics,
  });

  factory KeystoreCapabilities.none() {
    return KeystoreCapabilities(
      hasSecureHardware: false,
      hasStrongBox: false,
      hasSecureEnclave: false,
      hasBiometrics: false,
    );
  }

  String get description {
    final features = <String>[];
    if (hasStrongBox) features.add('StrongBox');
    if (hasSecureEnclave) features.add('Secure Enclave');
    if (hasSecureHardware) features.add('Secure Hardware');
    if (hasBiometrics) features.add('Biometrics');
    
    return features.isEmpty ? 'No secure hardware' : features.join(', ');
  }
}

/// Keystore exception
class KeystoreException implements Exception {
  final String message;
  
  KeystoreException(this.message);
  
  @override
  String toString() => 'KeystoreException: $message';
}

