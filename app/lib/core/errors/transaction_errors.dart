/// Transaction error mapping - converts FFI errors to human-readable messages
library;

import '../i18n/arb_text_localizer.dart';

/// Transaction error types
enum TransactionErrorType {
  /// Invalid recipient address
  invalidAddress,

  /// Invalid amount (zero, negative, or overflow)
  invalidAmount,

  /// Insufficient funds in wallet
  insufficientFunds,

  /// Memo too long (> 512 bytes)
  memoTooLong,

  /// Memo contains invalid UTF-8
  memoInvalidUtf8,

  /// Memo contains control characters
  memoControlChars,

  /// Too many outputs (> 50)
  tooManyOutputs,

  /// Network error during broadcast
  networkError,

  /// Transaction rejected by network
  txRejected,

  /// Transaction already in mempool
  txAlreadyInMempool,

  /// Transaction conflicts with unconfirmed tx
  txConflict,

  /// Fee too low
  feeTooLow,

  /// Fee too high
  feeTooHigh,

  /// Transaction expired
  txExpired,

  /// Wallet locked or unavailable
  walletLocked,

  /// Watch-only wallet cannot spend
  watchOnlyCannotSpend,

  /// Wallet is still finalizing spendability/witness state
  syncFinalizing,

  /// Wallet requires an explicit rescan before spending
  rescanRequired,

  /// Unknown error
  unknown,
}

/// Human-readable transaction error
class TransactionError implements Exception {
  final TransactionErrorType type;
  final String message;
  final String? technicalDetails;
  final String? suggestion;

  TransactionError({
    required this.type,
    required this.message,
    this.technicalDetails,
    this.suggestion,
  });

  @override
  String toString() => message;

  /// Get user-friendly error display
  String get displayMessage {
    if (suggestion != null) {
      return '$message\n\n$suggestion';
    }
    return message;
  }
}

/// Maps error strings from FFI to human-readable TransactionError
class TransactionErrorMapper {
  /// Maximum memo length in bytes
  static const int maxMemoBytes = 512;

  /// Maximum outputs per transaction
  static const int maxOutputs = 50;

  /// Minimum fee in arrrtoshis
  static const int minFee = 10000;

  /// Maximum fee in arrrtoshis (0.01 ARRR)
  static const int maxFee = 1000000;

  /// Map FFI error string to TransactionError
  static TransactionError mapError(dynamic error) {
    final errorStr = error.toString().toLowerCase();

    // Deterministic spendability state errors from Rust
    if (errorStr.contains('err_witness_repair_queued') ||
        errorStr.contains('err_sync_finalizing')) {
      return TransactionError(
        type: TransactionErrorType.syncFinalizing,
        message: 'Spendability is finalizing'.tr,
        technicalDetails: error.toString(),
        suggestion: 'Let wallet sync finish, then try sending again.'.tr,
      );
    }

    if (errorStr.contains('err_rescan_required')) {
      return TransactionError(
        type: TransactionErrorType.rescanRequired,
        message: 'Rescan required before sending'.tr,
        technicalDetails: error.toString(),
        suggestion:
            'Run a rescan, let it complete, then retry the transaction.'.tr,
      );
    }

    // Address errors
    if (errorStr.contains('invalid address') ||
        errorStr.contains('must be sapling') ||
        errorStr.contains('must start with zs')) {
      return TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Invalid recipient address'.tr,
        suggestion:
            'Please enter a valid Pirate Chain address starting with "zs1".'.tr,
      );
    }

    // Amount errors
    if (errorStr.contains('invalid amount') ||
        errorStr.contains('zero amount') ||
        errorStr.contains('negative')) {
      return TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Invalid amount'.tr,
        suggestion: 'Please enter a valid positive amount.'.tr,
      );
    }

    if (errorStr.contains('overflow')) {
      return TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Amount too large'.tr,
        suggestion: 'Please enter a smaller amount.'.tr,
      );
    }

    // Insufficient funds
    if (errorStr.contains('insufficient') || errorStr.contains('not enough')) {
      return TransactionError(
        type: TransactionErrorType.insufficientFunds,
        message: 'Insufficient funds'.tr,
        technicalDetails: error.toString(),
        suggestion:
            "You don't have enough ARRR to complete this transaction including fees."
                .tr,
      );
    }

    // Memo errors
    if (errorStr.contains('memo') &&
        (errorStr.contains('too long') || errorStr.contains('bytes'))) {
      return TransactionError(
        type: TransactionErrorType.memoTooLong,
        message: 'Memo is too long'.tr,
        suggestion: 'Please shorten your memo to {maxBytes} bytes or less.'
            .trArgs({'maxBytes': maxMemoBytes}),
      );
    }

    if (errorStr.contains('memo') && errorStr.contains('utf-8')) {
      return TransactionError(
        type: TransactionErrorType.memoInvalidUtf8,
        message: 'Memo contains invalid characters'.tr,
        suggestion:
            'Please remove any special or non-text characters from the memo.'
                .tr,
      );
    }

    if (errorStr.contains('memo') && errorStr.contains('control')) {
      return TransactionError(
        type: TransactionErrorType.memoControlChars,
        message: 'Memo contains invalid control characters'.tr,
        suggestion:
            'Please remove any hidden formatting characters from the memo.'.tr,
      );
    }

    // Output count
    if (errorStr.contains('too many outputs') || errorStr.contains('maximum')) {
      return TransactionError(
        type: TransactionErrorType.tooManyOutputs,
        message: 'Too many recipients'.tr,
        suggestion: 'Maximum {maxOutputs} recipients per transaction.'.trArgs({
          'maxOutputs': maxOutputs,
        }),
      );
    }

    // Fee errors
    if (errorStr.contains('fee') && errorStr.contains('low')) {
      return TransactionError(
        type: TransactionErrorType.feeTooLow,
        message: 'Network fee too low'.tr,
        suggestion:
            'The fee is below the minimum required. Please increase it.'.tr,
      );
    }

    if (errorStr.contains('fee') && errorStr.contains('high')) {
      return TransactionError(
        type: TransactionErrorType.feeTooHigh,
        message: 'Network fee unusually high'.tr,
        suggestion: 'The fee seems too high. Please review before sending.'.tr,
      );
    }

    // Network/broadcast errors
    if (errorStr.contains('network') ||
        errorStr.contains('connection') ||
        errorStr.contains('timeout')) {
      return TransactionError(
        type: TransactionErrorType.networkError,
        message: 'Network connection failed'.tr,
        suggestion: 'Please check your internet connection and try again.'.tr,
      );
    }

    if (errorStr.contains('rejected')) {
      return TransactionError(
        type: TransactionErrorType.txRejected,
        message: 'Transaction rejected by network'.tr,
        technicalDetails: error.toString(),
        suggestion:
            'The network rejected this transaction. Please try again later.'.tr,
      );
    }

    if (errorStr.contains('already in mempool') ||
        errorStr.contains('duplicate')) {
      return TransactionError(
        type: TransactionErrorType.txAlreadyInMempool,
        message: 'Transaction already sent'.tr,
        suggestion:
            'This transaction was already broadcast. Check your history.'.tr,
      );
    }

    if (errorStr.contains('conflict') || errorStr.contains('double spend')) {
      return TransactionError(
        type: TransactionErrorType.txConflict,
        message: 'Transaction conflicts with pending transaction'.tr,
        suggestion:
            'Wait for your previous transaction to confirm before sending again.'
                .tr,
      );
    }

    if (errorStr.contains('expired') || errorStr.contains('expiry')) {
      return TransactionError(
        type: TransactionErrorType.txExpired,
        message: 'Transaction expired'.tr,
        suggestion: 'Please rebuild and send the transaction again.'.tr,
      );
    }

    // Wallet errors
    if (errorStr.contains('locked') || errorStr.contains('unavailable')) {
      return TransactionError(
        type: TransactionErrorType.walletLocked,
        message: 'Wallet is locked'.tr,
        suggestion: 'Please unlock your wallet to send transactions.'.tr,
      );
    }

    if (errorStr.contains('watch') && errorStr.contains('only')) {
      return TransactionError(
        type: TransactionErrorType.watchOnlyCannotSpend,
        message: 'Cannot send from view only wallet'.tr,
        suggestion:
            'This wallet can only view incoming transactions. Use the full wallet to send.'
                .tr,
      );
    }

    // Unknown error
    return TransactionError(
      type: TransactionErrorType.unknown,
      message: 'Transaction failed'.tr,
      technicalDetails: error.toString(),
      suggestion:
          'Please try again. If the problem persists, contact support.'.tr,
    );
  }

  /// Validate memo and return error if invalid
  static TransactionError? validateMemo(String? memo) {
    if (memo == null || memo.isEmpty) {
      return null; // Empty memo is valid
    }

    // Check UTF-8 encoding
    try {
      final bytes = memo.codeUnits;

      // Check byte length
      if (bytes.length > maxMemoBytes) {
        return TransactionError(
          type: TransactionErrorType.memoTooLong,
          message: 'Memo is too long ({length}/{maxBytes} bytes)'.trArgs({
            'length': bytes.length,
            'maxBytes': maxMemoBytes,
          }),
          suggestion: 'Please shorten your memo.'.tr,
        );
      }

      // Check for control characters (except newline, tab, carriage return)
      for (final char in memo.runes) {
        if (_isControlChar(char)) {
          return TransactionError(
            type: TransactionErrorType.memoControlChars,
            message: 'Memo contains invalid control characters'.tr,
            suggestion: 'Please remove any hidden formatting characters.'.tr,
          );
        }
      }

      return null;
    } catch (e) {
      return TransactionError(
        type: TransactionErrorType.memoInvalidUtf8,
        message: 'Memo contains invalid characters'.tr,
        suggestion: 'Please use only standard text characters.'.tr,
      );
    }
  }

  /// Check if a character code is a control character
  static bool _isControlChar(int code) {
    // Allow newline (10), tab (9), carriage return (13)
    if (code == 9 || code == 10 || code == 13) {
      return false;
    }
    // Control characters are 0-31 and 127
    return code < 32 || code == 127;
  }

  /// Validate address format
  static TransactionError? validateAddress(String address) {
    if (address.isEmpty) {
      return TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Address is required'.tr,
        suggestion: 'Please enter a recipient address.'.tr,
      );
    }

    final lower = address.toLowerCase();
    final isSapling = lower.startsWith('zs1');
    final isOrchard = lower.startsWith('pirate1');
    if (!isSapling && !isOrchard) {
      return TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Invalid address format'.tr,
        suggestion: 'Address must start with "zs1" or "pirate1".'.tr,
      );
    }

    // Basic length check (Sapling ~78 chars, Orchard typically longer)
    const minLen = 70;
    final maxLen = isOrchard ? 120 : 90;
    if (address.length < minLen || address.length > maxLen) {
      return TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Address has invalid length'.tr,
        suggestion: 'Please check the address and try again.'.tr,
      );
    }

    return null;
  }

  /// Validate amount
  static TransactionError? validateAmount(
    int arrrtoshis,
    int availableBalance,
  ) {
    if (arrrtoshis <= 0) {
      return TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Amount must be greater than zero'.tr,
        suggestion: 'Please enter a valid amount to send.'.tr,
      );
    }

    if (arrrtoshis > availableBalance) {
      return TransactionError(
        type: TransactionErrorType.insufficientFunds,
        message: 'Insufficient funds'.tr,
        suggestion: "You don't have enough ARRR to send this amount.".tr,
      );
    }

    return null;
  }
}
