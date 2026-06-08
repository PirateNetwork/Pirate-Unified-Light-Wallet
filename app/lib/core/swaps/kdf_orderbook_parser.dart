import 'package:decimal/decimal.dart';

import 'swap_models.dart';

class KdfOrderbook {
  const KdfOrderbook({required this.asks, required this.bids});

  final List<SwapOrderbookLevel> asks;
  final List<SwapOrderbookLevel> bids;

  bool get isEmpty => asks.isEmpty && bids.isEmpty;
}

KdfOrderbook parseKdfOrderbook(Map<String, dynamic> response) {
  final result = _orderbookResult(response);
  return KdfOrderbook(
    asks: parseKdfOrderbookSide(result, 'asks'),
    bids: parseKdfOrderbookSide(result, 'bids'),
  );
}

List<SwapOrderbookLevel> parseKdfOrderbookSide(
  Map<String, dynamic> response,
  String side,
) {
  return _sideValues(response, side)
      .map(parseKdfOrderbookLevel)
      .whereType<SwapOrderbookLevel>()
      .toList(growable: false);
}

SwapOrderbookLevel? parseKdfOrderbookLevel(Object? value) {
  if (value is! Map) return null;

  final raw = Map<String, dynamic>.from(value);
  final entry = _mapOrNull(raw['entry']) ?? raw;
  final price = _decimal(entry['price'] ?? raw['price']);
  final volume = _decimal(
    entry['base_max_volume'] ??
        raw['base_max_volume'] ??
        entry['base_max_volume_aggr'] ??
        raw['base_max_volume_aggr'] ??
        entry['max_volume'] ??
        raw['max_volume'] ??
        entry['maxvolume'] ??
        raw['maxvolume'] ??
        entry['volume'] ??
        raw['volume'],
  );

  if (price <= Decimal.zero || volume <= Decimal.zero) return null;

  return SwapOrderbookLevel(
    priceLtcPerArrr: price,
    arrrAmount: volume,
    orderId: (entry['uuid'] ?? raw['uuid'])?.toString(),
    raw: raw,
  );
}

Map<String, dynamic> _orderbookResult(Map<String, dynamic> response) {
  final result = _mapOrNull(response['result']);
  if (result == null) return response;
  return _mapOrNull(result['orderbook']) ?? result;
}

Iterable<Object?> _sideValues(Map<String, dynamic> response, String side) {
  final result = _orderbookResult(response);
  final value = result[side];
  if (value is List) return value;
  if (value is Map) return value.values;
  return const <Object?>[];
}

Map<String, dynamic>? _mapOrNull(Object? value) {
  if (value is! Map) return null;
  return Map<String, dynamic>.from(value);
}

Decimal _decimal(Object? value) {
  if (value == null) return Decimal.zero;
  if (value is Decimal) return value;
  if (value is num || value is String) {
    final text = value.toString().trim();
    if (text.isEmpty) return Decimal.zero;
    return Decimal.tryParse(text) ?? Decimal.zero;
  }

  final map = _mapOrNull(value);
  if (map == null) return Decimal.zero;

  for (final key in const [
    'decimal',
    'value',
    'amount',
    'price',
    'base_max_volume',
    'base_max_volume_aggr',
    'max_volume',
    'maxvolume',
    'volume',
  ]) {
    final parsed = _decimal(map[key]);
    if (parsed > Decimal.zero) return parsed;
  }

  final fraction = _mapOrNull(map['fraction']);
  if (fraction != null) {
    final parsed = _fraction(fraction);
    if (parsed > Decimal.zero) return parsed;
  }

  final rational = _mapOrNull(map['rational']);
  if (rational != null) {
    final parsed = _fraction(rational);
    if (parsed > Decimal.zero) return parsed;
  }

  return _fraction(map);
}

Decimal _fraction(Map<String, dynamic> map) {
  final numerator = map['numer'] ?? map['numerator'];
  final denominator = map['denom'] ?? map['denominator'];
  if (numerator == null || denominator == null) return Decimal.zero;

  final numValue = _decimal(numerator);
  final denValue = _decimal(denominator);
  if (numValue == Decimal.zero || denValue == Decimal.zero) {
    return Decimal.zero;
  }
  return (numValue / denValue).toDecimal(scaleOnInfinitePrecision: 16);
}
