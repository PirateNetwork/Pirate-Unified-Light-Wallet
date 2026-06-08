import 'swap_models.dart';

const List<String> _swapHistoryCsvHeaders = [
  'id',
  'wallet_id',
  'pair',
  'side',
  'status',
  'created_at_utc',
  'updated_at_utc',
  'pay_amount',
  'pay_asset',
  'receive_amount',
  'receive_asset',
  'reference_price_rel_per_arrr',
  'market_arrr_amount',
  'market_rel_amount',
  'remainder_arrr_amount',
  'remainder_rel_amount',
  'app_fee_arrr',
  'total_taker_fee_arrr',
  'kdf_taker_fee_arrr_estimate',
  'slippage_cap',
  'realized_slippage',
  'market_swap_uuid',
  'limit_order_uuid',
  'deposit_address',
  'destination_address',
  'arrr_receiving_address',
  'last_error',
];

String buildSwapHistoryCsv(List<SwapIntent> intents) {
  final sorted = intents.toList()
    ..sort((a, b) => b.updatedAt.compareTo(a.updatedAt));
  final rows = [
    _swapHistoryCsvHeaders,
    for (final intent in sorted) _swapHistoryCsvRow(intent),
  ];
  return '${rows.map(_csvLine).join('\n')}\n';
}

List<String> _swapHistoryCsvRow(SwapIntent intent) {
  final isBuy = intent.side == SwapSide.buyArrr;
  final plan = intent.plan;
  return [
    intent.id,
    intent.walletId,
    intent.pair.displayName,
    intent.side.name,
    intent.status.name,
    intent.createdAt.toUtc().toIso8601String(),
    intent.updatedAt.toUtc().toIso8601String(),
    intent.requestedPayAmount.toString(),
    if (isBuy) intent.pair.relTicker else intent.pair.baseTicker,
    intent.expectedReceiveAmount.toString(),
    if (isBuy) intent.pair.baseTicker else intent.pair.relTicker,
    plan.referencePriceRelPerArrr.toString(),
    plan.marketArrrAmount.toString(),
    plan.marketRelAmount.toString(),
    plan.remainderArrrAmount.toString(),
    plan.remainderRelAmount.toString(),
    plan.appFeeArrrAmount.toString(),
    plan.totalTakerFeeArrrAmount.toString(),
    plan.estimatedKdfTakerFeeArrrAmount.toString(),
    plan.slippageCap.toString(),
    plan.realizedSlippage.toString(),
    intent.marketSwapUuid ?? '',
    intent.limitOrderUuid ?? '',
    intent.relDepositAddress ?? '',
    intent.destinationRelAddress ?? '',
    intent.arrReceivingAddress ?? '',
    intent.lastError ?? '',
  ];
}

String _csvLine(List<String> values) {
  return values.map(_csvCell).join(',');
}

String _csvCell(String value) {
  if (!value.contains(',') &&
      !value.contains('"') &&
      !value.contains('\n') &&
      !value.contains('\r')) {
    return value;
  }
  return '"${value.replaceAll('"', '""')}"';
}
