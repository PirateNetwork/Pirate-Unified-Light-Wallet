import 'package:decimal/decimal.dart';

const arrrZatoshisPerCoin = 100000000;

BigInt arrrToZatoshis(Decimal amount) {
  final scaled = amount * Decimal.fromInt(arrrZatoshisPerCoin);
  return scaled.truncate().toBigInt();
}

Decimal arrrFromZatoshis(BigInt zatoshis) {
  return (Decimal.fromBigInt(zatoshis) / Decimal.fromInt(arrrZatoshisPerCoin))
      .toDecimal(scaleOnInfinitePrecision: 8);
}

String formatSwapAmount(Decimal amount, {int fractionDigits = 8}) {
  final text = amount.toString();
  if (!text.contains('.')) return text;
  final trimmed = text
      .replaceFirst(RegExp(r'0+$'), '')
      .replaceFirst(RegExp(r'\.$'), '');
  if (!trimmed.contains('.')) return trimmed;
  final parts = trimmed.split('.');
  if (parts.length == 2 && parts[1].length > fractionDigits) {
    return '${parts[0]}.${parts[1].substring(0, fractionDigits)}';
  }
  return trimmed;
}

String formatSwapAmountFixed(Decimal amount, {required int fractionDigits}) {
  final text = amount.toString();
  final parts = text.split('.');
  if (fractionDigits <= 0) return parts.first;

  final whole = parts.first;
  final fraction = parts.length > 1 ? parts[1] : '';
  final normalizedFraction = fraction.length > fractionDigits
      ? fraction.substring(0, fractionDigits)
      : fraction.padRight(fractionDigits, '0');

  return '$whole.$normalizedFraction';
}

String formatSwapAmountTextRounded(String value, {int fractionDigits = 2}) {
  final trimmed = value.trim();
  if (trimmed.isEmpty) return trimmed;
  try {
    return Decimal.parse(trimmed).toStringAsFixed(fractionDigits);
  } catch (_) {
    return value;
  }
}
