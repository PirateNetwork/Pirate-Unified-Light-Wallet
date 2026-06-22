/// Lightweight Litecoin address validation.
///
/// This performs structural checks (prefix, length, character set) rather than
/// a full base58/bech32 checksum verification. The goal is to catch the common
/// fund-loss mistakes before a swap is started: empty fields, an ARRR address
/// pasted by accident, an obvious typo, or a truncated address. The KDF backend
/// performs the authoritative validation when the withdrawal is broadcast.
library;

import '../i18n/arb_text_localizer.dart';

class LtcAddressCheck {
  const LtcAddressCheck._({required this.isValid, this.error});
  const LtcAddressCheck.invalid(String error)
    : this._(isValid: false, error: error);

  final bool isValid;
  final String? error;

  static const valid = LtcAddressCheck._(isValid: true);
}

const _base58Chars =
    '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const _bech32Chars = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';

/// Returns a structural validation result for a (mainnet) Litecoin address.
LtcAddressCheck checkLtcAddress(String raw) {
  final address = raw.trim();
  if (address.isEmpty) {
    return LtcAddressCheck.invalid(
      'Enter the LTC address to receive funds.'.tr,
    );
  }

  // Guard against pasting a Pirate Chain (ARRR) address by mistake.
  final lower = address.toLowerCase();
  if (lower.startsWith('zs1') || lower.startsWith('pirate1')) {
    return LtcAddressCheck.invalid(
      'That looks like a Pirate (ARRR) address. Enter a Litecoin (LTC) address.'
          .tr,
    );
  }

  // Native SegWit (bech32): ltc1...
  if (lower.startsWith('ltc1')) {
    if (address.length < 26 || address.length > 90) {
      return LtcAddressCheck.invalid('This LTC address looks incomplete.'.tr);
    }
    final body = lower.substring(4);
    final hasInvalidChar = body.runes.any(
      (rune) => !_bech32Chars.contains(String.fromCharCode(rune)),
    );
    if (hasInvalidChar) {
      return LtcAddressCheck.invalid(
        'This LTC address contains invalid characters.'.tr,
      );
    }
    return LtcAddressCheck.valid;
  }

  // Legacy P2PKH (L...) and P2SH (M... or 3...).
  final first = address[0];
  if (first == 'L' || first == 'M' || first == '3') {
    if (address.length < 26 || address.length > 36) {
      return LtcAddressCheck.invalid('This LTC address looks incomplete.'.tr);
    }
    final hasInvalidChar = address.runes.any(
      (rune) => !_base58Chars.contains(String.fromCharCode(rune)),
    );
    if (hasInvalidChar) {
      return LtcAddressCheck.invalid(
        'This LTC address contains invalid characters.'.tr,
      );
    }
    return LtcAddressCheck.valid;
  }

  return LtcAddressCheck.invalid(
    'Enter a valid Litecoin address (starts with ltc1, L, M, or 3).'.tr,
  );
}

bool isLikelyValidLtcAddress(String raw) => checkLtcAddress(raw).isValid;
