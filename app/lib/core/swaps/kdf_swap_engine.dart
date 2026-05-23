import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:decimal/decimal.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:komodo_defi_sdk/komodo_defi_sdk.dart';
import 'package:komodo_defi_types/komodo_defi_types.dart' as kdf_types;

import '../ffi/ffi_bridge.dart';
import 'kdf_swap_payloads.dart';
import 'swap_models.dart';

class KdfSwapEngineException implements Exception {
  const KdfSwapEngineException(this.message, [this.cause]);

  final String message;
  final Object? cause;

  @override
  String toString() => cause == null ? message : '$message: $cause';
}

class KdfSwapEngine {
  KdfSwapEngine({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  static const supportedBase = 'ARRR';
  static const supportedRel = 'LTC';
  static const _walletPasswordKeyPrefix = 'pirate_kdf_wallet_password_v1';

  final FlutterSecureStorage _storage;
  KomodoDefiSdk? _sdk;
  String? _walletId;
  String? _rpcPassword;
  Future<void>? _startup;

  bool get isRunning => _sdk != null && _walletId != null;

  Future<void> ensureStarted(String walletId) async {
    if (_sdk != null && _walletId == walletId) return;
    if (_startup != null) return _startup;
    _startup = _start(walletId);
    try {
      await _startup;
    } finally {
      _startup = null;
    }
  }

  Future<void> activateArrrLtc() async {
    final sdk = _requireSdk();
    await _configureArrrParamsIfAvailable(sdk);
    for (final ticker in const [supportedRel, supportedBase]) {
      final asset = _findAsset(sdk, ticker);
      await sdk.assets.activateAsset(asset).drain<void>();
    }
  }

  Future<String> getLtcDepositAddress() async {
    final sdk = _requireSdk();
    final ltc = _findAsset(sdk, supportedRel);
    final pubkeys = await sdk.pubkeys.getPubkeys(ltc);
    final activeKeys = pubkeys.keys.where((key) => key.isActiveForSwap);
    final activeKey = activeKeys.isNotEmpty
        ? activeKeys.first
        : (pubkeys.keys.isNotEmpty ? pubkeys.keys.first : null);
    if (activeKey == null) {
      throw const KdfSwapEngineException(
        'KDF did not return an LTC deposit address.',
      );
    }
    return activeKey.address;
  }

  Future<List<SwapOrderbookLevel>> loadArrrLtcAsks() async {
    final response = await _rpc(
      KdfSwapPayloads.orderbook(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
      ),
    );
    final result = Map<String, dynamic>.from(
      response['result'] as Map? ?? const {},
    );
    final asks = result['asks'] as List? ?? const [];
    return asks.map(_orderbookLevel).whereType<SwapOrderbookLevel>().toList();
  }

  Future<List<SwapOrderbookLevel>> loadArrrLtcBids() async {
    final response = await _rpc(
      KdfSwapPayloads.orderbook(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
      ),
    );
    final result = Map<String, dynamic>.from(
      response['result'] as Map? ?? const {},
    );
    final bids = result['bids'] as List? ?? const [];
    return bids.map(_orderbookLevel).whereType<SwapOrderbookLevel>().toList();
  }

  Future<Map<String, dynamic>> tradePreimageForBuy(SwapPlan plan) {
    return _rpc(
      KdfSwapPayloads.tradePreimage(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
        swapMethod: 'buy',
        volume: plan.marketArrrAmount.toString(),
      ),
    );
  }

  Future<String> startMarketBuy(SwapPlan plan) async {
    if (!plan.hasMarketFill) {
      throw const KdfSwapEngineException(
        'Market buy requested with no market-fill plan.',
      );
    }
    final response = await _rpc(
      KdfSwapPayloads.startSwap(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
        baseAmount: plan.marketArrrAmount.toString(),
        relAmount: plan.marketLtcAmount.toString(),
        method: 'buy',
      ),
    );
    return _uuidFromResponse(response, 'swap');
  }

  Future<String> placeRemainderBuyLimit(SwapPlan plan) async {
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
        base: supportedRel,
        rel: supportedBase,
        price: priceArrrPerLtc.toString(),
        volume: plan.remainderLtcAmount.toString(),
      ),
    );
    return _uuidFromResponse(response, 'order');
  }

  Future<String> startMarketSell({
    required Decimal arrrAmount,
    required Decimal expectedLtcAmount,
  }) async {
    final response = await _rpc(
      KdfSwapPayloads.startSwap(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
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
  }) async {
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
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
  }) async {
    await cancelOrder(existingUuid);
    final priceArrrPerLtc = _divide(Decimal.one, newPriceLtcPerArrr, scale: 8);
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: supportedRel,
        rel: supportedBase,
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
  }) async {
    await cancelOrder(existingUuid);
    final response = await _rpc(
      KdfSwapPayloads.setOrder(
        rpcPass: _requireRpcPass(),
        base: supportedBase,
        rel: supportedRel,
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

  Future<Decimal> ltcBalance() async {
    final response = await _rpc(
      KdfSwapPayloads.balance(rpcPass: _requireRpcPass(), coin: supportedRel),
    );
    final result = Map<String, dynamic>.from(
      response['result'] as Map? ?? const {},
    );
    return _decimal(
      result['balance'] ?? result['spendable'] ?? result['available'],
    );
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

  Future<Map<String, dynamic>> withdrawLtc({
    required String address,
    required Decimal amount,
  }) {
    return _rpc(
      KdfSwapPayloads.withdraw(
        rpcPass: _requireRpcPass(),
        coin: supportedRel,
        to: address,
        amount: amount,
      ),
    );
  }

  Future<void> dispose() async {
    final sdk = _sdk;
    _sdk = null;
    _walletId = null;
    _rpcPassword = null;
    if (sdk == null) return;
    try {
      await sdk.auth.signOut();
    } catch (_) {
      // The SDK may already be signed out while shutting down.
    }
    await sdk.dispose();
  }

  Future<void> _start(String walletId) async {
    await dispose();

    var seed = await FfiBridge.exportSeedForKdf(walletId);
    final walletPassword = await _getOrCreateKdfWalletPassword(walletId);
    final rpcPassword = _randomSecret(32);
    final walletName = _kdfWalletName(walletId);
    final sdk = KomodoDefiSdk(
      host: LocalConfig(https: false, rpcPassword: rpcPassword),
      config: const KomodoDefiSdkConfig(
        defaultAssets: {supportedBase, supportedRel},
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
      _sdk = sdk;
      _walletId = walletId;
      _rpcPassword = rpcPassword;
    } catch (error) {
      await sdk.dispose();
      throw KdfSwapEngineException('Failed to start KDF swap engine', error);
    } finally {
      seed = '';
    }
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
    _assertNoGleecEndpoint(payload);
    final response = await _requireSdk().client.executeRpc(payload);
    return Map<String, dynamic>.from(response);
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

  SwapOrderbookLevel? _orderbookLevel(Object? value) {
    if (value is! Map) return null;
    final json = Map<String, dynamic>.from(value);
    final price = _decimal(json['price']);
    final volume = _decimal(
      json['base_max_volume'] ??
          json['base_max_volume_aggr'] ??
          json['max_volume'] ??
          json['volume'],
    );
    if (price.compareTo(Decimal.zero) <= 0 ||
        volume.compareTo(Decimal.zero) <= 0) {
      return null;
    }
    return SwapOrderbookLevel(
      priceLtcPerArrr: price,
      arrrAmount: volume,
      orderId: json['uuid'] as String?,
      raw: json,
    );
  }

  Decimal _decimal(Object? value) {
    if (value == null) return Decimal.zero;
    if (value is Decimal) return value;
    if (value is num || value is String) return Decimal.parse(value.toString());
    if (value is Map) {
      final decimal = value['decimal'];
      if (decimal != null) return Decimal.parse(decimal.toString());
    }
    return Decimal.zero;
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

  String _kdfWalletName(String walletId) {
    final digest = sha256.convert(utf8.encode(walletId)).toString();
    return 'pirate_swap_${digest.substring(0, 24)}';
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
