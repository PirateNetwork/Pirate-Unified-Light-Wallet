import 'package:decimal/decimal.dart';

class KdfSwapPayloads {
  const KdfSwapPayloads._();

  static Map<String, dynamic> orderbook({
    required String rpcPass,
    required String base,
    required String rel,
  }) => {
    'method': 'orderbook',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {'base': base, 'rel': rel},
  };

  static Map<String, dynamic> tradePreimage({
    required String rpcPass,
    required String base,
    required String rel,
    required String swapMethod,
    String? volume,
    String? price,
    bool? max,
  }) => {
    'method': 'trade_preimage',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {
      'base': base,
      'rel': rel,
      'swap_method': swapMethod,
      'volume': ?volume,
      'price': ?price,
      'max': ?max,
    },
  };

  static Map<String, dynamic> startSwap({
    required String rpcPass,
    required String base,
    required String rel,
    required String baseAmount,
    required String relAmount,
    required String method,
  }) => {
    'method': 'start_swap',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {
      'base': base,
      'rel': rel,
      'base_coin_amount': baseAmount,
      'rel_coin_amount': relAmount,
      'method': method,
    },
  };

  static Map<String, dynamic> setOrder({
    required String rpcPass,
    required String base,
    required String rel,
    required String price,
    required String volume,
    String? minVolume,
  }) => {
    'method': 'setprice',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {
      'base': base,
      'rel': rel,
      'price': price,
      'volume': volume,
      'min_volume': ?minVolume,
    },
  };

  static Map<String, dynamic> cancelOrder({
    required String rpcPass,
    required String uuid,
  }) => {
    'method': 'cancel_order',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {'uuid': uuid},
  };

  static Map<String, dynamic> myOrders({required String rpcPass}) => {
    'method': 'my_orders',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
  };

  static Map<String, dynamic> swapStatus({
    required String rpcPass,
    required String uuid,
  }) => {
    'method': 'my_swap_status',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {'uuid': uuid},
  };

  static Map<String, dynamic> balance({
    required String rpcPass,
    required String coin,
  }) => {
    'method': 'my_balance',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {'coin': coin},
  };

  static Map<String, dynamic> withdraw({
    required String rpcPass,
    required String coin,
    required String to,
    required Decimal amount,
    bool max = false,
  }) => {
    'method': 'withdraw',
    'mmrpc': '2.0',
    'rpc_pass': rpcPass,
    'params': {
      'coin': coin,
      'to': to,
      if (max) 'max': true else 'amount': amount.toString(),
    },
  };
}
