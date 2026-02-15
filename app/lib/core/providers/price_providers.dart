import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../features/settings/providers/preferences_providers.dart';

enum ArrrPriceSource { coingecko, coinMarketCap }

class ArrrPriceQuote {
  const ArrrPriceQuote({
    required this.currency,
    required this.pricePerArrr,
    required this.fetchedAt,
    required this.source,
  });

  final CurrencyPreference currency;
  final double pricePerArrr;
  final DateTime fetchedAt;
  final ArrrPriceSource source;
}

class ArrrPriceFormatter {
  static String formatArrr(double amount) {
    return '${_groupedFixed(amount, 8)} ARRR';
  }

  static String formatCurrency(
    CurrencyPreference currency,
    double amount, {
    bool includeCode = true,
  }) {
    final formatted = _groupedFixed(amount, currency.fractionDigits);
    switch (currency) {
      case CurrencyPreference.usd:
        return includeCode ? 'USD \$$formatted' : '\$$formatted';
      case CurrencyPreference.eur:
        return includeCode ? 'EUR $formatted' : formatted;
      case CurrencyPreference.gbp:
        return includeCode ? 'GBP $formatted' : formatted;
      case CurrencyPreference.btc:
        return includeCode ? 'BTC $formatted' : formatted;
      case CurrencyPreference.cad:
        return includeCode ? 'CAD \$$formatted' : '\$$formatted';
      case CurrencyPreference.aud:
        return includeCode ? 'AUD \$$formatted' : '\$$formatted';
      case CurrencyPreference.jpy:
        return includeCode ? 'JPY ¥$formatted' : '¥$formatted';
      case CurrencyPreference.chf:
        return includeCode ? 'CHF $formatted' : formatted;
      case CurrencyPreference.cny:
        return includeCode ? 'CNY ¥$formatted' : '¥$formatted';
      case CurrencyPreference.inr:
        return includeCode ? 'INR ₹$formatted' : '₹$formatted';
      case CurrencyPreference.brl:
        return includeCode ? 'BRL R\$$formatted' : 'R\$$formatted';
      case CurrencyPreference.krw:
        return includeCode ? 'KRW ₩$formatted' : '₩$formatted';
      case CurrencyPreference.rub:
        return includeCode ? 'RUB ₽$formatted' : '₽$formatted';
      case CurrencyPreference.uah:
        return includeCode ? 'UAH ₴$formatted' : '₴$formatted';
      case CurrencyPreference.mxn:
        return includeCode ? 'MXN \$$formatted' : '\$$formatted';
      case CurrencyPreference.ars:
        return includeCode ? 'ARS \$$formatted' : '\$$formatted';
      case CurrencyPreference.aed:
        return includeCode ? 'AED $formatted' : formatted;
      case CurrencyPreference.bhd:
        return includeCode ? 'BHD $formatted' : formatted;
      case CurrencyPreference.kwd:
        return includeCode ? 'KWD $formatted' : formatted;
      case CurrencyPreference.sar:
        return includeCode ? 'SAR $formatted' : formatted;
      case CurrencyPreference.tryCurrency:
        return includeCode ? 'TRY ₺$formatted' : '₺$formatted';
    }
  }

  static String _groupedFixed(double value, int fractionDigits) {
    final fixed = value.toStringAsFixed(fractionDigits);
    final parts = fixed.split('.');
    var integer = parts[0];
    final negative = integer.startsWith('-');
    if (negative) {
      integer = integer.substring(1);
    }
    final grouped = integer.replaceAllMapped(
      RegExp(r'\B(?=(\d{3})+(?!\d))'),
      (_) => ',',
    );
    final sign = negative ? '-' : '';
    if (parts.length == 1 || fractionDigits == 0) {
      return '$sign$grouped';
    }
    return '$sign$grouped.${parts[1]}';
  }
}

class _ArrrPriceService {
  static const Duration _timeout = Duration(seconds: 8);
  static const Duration _cacheTtl = Duration(seconds: 30);
  static const String _coinMarketCapQuote =
      'https://api.coinmarketcap.com/data-api/v3/cryptocurrency/quote/latest?id=3951';
  static const String _coinMarketCapMarketPairs =
      'https://api.coinmarketcap.com/data-api/v3/cryptocurrency/market-pairs/latest?slug=pirate-chain&start=1&limit=1&category=spot&centerType=all&sort=cmc_rank_advanced';
  static const Map<CurrencyPreference, String> _vsCurrencyCodes = {
    CurrencyPreference.usd: 'usd',
    CurrencyPreference.eur: 'eur',
    CurrencyPreference.gbp: 'gbp',
    CurrencyPreference.btc: 'btc',
    CurrencyPreference.cad: 'cad',
    CurrencyPreference.aud: 'aud',
    CurrencyPreference.jpy: 'jpy',
    CurrencyPreference.chf: 'chf',
    CurrencyPreference.cny: 'cny',
    CurrencyPreference.inr: 'inr',
    CurrencyPreference.brl: 'brl',
    CurrencyPreference.krw: 'krw',
    CurrencyPreference.rub: 'rub',
    CurrencyPreference.uah: 'uah',
    CurrencyPreference.mxn: 'mxn',
    CurrencyPreference.ars: 'ars',
    CurrencyPreference.aed: 'aed',
    CurrencyPreference.bhd: 'bhd',
    CurrencyPreference.kwd: 'kwd',
    CurrencyPreference.sar: 'sar',
    CurrencyPreference.tryCurrency: 'try',
  };

  static final Map<CurrencyPreference, ArrrPriceQuote> _lastByCurrency =
      <CurrencyPreference, ArrrPriceQuote>{};

  static ArrrPriceQuote? cached(CurrencyPreference currency) {
    final existing = _lastByCurrency[currency];
    if (existing == null) return null;
    if (DateTime.now().difference(existing.fetchedAt) > _cacheTtl) {
      return null;
    }
    return existing;
  }

  static Future<ArrrPriceQuote?> fetch(CurrencyPreference currency) async {
    final cachedQuote = cached(currency);
    if (cachedQuote != null) {
      return cachedQuote;
    }

    final now = DateTime.now();
    final geckoPrices = await _fetchCoingeckoPrices();
    if (geckoPrices != null) {
      final price = geckoPrices[currency];
      if (price != null && price > 0) {
        final quote = ArrrPriceQuote(
          currency: currency,
          pricePerArrr: price,
          fetchedAt: now,
          source: ArrrPriceSource.coingecko,
        );
        _lastByCurrency[currency] = quote;
        return quote;
      }
    }

    final cmcUsd = await _fetchCoinMarketCapUsd();
    if (cmcUsd == null || cmcUsd <= 0) {
      return null;
    }

    if (currency != CurrencyPreference.usd) {
      // Public CoinMarketCap data API only exposes a single quote (USD-like).
      // We intentionally avoid deriving other fiat rates from cached cross rates.
      return null;
    }

    final quote = ArrrPriceQuote(
      currency: currency,
      pricePerArrr: cmcUsd,
      fetchedAt: now,
      source: ArrrPriceSource.coinMarketCap,
    );
    _lastByCurrency[currency] = quote;
    return quote;
  }

  static Future<Map<CurrencyPreference, double>?>
  _fetchCoingeckoPrices() async {
    try {
      final vsCurrencies = _vsCurrencyCodes.values.join(',');
      final uri = Uri.parse(
        'https://api.coingecko.com/api/v3/simple/price?ids=pirate-chain&vs_currencies=$vsCurrencies',
      );
      final json = await _downloadJson(uri, userAgent: 'PirateWallet');
      if (json is! Map) return null;
      final pirate = json['pirate-chain'];
      if (pirate is! Map) return null;

      final map = <CurrencyPreference, double>{};
      for (final entry in _vsCurrencyCodes.entries) {
        final price = _parsePrice(pirate[entry.value]);
        if (price != null && price > 0) {
          map[entry.key] = price;
        }
      }
      return map.isEmpty ? null : map;
    } catch (_) {
      return null;
    }
  }

  static Future<double?> _fetchCoinMarketCapUsd() async {
    final fromQuote = await _fetchCoinMarketCapUsdFromQuote();
    if (fromQuote != null && fromQuote > 0) {
      return fromQuote;
    }
    return _fetchCoinMarketCapUsdFromMarketPairs();
  }

  static Future<double?> _fetchCoinMarketCapUsdFromQuote() async {
    try {
      final uri = Uri.parse(_coinMarketCapQuote);
      final json = await _downloadJson(uri, userAgent: 'PirateWallet');
      if (json is! Map) return null;
      final data = json['data'];
      if (data is! List || data.isEmpty) return null;
      final first = data.first;
      if (first is! Map) return null;
      final quotes = first['quotes'];
      if (quotes is! List || quotes.isEmpty) return null;
      final quote = quotes.first;
      if (quote is! Map) return null;
      return _parsePrice(quote['price']);
    } catch (_) {
      return null;
    }
  }

  static Future<double?> _fetchCoinMarketCapUsdFromMarketPairs() async {
    try {
      final uri = Uri.parse(_coinMarketCapMarketPairs);
      final json = await _downloadJson(uri, userAgent: 'PirateWallet');
      if (json is! Map) return null;
      final data = json['data'];
      if (data is! Map) return null;
      final pairs = data['marketPairs'];
      if (pairs is! List || pairs.isEmpty) return null;
      final first = pairs.first;
      if (first is! Map) return null;
      return _parsePrice(first['price']);
    } catch (_) {
      return null;
    }
  }

  static Future<dynamic> _downloadJson(Uri uri, {String? userAgent}) async {
    final client = HttpClient();
    try {
      final request = await client.getUrl(uri).timeout(_timeout);
      if (userAgent != null && userAgent.isNotEmpty) {
        request.headers.set('User-Agent', userAgent);
      }
      request.headers.set('Accept', 'application/json');
      final response = await request.close().timeout(_timeout);
      if (response.statusCode < 200 || response.statusCode >= 300) {
        return null;
      }
      final body = await response.transform(utf8.decoder).join();
      return jsonDecode(body);
    } finally {
      client.close(force: true);
    }
  }

  static double? _parsePrice(dynamic value) {
    if (value is num) return value.toDouble();
    return double.tryParse(value?.toString() ?? '');
  }
}

final arrrPriceQuoteProvider = StreamProvider<ArrrPriceQuote?>((ref) {
  final currency = ref.watch(currencyPreferenceProvider);
  final allowPrices = ref.watch(allowPriceApisProvider);
  final controller = StreamController<ArrrPriceQuote?>();

  Timer? pollTimer;
  ArrrPriceQuote? last;

  Future<void> refreshQuote() async {
    if (controller.isClosed) {
      return;
    }
    final quote = await _ArrrPriceService.fetch(currency);
    if (controller.isClosed) {
      return;
    }
    if (quote != null) {
      last = quote;
      controller.add(quote);
      return;
    }
    if (last == null) {
      controller.add(null);
    }
  }

  if (!allowPrices || kIsWeb) {
    controller.add(null);
    unawaited(controller.close());
    return controller.stream;
  }

  last = _ArrrPriceService.cached(currency);
  if (last != null) {
    controller.add(last);
  }

  unawaited(refreshQuote());
  pollTimer = Timer.periodic(const Duration(seconds: 45), (_) {
    unawaited(refreshQuote());
  });

  ref.onDispose(() {
    pollTimer?.cancel();
    if (!controller.isClosed) {
      unawaited(controller.close());
    }
  });

  return controller.stream;
});
