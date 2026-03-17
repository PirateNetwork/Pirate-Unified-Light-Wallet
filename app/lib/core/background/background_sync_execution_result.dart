class BackgroundSyncExecutionResult {
  final String mode;
  final int blocksSynced;
  final int startHeight;
  final int endHeight;
  final int durationSecs;
  final int newTransactions;
  final int? newBalance;
  final String tunnelUsed;
  final List<String> errors;
  final String? walletId;

  const BackgroundSyncExecutionResult({
    required this.mode,
    required this.blocksSynced,
    required this.startHeight,
    required this.endHeight,
    required this.durationSecs,
    required this.newTransactions,
    this.newBalance,
    required this.tunnelUsed,
    this.errors = const [],
    this.walletId,
  });

  Map<String, dynamic> toPlatformMap() {
    return {
      'mode': mode,
      'blocks_synced': blocksSynced,
      'start_height': startHeight,
      'end_height': endHeight,
      'duration_secs': durationSecs,
      'new_transactions': newTransactions,
      'new_balance': newBalance,
      'tunnel_used': tunnelUsed,
      'errors': errors,
      if (walletId != null) 'wallet_id': walletId,
      if (walletId != null) 'walletId': walletId,
    };
  }
}
