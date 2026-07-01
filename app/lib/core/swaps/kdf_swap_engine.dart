import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:decimal/decimal.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:komodo_defi_sdk/komodo_defi_sdk.dart';
import 'package:komodo_defi_types/komodo_defi_types.dart' as kdf_types;

import '../ffi/ffi_bridge.dart';
import 'kdf_orderbook_parser.dart';
import 'kdf_swap_payloads.dart';
import 'swap_models.dart';

class KdfSwapEngineException implements Exception {
  const KdfSwapEngineException(this.message, [this.cause]);

  final String message;
  final Object? cause;

  @override
  String toString() => cause == null ? message : '$message: $cause';
}

typedef KdfSwapNetworkPolicyReader = KdfSwapNetworkPolicy Function();
typedef KdfSwapStartupForTesting =
    Future<void> Function(
      String walletId,
      KdfSwapNetworkPolicy policy,
      int generation,
    );

enum _KdfSeedMatch { canonical, legacyLocalized, mismatch }

class KdfSwapNetworkPolicy {
  const KdfSwapNetworkPolicy._({
    required this.name,
    required this.isSupported,
    this.blockedMessage,
  });

  const KdfSwapNetworkPolicy.direct()
    : this._(name: 'direct', isSupported: true);

  const KdfSwapNetworkPolicy.tor() : this._(name: 'tor', isSupported: true);

  const KdfSwapNetworkPolicy.socks5()
    : this._(name: 'socks5', isSupported: true);

  const KdfSwapNetworkPolicy.blocked(this.name, this.blockedMessage)
    : isSupported = false;

  final String name;
  final bool isSupported;
  final String? blockedMessage;

  void assertSupported() {
    if (isSupported) return;
    throw KdfSwapEngineException(
      blockedMessage ??
          'Swaps are disabled because KDF cannot honor $name networking.',
    );
  }
}

bool isKdfInsufficientBalanceError(Object error, {String? coin}) {
  final normalizedCoin = coin?.toUpperCase();
  final candidate = error is KdfSwapEngineException ? error.cause : error;
  final text = error.toString();

  if (_hasInsufficientBalance(candidate, normalizedCoin)) return true;
  if (!text.contains('NotSufficientBalance')) return false;
  return normalizedCoin == null || text.toUpperCase().contains(normalizedCoin);
}

bool _hasInsufficientBalance(Object? value, String? normalizedCoin) {
  if (value is Map) {
    final json = Map<String, dynamic>.from(value);
    if (json['error_type'] == 'NotSufficientBalance') {
      if (normalizedCoin == null) return true;
      final errorData = json['error_data'];
      if (errorData is Map) {
        return errorData['coin']?.toString().toUpperCase() == normalizedCoin;
      }
      return json.toString().toUpperCase().contains(normalizedCoin);
    }
    return json.values.any(
      (entry) => _hasInsufficientBalance(entry, normalizedCoin),
    );
  }
  if (value is Iterable) {
    return value.any((entry) => _hasInsufficientBalance(entry, normalizedCoin));
  }
  return false;
}

class KdfSwapEngine {
  KdfSwapEngine({
    FlutterSecureStorage? storage,
    KdfSwapNetworkPolicyReader? networkPolicyReader,
    KdfSwapStartupForTesting? startupForTesting,
  }) : _storage = storage ?? const FlutterSecureStorage(),
       _networkPolicyReader = networkPolicyReader,
       _startupForTesting = startupForTesting;

  static const supportedBase = 'ARRR';
  static const supportedRel = 'LTC';
  static const supportedVarrr = 'vARRR';
  static const _walletPasswordKeyPrefix = 'pirate_kdf_wallet_password_v1';

  final FlutterSecureStorage _storage;
  final KdfSwapNetworkPolicyReader? _networkPolicyReader;
  final KdfSwapStartupForTesting? _startupForTesting;
  KomodoDefiSdk? _sdk;
  String? _walletId;
  String? _rpcPassword;
  KdfSwapNetworkPolicy? _networkPolicy;
  Future<void>? _startup;
  String? _startupWalletId;
  KdfSwapNetworkPolicy? _startupNetworkPolicy;
  int _lifecycleGeneration = 0;

  bool get isRunning => _sdk != null && _walletId != null;

  Future<void> ensureStarted(String walletId) async {
    final policy = _currentNetworkPolicy()..assertSupported();
    if (_sdk != null &&
        _walletId == walletId &&
        _sameNetworkPolicy(_networkPolicy, policy)) {
      return;
    }
    final existingStartup = _startup;
    if (existingStartup != null &&
        _startupWalletId == walletId &&
        _sameNetworkPolicy(_startupNetworkPolicy, policy)) {
      return existingStartup;
    }
    final generation = ++_lifecycleGeneration;
    final startup =
        _startupForTesting?.call(walletId, policy, generation) ??
        _start(walletId, policy, generation);
    _startup = startup;
    _startupWalletId = walletId;
    _startupNetworkPolicy = policy;
    try {
      await startup;
    } finally {
      if (identical(_startup, startup)) {
        _startup = null;
        _startupWalletId = null;
        _startupNetworkPolicy = null;
      }
    }
  }

  Future<void> activateArrrLtc() async {
    await activatePair(SwapPair.arrrLtc);
  }

  Future<void> activatePair(SwapPair pair) async {
    final sdk = _requireSdk();
    await _configureArrrParamsIfAvailable(sdk);
    for (final ticker in [pair.relTicker, pair.baseTicker]) {
      final asset = _findAsset(sdk, ticker);
      await sdk.assets.activateAsset(asset).drain<void>();
    }
  }

  Future<String> getLtcDepositAddress() => _getDepositAddress(supportedRel);

  Future<String> getArrrDepositAddress() => _getDepositAddress(supportedBase);

  Future<String> getDepositAddress(SwapAsset asset) {
    return _getDepositAddress(asset.ticker);
  }

  Future<String> _getDepositAddress(String ticker) async {
    final sdk = _requireSdk();
    final asset = _findAsset(sdk, ticker);
    final pubkeys = await sdk.pubkeys.getPubkeys(asset);
    final activeKeys = pubkeys.keys.where((key) => key.isActiveForSwap);
    final activeKey = activeKeys.isNotEmpty
        ? activeKeys.first
        : (pubkeys.keys.isNotEmpty ? pubkeys.keys.first : null);
    if (activeKey == null) {
      throw KdfSwapEngineException(
        'KDF did not return a $ticker deposit address.',
      );
    }
    return activeKey.address;
  }

  Future<List<SwapOrderbookLevel>> loadArrrLtcAsks() async {
    return (await loadArrrLtcOrderbook()).asks;
  }

  Future<List<SwapOrderbookLevel>> loadAsks(SwapPair pair) async {
    return (await loadOrderbook(pair)).asks;
  }

  Future<List<SwapOrderbookLevel>> loadArrrLtcBids() async {
    return (await loadArrrLtcOrderbook()).bids;
  }

  Future<List<SwapOrderbookLevel>> loadBids(SwapPair pair) async {
    return (await loadOrderbook(pair)).bids;
  }

  Future<KdfOrderbook> loadArrrLtcOrderbook() async {
    return loadOrderbook(SwapPair.arrrLtc);
  }

  Future<KdfOrderbook> loadOrderbook(SwapPair pair) async {
    const attempts = 4;
    KdfOrderbook? lastBook;
    for (var attempt = 0; attempt < attempts; attempt += 1) {
      final response = await _rpc(
        KdfSwapPayloads.orderbook(
          rpcPass: _requireRpcPass(),
          base: pair.baseTicker,
          rel: pair.relTicker,
        ),
      );
      final book = parseKdfOrderbook(response);
      if (!book.isEmpty || attempt == attempts - 1) return book;
      lastBook = book;
      await Future<void>.delayed(const Duration(milliseconds: 1500));
    }
    return lastBook ?? const KdfOrderbook(asks: [], bids: []);
  }

  Future<Map<String, dynamic>> tradePreimageForBuy(SwapPlan plan) {
    return tradePreimageForBuyPair(plan, pair: SwapPair.arrrLtc);
  }

  Future<Map<String, dynamic>> tradePreimageForBuyPair(
    SwapPlan plan, {
    required SwapPair pair,
  }) {
    return _rpc(
      KdfSwapPayloads.tradePreimage(
        rpcPass: _requireRpcPass(),
        base: pair.baseTicker,
        rel: pair.relTicker,
        swapMethod: 'buy',
        volume: plan.marketArrrAmount.toString(),
        price: plan.referencePriceLtcPerArrr.toString(),
      ),
    );
  }

  Future<String> startMarketBuy(SwapPlan plan) async {
    return startMarketBuyPair(plan, pair: SwapPair.arrrLtc);
  }

  Future<String> startMarketBuyPair(
    SwapPlan plan, {
    required SwapPair pair,
  }) async {
    if (!plan.hasMarketFill) {
      throw const KdfSwapEngineException(
        'Market buy requested with no market-fill plan.',
      );
    }
    final response = await _rpc(
      KdfSwapPayloads.startSwap(
        rpcPass: _requireRpcPass(),
        base: pair.baseTicker,
        rel: pair.relTicker,
        baseAmount: plan.marketArrrAmount.toString(),
        relAmount: plan.marketLtcAmount.toString(),
        method: 'buy',
      ),
    );
    return _uuidFromResponse(response, 'swap');
  }

  Future<String> placeRemainderBuyLimit(SwapPlan plan) async {
    return placeRemainderBuyLimitPair(plan, pair: SwapPair.arrrLtc);
  }

  Future<String> placeRemainderBuyLimitPair(
    SwapPlan plan, {
    required SwapPair pair,
  }) async {
    final limitPrice = plan.limitPriceLtcPerArrr;
    if (limitPrice == null ||
        plan.remainderLtcAmount.compareTo(Decimal.zero) <= 0) {
      throw const KdfSwapEngineException(
        'Limit buy requested with no remainder.',
      );
    }

    final priceArrrPerLtc = _divide(Decimal.one, limitPrice, scale: 8);
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: pair.relTicker,
        rel: pair.baseTicker,
        price: priceArrrPerLtc.toString(),
        volume: plan.remainderLtcAmount.toString(),
      ),
    );
    return _uuidFromResponse(response, 'order');
  }

  Future<String> startMarketSell({
    required Decimal arrrAmount,
    required Decimal expectedLtcAmount,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    final response = await _rpc(
      KdfSwapPayloads.startSwap(
        rpcPass: _requireRpcPass(),
        base: pair.baseTicker,
        rel: pair.relTicker,
        baseAmount: arrrAmount.toString(),
        relAmount: expectedLtcAmount.toString(),
        method: 'sell',
      ),
    );
    return _uuidFromResponse(response, 'swap');
  }

  Future<String> placeSellLimit({
    required Decimal arrrAmount,
    required Decimal priceLtcPerArrr,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: pair.baseTicker,
        rel: pair.relTicker,
        price: priceLtcPerArrr.toString(),
        volume: arrrAmount.toString(),
      ),
    );
    return _uuidFromResponse(response, 'order');
  }

  Future<void> cancelOrder(String uuid) async {
    await _rpc(
      KdfSwapPayloads.cancelOrder(rpcPass: _requireRpcPass(), uuid: uuid),
    );
  }

  Future<String> modifyBuyLimitOrder({
    required String existingUuid,
    required Decimal newPriceLtcPerArrr,
    required Decimal ltcVolume,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await cancelOrder(existingUuid);
    final priceArrrPerLtc = _divide(Decimal.one, newPriceLtcPerArrr, scale: 8);
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: pair.relTicker,
        rel: pair.baseTicker,
        price: priceArrrPerLtc.toString(),
        volume: ltcVolume.toString(),
      ),
    );
    return _uuidFromResponse(response, 'order');
  }

  Future<String> modifySellLimitOrder({
    required String existingUuid,
    required Decimal newPriceLtcPerArrr,
    required Decimal arrrVolume,
    SwapPair pair = SwapPair.arrrLtc,
  }) async {
    await cancelOrder(existingUuid);
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: pair.baseTicker,
        rel: pair.relTicker,
        price: newPriceLtcPerArrr.toString(),
        volume: arrrVolume.toString(),
      ),
    );
    return _uuidFromResponse(response, 'order');
  }

  Future<Map<String, dynamic>> swapStatus(String uuid) {
    return _rpc(
      KdfSwapPayloads.swapStatus(rpcPass: _requireRpcPass(), uuid: uuid),
    );
  }

  Future<Map<String, dynamic>> myOrders() {
    return _rpc(KdfSwapPayloads.myOrders(rpcPass: _requireRpcPass()));
  }

  Future<Decimal> ltcBalance() => _coinBalance(supportedRel);

  Future<Decimal> arrrBalance() => _coinBalance(supportedBase);

  Future<Decimal> coinBalance(SwapAsset asset) => _coinBalance(asset.ticker);

  Future<Decimal> relBalance(SwapPair pair) => coinBalance(pair.relAsset);

  Future<Decimal> _coinBalance(String coin) async {
    final sdk = _requireSdk();
    final asset = _findAsset(sdk, coin);
    try {
      final balance = await sdk.balances.getBalance(asset.id);
      return balance.spendable;
    } catch (error) {
      throw KdfSwapEngineException('Failed to read $coin KDF balance', error);
    }
  }

  Future<Map<String, dynamic>> withdrawArrrToWallet({
    required String address,
    required Decimal amount,
  }) {
    return _rpc(
      KdfSwapPayloads.withdraw(
        rpcPass: _requireRpcPass(),
        coin: supportedBase,
        to: address,
        amount: amount,
      ),
    );
  }

  Future<Map<String, dynamic>> withdrawAllArrrToWallet({
    required String address,
  }) {
    return _rpc(
      KdfSwapPayloads.withdraw(
        rpcPass: _requireRpcPass(),
        coin: supportedBase,
        to: address,
        max: true,
      ),
    );
  }

  Future<Map<String, dynamic>> withdrawLtc({
    required String address,
    required Decimal amount,
  }) {
    return withdrawAsset(
      asset: SwapAsset.ltc,
      address: address,
      amount: amount,
    );
  }

  Future<Map<String, dynamic>> withdrawAsset({
    required SwapAsset asset,
    required String address,
    required Decimal amount,
  }) {
    return _rpc(
      KdfSwapPayloads.withdraw(
        rpcPass: _requireRpcPass(),
        coin: asset.ticker,
        to: address,
        amount: amount,
      ),
    );
  }

  Future<Map<String, dynamic>> withdrawAllLtc({required String address}) {
    return withdrawAllAsset(asset: SwapAsset.ltc, address: address);
  }

  Future<Map<String, dynamic>> withdrawAllAsset({
    required SwapAsset asset,
    required String address,
  }) {
    return _rpc(
      KdfSwapPayloads.withdraw(
        rpcPass: _requireRpcPass(),
        coin: asset.ticker,
        to: address,
        max: true,
      ),
    );
  }

  Future<void> dispose() async {
    _lifecycleGeneration += 1;
    _startup = null;
    _startupWalletId = null;
    _startupNetworkPolicy = null;
    await _disposeSdk();
  }

  Future<void> _disposeSdk() async {
    final sdk = _sdk;
    _sdk = null;
    _walletId = null;
    _rpcPassword = null;
    _networkPolicy = null;
    if (sdk == null) return;
    try {
      await sdk.auth.signOut();
    } catch (_) {
      // The SDK may already be signed out while shutting down.
    }
    await sdk.dispose();
  }

  Future<void> _start(
    String walletId,
    KdfSwapNetworkPolicy policy,
    int generation,
  ) async {
    await _disposeSdk();

    policy.assertSupported();
    // KDF is registered with the same seed as the active Unified Wallet. If a
    // swap gets interrupted, the KDF balances remain recoverable in Komodo
    // Wallet by restoring that same seed.
    var seed = await FfiBridge.exportSeedForKdf(walletId);
    var originalSeed = await FfiBridge.exportSeedRaw(walletId);
    final walletPassword = await _getOrCreateKdfWalletPassword(walletId);
    final rpcPassword = _randomSecret(32);
    final walletName = _kdfWalletName(walletId);
    final sdk = KomodoDefiSdk(
      host: LocalConfig(https: false, rpcPassword: rpcPassword),
      config: const KomodoDefiSdkConfig(
        defaultAssets: {supportedBase, supportedRel, supportedVarrr},
        preActivateDefaultAssets: false,
        preActivateHistoricalAssets: false,
        preActivateCustomTokenAssets: false,
      ),
    );

    try {
      await sdk.initialize();
      final options = kdf_types.AuthOptions(
        derivationMethod: kdf_types.DerivationMethod.hdWallet,
        allowWeakPassword: true,
      );
      var useCanonicalWallet = false;
      try {
        await _registerOrSignIn(
          sdk: sdk,
          walletName: walletName,
          walletPassword: walletPassword,
          options: options,
          seed: seed,
        );
      } catch (error) {
        if (!_looksLikeInvalidBip39Seed(error)) rethrow;
        useCanonicalWallet = true;
      }

      if (!useCanonicalWallet) {
        switch (await _kdfSeedMatch(
          sdk,
          canonicalSeed: seed,
          originalSeed: originalSeed,
          walletPassword: walletPassword,
        )) {
          case _KdfSeedMatch.canonical:
            break;
          case _KdfSeedMatch.legacyLocalized:
            await sdk.auth.signOut();
            useCanonicalWallet = true;
            break;
          case _KdfSeedMatch.mismatch:
            throw const KdfSwapEngineException(
              'The KDF wallet seed does not match the active Unified Wallet.',
            );
        }
      }

      if (useCanonicalWallet) {
        // Older builds could register a localized mnemonic before the SDK's
        // English-only BIP39 validation rejected it. Preserve that record and
        // use a stable corrected wallet name for the canonical English seed.
        await _registerOrSignIn(
          sdk: sdk,
          walletName: _canonicalKdfWalletName(walletId),
          walletPassword: walletPassword,
          options: options,
          seed: seed,
        );
        await _assertKdfSeedMatches(sdk, seed, walletPassword);
      }
      if (generation != _lifecycleGeneration) {
        await sdk.dispose();
        return;
      }
      _sdk = sdk;
      _walletId = walletId;
      _rpcPassword = rpcPassword;
      _networkPolicy = policy;
    } catch (error) {
      await sdk.dispose();
      throw KdfSwapEngineException('Failed to start KDF swap engine', error);
    } finally {
      seed = '';
      originalSeed = '';
    }
  }

  Future<void> _registerOrSignIn({
    required KomodoDefiSdk sdk,
    required String walletName,
    required String walletPassword,
    required kdf_types.AuthOptions options,
    required String seed,
  }) async {
    try {
      await sdk.auth.register(
        walletName: walletName,
        password: walletPassword,
        options: options,
        mnemonic: kdf_types.Mnemonic.plaintext(seed),
      );
    } catch (error) {
      if (!_looksLikeExistingWallet(error)) rethrow;
      await sdk.auth.signIn(
        walletName: walletName,
        password: walletPassword,
        options: options,
      );
    }
  }

  Future<void> _assertKdfSeedMatches(
    KomodoDefiSdk sdk,
    String expectedSeed,
    String walletPassword,
  ) async {
    final mnemonic = await sdk.auth.getMnemonicPlainText(walletPassword);
    if (mnemonic.plaintextMnemonic?.trim() != expectedSeed.trim()) {
      throw const KdfSwapEngineException(
        'The KDF wallet seed does not match the active Unified Wallet.',
      );
    }
  }

  Future<_KdfSeedMatch> _kdfSeedMatch(
    KomodoDefiSdk sdk, {
    required String canonicalSeed,
    required String originalSeed,
    required String walletPassword,
  }) async {
    final mnemonic = await sdk.auth.getMnemonicPlainText(walletPassword);
    final actualSeed = mnemonic.plaintextMnemonic?.trim();
    if (actualSeed == canonicalSeed.trim()) return _KdfSeedMatch.canonical;
    if (originalSeed.trim() != canonicalSeed.trim() &&
        actualSeed == originalSeed.trim()) {
      return _KdfSeedMatch.legacyLocalized;
    }
    return _KdfSeedMatch.mismatch;
  }

  Future<void> _configureArrrParamsIfAvailable(KomodoDefiSdk sdk) async {
    final arrr = _findAsset(sdk, supportedBase);
    final paramsPath =
        await ZcashParamsDownloaderFactory.getDefaultParamsPath();
    if (paramsPath == null || paramsPath.trim().isEmpty) return;
    await sdk.activationConfigService.saveZhtlcConfig(
      arrr.id,
      ZhtlcUserConfig(zcashParamsPath: paramsPath),
    );
  }

  Future<String> _getOrCreateKdfWalletPassword(String walletId) async {
    final key = '$_walletPasswordKeyPrefix.$walletId';
    final existing = await _storage.read(key: key);
    if (existing != null && existing.isNotEmpty) return existing;

    final generated = _randomSecret(48);
    await _storage.write(key: key, value: generated);
    return generated;
  }

  Future<Map<String, dynamic>> _rpc(Map<String, dynamic> payload) async {
    _currentNetworkPolicy().assertSupported();
    _assertNoGleecEndpoint(payload);
    final response = await _requireSdk().client.executeRpc(payload);
    final responseMap = Map<String, dynamic>.from(response);
    final error = responseMap['error'] ?? responseMap['error_data'];
    if (error != null) {
      final method = payload['method']?.toString() ?? 'KDF RPC';
      throw KdfSwapEngineException('$method failed', responseMap);
    }
    return responseMap;
  }

  kdf_types.Asset _findAsset(KomodoDefiSdk sdk, String ticker) {
    final matches = sdk.assets.findAssetsByConfigId(ticker);
    if (matches.isEmpty) {
      throw KdfSwapEngineException(
        'KDF asset config for $ticker was not found.',
      );
    }
    return matches.first;
  }

  KomodoDefiSdk _requireSdk() {
    final sdk = _sdk;
    if (sdk == null) {
      throw const KdfSwapEngineException('KDF swap engine is not running.');
    }
    return sdk;
  }

  String _requireRpcPass() {
    final pass = _rpcPassword;
    if (pass == null || pass.isEmpty) {
      throw const KdfSwapEngineException(
        'KDF RPC password is not initialized.',
      );
    }
    return pass;
  }

  KdfSwapNetworkPolicy _currentNetworkPolicy() {
    return _networkPolicyReader?.call() ?? const KdfSwapNetworkPolicy.direct();
  }

  bool _sameNetworkPolicy(KdfSwapNetworkPolicy? a, KdfSwapNetworkPolicy b) {
    return a != null && a.name == b.name && a.isSupported == b.isSupported;
  }

  Decimal _divide(Decimal a, Decimal b, {required int scale}) {
    return (a / b).toDecimal(scaleOnInfinitePrecision: scale);
  }

  String _uuidFromResponse(Map<String, dynamic> response, String noun) {
    final result = Map<String, dynamic>.from(
      response['result'] as Map? ?? const {},
    );
    final uuid = result['uuid']?.toString();
    if (uuid == null || uuid.isEmpty) {
      throw KdfSwapEngineException(
        'KDF did not return a $noun UUID.',
        response,
      );
    }
    return uuid;
  }

  bool _looksLikeExistingWallet(Object error) {
    final message = error.toString().toLowerCase();
    return message.contains('already') ||
        message.contains('exist') ||
        message.contains('duplicate');
  }

  bool _looksLikeInvalidBip39Seed(Object error) {
    return error.toString().toLowerCase().contains('bip39');
  }

  String _kdfWalletName(String walletId) {
    final digest = sha256.convert(utf8.encode(walletId)).toString();
    return 'pirate_swap_${digest.substring(0, 24)}';
  }

  String _canonicalKdfWalletName(String walletId) {
    return '${_kdfWalletName(walletId)}_bip39';
  }

  String _randomSecret(int length) {
    final random = Random.secure();
    final bytes = List<int>.generate(length, (_) => random.nextInt(256));
    return base64UrlEncode(bytes);
  }

  void _assertNoGleecEndpoint(Object? value) {
    if (value is String && value.toLowerCase().contains('gleec')) {
      throw const KdfSwapEngineException(
        'Refusing to use Gleec swap endpoint/config.',
      );
    }
    if (value is Map) {
      for (final entry in value.entries) {
        _assertNoGleecEndpoint(entry.key);
        _assertNoGleecEndpoint(entry.value);
      }
    } else if (value is Iterable) {
      value.forEach(_assertNoGleecEndpoint);
    }
  }
}
