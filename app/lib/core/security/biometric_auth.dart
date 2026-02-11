// Biometric authentication
//
// Uses local_auth package for fingerprint/Face ID/Touch ID

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:local_auth/local_auth.dart';

/// Biometric authentication manager
class BiometricAuth {
  static final _auth = LocalAuthentication();

  /// Check if biometrics are available
  static Future<bool> isAvailable() async {
    if (Platform.isLinux) {
      // local_auth 3.x does not currently provide Linux support.
      return false;
    }
    try {
      final deviceSupported = await _auth.isDeviceSupported();
      if (!deviceSupported) {
        return false;
      }
      final canCheck = await _auth.canCheckBiometrics;
      if (!canCheck) {
        return false;
      }
      final enrolled = await _auth.getAvailableBiometrics();
      return enrolled.isNotEmpty;
    } on LocalAuthException catch (e) {
      debugPrint(
        'Biometric availability check failed (${e.code.name}): ${e.description ?? e}',
      );
      return false;
    } catch (e) {
      debugPrint('Biometric availability check failed: $e');
      return false;
    }
  }

  /// Get available biometric types
  static Future<List<BiometricType>> getAvailableBiometrics() async {
    if (Platform.isLinux) {
      return const <BiometricType>[];
    }
    try {
      return await _auth.getAvailableBiometrics();
    } on LocalAuthException catch (e) {
      debugPrint(
        'Failed to query enrolled biometrics (${e.code.name}): ${e.description ?? e}',
      );
      return const <BiometricType>[];
    } catch (e) {
      debugPrint('Failed to query enrolled biometrics: $e');
      return const <BiometricType>[];
    }
  }

  /// Authenticate with biometrics
  static Future<bool> authenticate({
    String reason = 'Please authenticate to unlock wallet',
    bool biometricOnly = false,
  }) async {
    if (Platform.isLinux) {
      throw BiometricException(
        'Biometric authentication is not supported on Linux builds yet.',
      );
    }

    // Windows Hello via local_auth does not support biometricOnly=true.
    // Keep biometric-only behavior on Android/iOS/macOS, and gracefully
    // downgrade on platforms that don't support that flag.
    final supportsBiometricOnly = !Platform.isWindows;
    final effectiveBiometricOnly = biometricOnly && supportsBiometricOnly;

    try {
      return await _auth.authenticate(
        localizedReason: reason,
        biometricOnly: effectiveBiometricOnly,
        persistAcrossBackgrounding: true,
        sensitiveTransaction: true,
      );
    } on LocalAuthException catch (e) {
      if (_isUserCancellation(e.code)) {
        return false;
      }
      throw BiometricException(_messageForLocalAuthException(e));
    } on PlatformException catch (e) {
      // Compatibility fallback for platform implementations that still throw
      // PlatformException directly.
      final code = e.code.toLowerCase();
      if (code.contains('cancel')) {
        return false;
      }
      if (code.contains('notavailable') || code.contains('nohardware')) {
        throw BiometricException('Biometric hardware is not available.');
      }
      if (code.contains('notenrolled')) {
        throw BiometricException('No biometrics are enrolled on this device.');
      }
      if (code.contains('lockout')) {
        throw BiometricException(
          'Biometric authentication is locked. Unlock your device and try again.',
        );
      }
      throw BiometricException(e.message ?? 'Biometric authentication failed.');
    } catch (e) {
      throw BiometricException('Biometric authentication failed: $e');
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

  static bool _isUserCancellation(LocalAuthExceptionCode code) {
    return code == LocalAuthExceptionCode.userCanceled ||
        code == LocalAuthExceptionCode.systemCanceled ||
        code == LocalAuthExceptionCode.timeout ||
        code == LocalAuthExceptionCode.userRequestedFallback;
  }

  static String _messageForLocalAuthException(LocalAuthException error) {
    switch (error.code) {
      case LocalAuthExceptionCode.noBiometricHardware:
      case LocalAuthExceptionCode.biometricHardwareTemporarilyUnavailable:
        return 'Biometric hardware is not available.';
      case LocalAuthExceptionCode.noBiometricsEnrolled:
        return 'No biometrics are enrolled on this device.';
      case LocalAuthExceptionCode.noCredentialsSet:
        return 'Set up a device passcode/credentials first, then try again.';
      case LocalAuthExceptionCode.temporaryLockout:
      case LocalAuthExceptionCode.biometricLockout:
        return 'Biometric authentication is locked. Unlock your device and try again.';
      case LocalAuthExceptionCode.authInProgress:
        return 'Another biometric authentication is already in progress.';
      case LocalAuthExceptionCode.uiUnavailable:
        return 'Biometric prompt is currently unavailable.';
      case LocalAuthExceptionCode.deviceError:
      case LocalAuthExceptionCode.unknownError:
        return error.description ?? 'Biometric authentication failed.';
      case LocalAuthExceptionCode.userCanceled:
      case LocalAuthExceptionCode.systemCanceled:
      case LocalAuthExceptionCode.timeout:
      case LocalAuthExceptionCode.userRequestedFallback:
        return 'Biometric authentication was cancelled.';
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
