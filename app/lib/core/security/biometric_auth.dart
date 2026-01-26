// Biometric authentication
//
// Uses local_auth package for fingerprint/Face ID/Touch ID

import 'package:flutter/services.dart';
import 'package:local_auth/local_auth.dart';

/// Biometric authentication manager
class BiometricAuth {
  static final _auth = LocalAuthentication();

  /// Check if biometrics are available
  static Future<bool> isAvailable() async {
    try {
      return await _auth.canCheckBiometrics || await _auth.isDeviceSupported();
    } catch (e) {
      return false;
    }
  }

  /// Get available biometric types
  static Future<List<BiometricType>> getAvailableBiometrics() async {
    try {
      return await _auth.getAvailableBiometrics();
    } catch (e) {
      return [];
    }
  }

  /// Authenticate with biometrics
  static Future<bool> authenticate({
    String reason = 'Please authenticate to unlock wallet',
    bool biometricOnly = false,
  }) async {
    try {
      // local_auth 3.0.0 API - authenticate without options parameter
      // The options parameter was removed in favor of direct named parameters
      return await _auth.authenticate(
        localizedReason: reason,
      );
    } on PlatformException catch (e) {
      // Handle specific errors
      if (e.code == 'NotAvailable') {
        throw BiometricException('Biometrics not available');
      } else if (e.code == 'NotEnrolled') {
        throw BiometricException('No biometrics enrolled');
      } else if (e.code == 'LockedOut') {
        throw BiometricException('Too many attempts, locked out');
      }
      return false;
    } catch (e) {
      return false;
    }
  }

  /// Stop authentication
  static Future<void> stopAuthentication() async {
    try {
      await _auth.stopAuthentication();
    } catch (e) {
      // Ignore
    }
  }

  /// Check if device has fingerprint
  static Future<bool> hasFingerprint() async {
    final biometrics = await getAvailableBiometrics();
    return biometrics.contains(BiometricType.fingerprint);
  }

  /// Check if device has face recognition
  static Future<bool> hasFaceId() async {
    final biometrics = await getAvailableBiometrics();
    return biometrics.contains(BiometricType.face);
  }

  /// Check if device has iris scanner
  static Future<bool> hasIris() async {
    final biometrics = await getAvailableBiometrics();
    return biometrics.contains(BiometricType.iris);
  }

  /// Get biometric type display name
  static String getBiometricName(BiometricType type) {
    switch (type) {
      case BiometricType.face:
        return 'Face ID';
      case BiometricType.fingerprint:
        return 'Fingerprint';
      case BiometricType.iris:
        return 'Iris';
      case BiometricType.strong:
        return 'Strong Biometric';
      case BiometricType.weak:
        return 'Weak Biometric';
    }
  }
}

/// Biometric exception
class BiometricException implements Exception {
  final String message;
  
  BiometricException(this.message);
  
  @override
  String toString() => 'BiometricException: $message';
}

