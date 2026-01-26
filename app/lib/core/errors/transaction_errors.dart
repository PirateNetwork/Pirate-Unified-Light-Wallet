/// Transaction error mapping - converts FFI errors to human-readable messages
library;

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

  /// Unknown error
  unknown,
}

/// Human-readable transaction error
class TransactionError implements Exception {
  final TransactionErrorType type;
  final String message;
  final String? technicalDetails;
  final String? suggestion;

  const TransactionError({
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

    // Address errors
    if (errorStr.contains('invalid address') ||
        errorStr.contains('must be sapling') ||
        errorStr.contains('must start with zs')) {
      return const TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Invalid recipient address',
        suggestion:
            'Please enter a valid Pirate Chain address starting with "zs1".',
      );
    }

    // Amount errors
    if (errorStr.contains('invalid amount') ||
        errorStr.contains('zero amount') ||
        errorStr.contains('negative')) {
      return const TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Invalid amount',
        suggestion: 'Please enter a valid positive amount.',
      );
    }

    if (errorStr.contains('overflow')) {
      return const TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Amount too large',
        suggestion: 'Please enter a smaller amount.',
      );
    }

    // Insufficient funds
    if (errorStr.contains('insufficient') || errorStr.contains('not enough')) {
      return TransactionError(
        type: TransactionErrorType.insufficientFunds,
        message: 'Insufficient funds',
        technicalDetails: error.toString(),
        suggestion:
            "You don't have enough ARRR to complete this transaction including fees.",
      );
    }

    // Memo errors
    if (errorStr.contains('memo') &&
        (errorStr.contains('too long') || errorStr.contains('bytes'))) {
      return TransactionError(
        type: TransactionErrorType.memoTooLong,
        message: 'Memo is too long',
        suggestion: 'Please shorten your memo to $maxMemoBytes bytes or less.',
      );
    }

    if (errorStr.contains('memo') && errorStr.contains('utf-8')) {
      return const TransactionError(
        type: TransactionErrorType.memoInvalidUtf8,
        message: 'Memo contains invalid characters',
        suggestion:
            'Please remove any special or non-text characters from the memo.',
      );
    }

    if (errorStr.contains('memo') && errorStr.contains('control')) {
      return const TransactionError(
        type: TransactionErrorType.memoControlChars,
        message: 'Memo contains invalid control characters',
        suggestion:
            'Please remove any hidden formatting characters from the memo.',
      );
    }

    // Output count
    if (errorStr.contains('too many outputs') || errorStr.contains('maximum')) {
      return TransactionError(
        type: TransactionErrorType.tooManyOutputs,
        message: 'Too many recipients',
        suggestion: 'Maximum $maxOutputs recipients per transaction.',
      );
    }

    // Fee errors
    if (errorStr.contains('fee') && errorStr.contains('low')) {
      return const TransactionError(
        type: TransactionErrorType.feeTooLow,
        message: 'Network fee too low',
        suggestion:
            'The fee is below the minimum required. Please increase it.',
      );
    }

    if (errorStr.contains('fee') && errorStr.contains('high')) {
      return const TransactionError(
        type: TransactionErrorType.feeTooHigh,
        message: 'Network fee unusually high',
        suggestion: 'The fee seems too high. Please review before sending.',
      );
    }

    // Network/broadcast errors
    if (errorStr.contains('network') ||
        errorStr.contains('connection') ||
        errorStr.contains('timeout')) {
      return const TransactionError(
        type: TransactionErrorType.networkError,
        message: 'Network connection failed',
        suggestion: 'Please check your internet connection and try again.',
      );
    }

    if (errorStr.contains('rejected')) {
      return TransactionError(
        type: TransactionErrorType.txRejected,
        message: 'Transaction rejected by network',
        technicalDetails: error.toString(),
        suggestion:
            'The network rejected this transaction. Please try again later.',
      );
    }

    if (errorStr.contains('already in mempool') ||
        errorStr.contains('duplicate')) {
      return const TransactionError(
        type: TransactionErrorType.txAlreadyInMempool,
        message: 'Transaction already sent',
        suggestion:
            'This transaction was already broadcast. Check your history.',
      );
    }

    if (errorStr.contains('conflict') || errorStr.contains('double spend')) {
      return const TransactionError(
        type: TransactionErrorType.txConflict,
        message: 'Transaction conflicts with pending transaction',
        suggestion:
            'Wait for your previous transaction to confirm before sending again.',
      );
    }

    if (errorStr.contains('expired') || errorStr.contains('expiry')) {
      return const TransactionError(
        type: TransactionErrorType.txExpired,
        message: 'Transaction expired',
        suggestion: 'Please rebuild and send the transaction again.',
      );
    }

    // Wallet errors
    if (errorStr.contains('locked') || errorStr.contains('unavailable')) {
      return const TransactionError(
        type: TransactionErrorType.walletLocked,
        message: 'Wallet is locked',
        suggestion: 'Please unlock your wallet to send transactions.',
      );
    }

    if (errorStr.contains('watch') && errorStr.contains('only')) {
      return const TransactionError(
        type: TransactionErrorType.watchOnlyCannotSpend,
        message: 'Cannot send from watch-only wallet',
        suggestion:
            'This wallet can only view incoming transactions. Use the full wallet to send.',
      );
    }

    // Unknown error
    return TransactionError(
      type: TransactionErrorType.unknown,
      message: 'Transaction failed',
      technicalDetails: error.toString(),
      suggestion: 'Please try again. If the problem persists, contact support.',
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
          message: 'Memo is too long (${bytes.length}/$maxMemoBytes bytes)',
          suggestion: 'Please shorten your memo.',
        );
      }

      // Check for control characters (except newline, tab, carriage return)
      for (final char in memo.runes) {
        if (_isControlChar(char)) {
          return const TransactionError(
            type: TransactionErrorType.memoControlChars,
            message: 'Memo contains invalid control characters',
            suggestion: 'Please remove any hidden formatting characters.',
          );
        }
      }

      return null;
    } catch (e) {
      return const TransactionError(
        type: TransactionErrorType.memoInvalidUtf8,
        message: 'Memo contains invalid characters',
        suggestion: 'Please use only standard text characters.',
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
      return const TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Address is required',
        suggestion: 'Please enter a recipient address.',
      );
    }

    final lower = address.toLowerCase();
    final isSapling = lower.startsWith('zs1');
    final isOrchard = lower.startsWith('pirate1');
    if (!isSapling && !isOrchard) {
      return const TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Invalid address format',
        suggestion: 'Address must start with "zs1" or "pirate1".',
      );
    }

    // Basic length check (Sapling ~78 chars, Orchard typically longer)
    const minLen = 70;
    final maxLen = isOrchard ? 120 : 90;
    if (address.length < minLen || address.length > maxLen) {
      return const TransactionError(
        type: TransactionErrorType.invalidAddress,
        message: 'Address has invalid length',
        suggestion: 'Please check the address and try again.',
      );
    }

    return null;
  }

  /// Validate amount
  static TransactionError? validateAmount(
      int arrrtoshis, int availableBalance) {
    if (arrrtoshis <= 0) {
      return const TransactionError(
        type: TransactionErrorType.invalidAmount,
        message: 'Amount must be greater than zero',
        suggestion: 'Please enter a valid amount to send.',
      );
    }

    if (arrrtoshis > availableBalance) {
      return const TransactionError(
        type: TransactionErrorType.insufficientFunds,
        message: 'Insufficient funds',
        suggestion: "You don't have enough ARRR to send this amount.",
      );
    }

    return null;
  }
}
