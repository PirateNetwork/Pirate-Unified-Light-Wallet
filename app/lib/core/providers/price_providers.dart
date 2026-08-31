import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../ffi/ffi_bridge.dart';
import '../../features/settings/providers/preferences_providers.dart';

enum ArrrPriceSource { coingecko, coinPaprika, coinMarketCap }

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

class AssetUsdPriceQuote {
  const AssetUsdPriceQuote({
    required this.assetId,
    required this.ticker,
    required this.pricePerUnit,
    required this.fetchedAt,
  });

  final String assetId;
  final String ticker;
  final double pricePerUnit;
  final DateTime fetchedAt;
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
  static const String _coinPaprikaTicker =
      'https://api.coinpaprika.com/v1/tickers/arrr-pirate';
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
  static const Map<CurrencyPreference, String> _coinPaprikaQuoteCodes = {
    CurrencyPreference.usd: 'USD',
    CurrencyPreference.eur: 'EUR',
    CurrencyPreference.gbp: 'GBP',
    CurrencyPreference.btc: 'BTC',
    CurrencyPreference.cad: 'CAD',
    CurrencyPreference.aud: 'AUD',
    CurrencyPreference.jpy: 'JPY',
    CurrencyPreference.chf: 'CHF',
    CurrencyPreference.cny: 'CNY',
    CurrencyPreference.inr: 'INR',
    CurrencyPreference.brl: 'BRL',
    CurrencyPreference.krw: 'KRW',
    CurrencyPreference.rub: 'RUB',
    CurrencyPreference.uah: 'UAH',
    CurrencyPreference.mxn: 'MXN',
    CurrencyPreference.ars: 'ARS',
    CurrencyPreference.tryCurrency: 'TRY',
  };

  static final Map<CurrencyPreference, ArrrPriceQuote> _lastByCurrency =
      <CurrencyPreference, ArrrPriceQuote>{};
  static final Map<String, AssetUsdPriceQuote> _lastAssetUsdById =
      <String, AssetUsdPriceQuote>{};
  static final Map<CurrencyPreference, Future<ArrrPriceQuote?>>
  _inFlightByCurrency = <CurrencyPreference, Future<ArrrPriceQuote?>>{};
  static Future<Map<CurrencyPreference, double>?>? _coinGeckoInFlight;

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

    final existingRequest = _inFlightByCurrency[currency];
    if (existingRequest != null) {
      return await existingRequest;
    }

    final request = _inFlightByCurrency.putIfAbsent(
      currency,
      () => _fetchFresh(currency),
    );
    try {
      return await request;
    } finally {
      if (identical(_inFlightByCurrency[currency], request)) {
        // The removed future is the request already awaited above.
        unawaited(_inFlightByCurrency.remove(currency));
      }
    }
  }

  static Future<ArrrPriceQuote?> _fetchFresh(
    CurrencyPreference currency,
  ) async {
    final now = DateTime.now();
    final geckoPrices = await _fetchCoingeckoPrices();
    if (geckoPrices != null) {
      for (final entry in geckoPrices.entries) {
        _lastByCurrency[entry.key] = ArrrPriceQuote(
          currency: entry.key,
          pricePerArrr: entry.value,
          fetchedAt: now,
          source: ArrrPriceSource.coingecko,
        );
      }
      final quote = _lastByCurrency[currency];
      if (quote != null) {
        return quote;
      }
    }

    final paprikaPrice = await _fetchCoinPaprikaPrice(currency);
    if (paprikaPrice != null && paprikaPrice > 0) {
      final quote = ArrrPriceQuote(
        currency: currency,
        pricePerArrr: paprikaPrice,
        fetchedAt: DateTime.now(),
        source: ArrrPriceSource.coinPaprika,
      );
      _lastByCurrency[currency] = quote;
      return quote;
    }

    if (currency != CurrencyPreference.usd) {
      return null;
    }

    final cmcUsd = await _fetchCoinMarketCapUsd();
    if (cmcUsd == null || cmcUsd <= 0) {
      return null;
    }

    final quote = ArrrPriceQuote(
      currency: currency,
      pricePerArrr: cmcUsd,
      fetchedAt: DateTime.now(),
      source: ArrrPriceSource.coinMarketCap,
    );
    _lastByCurrency[currency] = quote;
    return quote;
  }

  static AssetUsdPriceQuote? cachedAssetUsd(String assetId) {
    final existing = _lastAssetUsdById[assetId];
    if (existing == null) return null;
    if (DateTime.now().difference(existing.fetchedAt) > _cacheTtl) {
      return null;
    }
    return existing;
  }

  static Future<AssetUsdPriceQuote?> fetchAssetUsd({
    required String assetId,
    required String ticker,
  }) async {
    final cachedQuote = cachedAssetUsd(assetId);
    if (cachedQuote != null) return cachedQuote;

    try {
      final uri = Uri.parse(
        'https://api.coingecko.com/api/v3/simple/price?ids=$assetId&vs_currencies=usd',
      );
      final json = await _downloadJson(uri, userAgent: 'StashiWallet');
      if (json is! Map) return null;
      final asset = json[assetId];
      if (asset is! Map) return null;
      final price = _parsePrice(asset['usd']);
      if (price == null || price <= 0) return null;

      final quote = AssetUsdPriceQuote(
        assetId: assetId,
        ticker: ticker,
        pricePerUnit: price,
        fetchedAt: DateTime.now(),
      );
      _lastAssetUsdById[assetId] = quote;
      return quote;
    } catch (_) {
      return null;
    }
  }

  static Future<Map<CurrencyPreference, double>?>
  _fetchCoingeckoPrices() async {
    final existingRequest = _coinGeckoInFlight;
    if (existingRequest != null) {
      return existingRequest;
    }

    final request = _fetchCoingeckoPricesUncached();
    _coinGeckoInFlight = request;
    try {
      return await request;
    } finally {
      if (identical(_coinGeckoInFlight, request)) {
        _coinGeckoInFlight = null;
      }
    }
  }

  static Future<Map<CurrencyPreference, double>?>
  _fetchCoingeckoPricesUncached() async {
    try {
      final vsCurrencies = _vsCurrencyCodes.values.join(',');
      final uri = Uri.parse(
        'https://api.coingecko.com/api/v3/simple/price?ids=pirate-chain&vs_currencies=$vsCurrencies',
      );
      final json = await _downloadJson(uri, userAgent: 'StashiWallet');
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

  static Future<double?> _fetchCoinPaprikaPrice(
    CurrencyPreference currency,
  ) async {
    final quoteCode = coinPaprikaQuoteCodeFor(currency);
    if (quoteCode == null) {
      return null;
    }

    try {
      final uri = Uri.parse(
        '$_coinPaprikaTicker?quotes=${Uri.encodeQueryComponent(quoteCode)}',
      );
      final json = await _downloadJson(uri, userAgent: 'StashiWallet');
      return parseCoinPaprikaPrice(json, quoteCode);
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
      final json = await _downloadJson(uri, userAgent: 'StashiWallet');
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
      final json = await _downloadJson(uri, userAgent: 'StashiWallet');
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
    try {
      final body = await FfiBridge.fetchExternalText(
        url: uri.toString(),
        accept: 'application/json',
        userAgent: userAgent,
      ).timeout(_timeout);
      return jsonDecode(body);
    } catch (_) {
      return null;
    }
  }

  static double? _parsePrice(dynamic value) {
    if (value is num) return value.toDouble();
    return double.tryParse(value?.toString() ?? '');
  }
}

@visibleForTesting
String? coinPaprikaQuoteCodeFor(CurrencyPreference currency) =>
    _ArrrPriceService._coinPaprikaQuoteCodes[currency];

@visibleForTesting
double? parseCoinPaprikaPrice(dynamic json, String quoteCode) {
  if (json is! Map || json['id'] != 'arrr-pirate' || json['symbol'] != 'ARRR') {
    return null;
  }
  final quotes = json['quotes'];
  if (quotes is! Map) return null;
  final quote = quotes[quoteCode.toUpperCase()];
  if (quote is! Map) return null;
  final value = quote['price'];
  final price = value is num
      ? value.toDouble()
      : double.tryParse(value?.toString() ?? '');
  return price != null && price > 0 ? price : null;
}

class PriceFeedRefreshNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void requestRefresh() {
    state = state + 1;
  }
}

final priceFeedRefreshProvider =
    NotifierProvider<PriceFeedRefreshNotifier, int>(
      PriceFeedRefreshNotifier.new,
    );

@visibleForTesting
class PriceQuotePoller<T> {
  PriceQuotePoller({
    required this.fetch,
    T? initialValue,
    this.refreshInterval = const Duration(seconds: 45),
    this.retryDelays = const [
      Duration(seconds: 3),
      Duration(seconds: 10),
      Duration(seconds: 30),
    ],
  }) : _last = initialValue,
       assert(retryDelays.isNotEmpty, 'retryDelays must not be empty') {
    _controller = StreamController<T?>(onListen: _start);
  }

  final Future<T?> Function() fetch;
  final Duration refreshInterval;
  final List<Duration> retryDelays;
  late final StreamController<T?> _controller;

  Timer? _timer;
  T? _last;
  int _consecutiveFailures = 0;
  bool _started = false;
  bool _disposed = false;
  bool _refreshing = false;
  bool _refreshRequested = false;
  bool _emittedUnavailable = false;

  Stream<T?> get stream => _controller.stream;

  void _start() {
    if (_started || _disposed) return;
    _started = true;
    final initial = _last;
    if (initial != null) {
      _controller.add(initial);
    }
    unawaited(_refresh());
  }

  void refreshNow() {
    if (_disposed || !_started) return;
    _timer?.cancel();
    if (_refreshing) {
      _refreshRequested = true;
      return;
    }
    unawaited(_refresh());
  }

  Future<void> _refresh() async {
    if (_disposed || _refreshing) return;
    _refreshing = true;

    T? quote;
    try {
      quote = await fetch();
    } catch (_) {
      quote = null;
    }

    if (_disposed) {
      _refreshing = false;
      return;
    }

    Duration nextDelay;
    if (quote != null) {
      _last = quote;
      _consecutiveFailures = 0;
      _emittedUnavailable = false;
      _controller.add(quote);
      nextDelay = refreshInterval;
    } else {
      _consecutiveFailures += 1;
      if (_last == null && !_emittedUnavailable) {
        _emittedUnavailable = true;
        _controller.add(null);
      }
      final retryIndex = _consecutiveFailures <= retryDelays.length
          ? _consecutiveFailures - 1
          : retryDelays.length - 1;
      nextDelay = retryDelays[retryIndex];
    }

    _refreshing = false;
    if (_refreshRequested) {
      _refreshRequested = false;
      unawaited(_refresh());
      return;
    }
    _timer = Timer(nextDelay, () => unawaited(_refresh()));
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _timer?.cancel();
    if (!_controller.isClosed) {
      unawaited(_controller.close());
    }
  }
}

final arrrPriceQuoteProvider = StreamProvider<ArrrPriceQuote?>((ref) {
  final currency = ref.watch(currencyPreferenceProvider);
  return _priceQuoteStream(ref, currency);
});

final arrrUsdPriceQuoteProvider = StreamProvider<ArrrPriceQuote?>((ref) {
  return _priceQuoteStream(ref, CurrencyPreference.usd);
});

final ltcUsdPriceQuoteProvider = StreamProvider<AssetUsdPriceQuote?>((ref) {
  return _assetUsdPriceQuoteStream(ref, assetId: 'litecoin', ticker: 'LTC');
});

Stream<ArrrPriceQuote?> _priceQuoteStream(
  Ref ref,
  CurrencyPreference currency,
) {
  final allowPrices = ref.watch(allowPriceApisProvider);

  if (!allowPrices || kIsWeb) {
    return Stream<ArrrPriceQuote?>.value(null);
  }

  final poller = PriceQuotePoller<ArrrPriceQuote>(
    fetch: () => _ArrrPriceService.fetch(currency),
    initialValue: _ArrrPriceService.cached(currency),
  );
  ref
    ..listen(priceFeedRefreshProvider, (_, _) {
      poller.refreshNow();
    })
    ..onDispose(poller.dispose);
  return poller.stream;
}

Stream<AssetUsdPriceQuote?> _assetUsdPriceQuoteStream(
  Ref ref, {
  required String assetId,
  required String ticker,
}) {
  final allowPrices = ref.watch(allowPriceApisProvider);

  if (!allowPrices || kIsWeb) {
    return Stream<AssetUsdPriceQuote?>.value(null);
  }

  final poller = PriceQuotePoller<AssetUsdPriceQuote>(
    fetch: () =>
        _ArrrPriceService.fetchAssetUsd(assetId: assetId, ticker: ticker),
    initialValue: _ArrrPriceService.cachedAssetUsd(assetId),
  );
  ref
    ..listen(priceFeedRefreshProvider, (_, _) {
      poller.refreshNow();
    })
    ..onDispose(poller.dispose);
  return poller.stream;
}
