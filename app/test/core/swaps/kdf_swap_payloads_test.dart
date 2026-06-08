import 'dart:convert';

import 'package:decimal/decimal.dart';
import 'package:pirate_wallet/core/swaps/kdf_swap_payloads.dart';
import 'package:test/test.dart';

void main() {
  void expectNoGleec(Map<String, dynamic> payload) {
    expect(jsonEncode(payload).toLowerCase(), isNot(contains('gleec')));
  }

  test('ARRR/LTC orderbook payload uses local KDF RPC shape', () {
    final payload = KdfSwapPayloads.orderbook(
      rpcPass: 'secret',
      base: 'ARRR',
      rel: 'LTC',
    );

    expect(payload['method'], 'orderbook');
    expect(payload['mmrpc'], '2.0');
    expect(payload['userpass'], 'secret');
    expect(payload, isNot(contains('rpc_pass')));
    expect(payload['params'], {'base': 'ARRR', 'rel': 'LTC'});
    expectNoGleec(payload);
  });

  test('start swap payload contains requested market amounts', () {
    final payload = KdfSwapPayloads.startSwap(
      rpcPass: 'secret',
      base: 'ARRR',
      rel: 'LTC',
      baseAmount: '10',
      relAmount: '0.1',
      method: 'buy',
    );

    expect(payload['method'], 'start_swap');
    final params = Map<String, dynamic>.from(payload['params'] as Map);
    expect(params['base_coin_amount'], '10');
    expect(params['rel_coin_amount'], '0.1');
    expect(params['method'], 'buy');
    expectNoGleec(payload);
  });

  test('trade preimage payload includes KDF-required price', () {
    final payload = KdfSwapPayloads.tradePreimage(
      rpcPass: 'secret',
      base: 'ARRR',
      rel: 'LTC',
      swapMethod: 'buy',
      volume: '10',
      price: '0.00575',
    );

    expect(payload['method'], 'trade_preimage');
    expect(payload['userpass'], 'secret');
    final params = Map<String, dynamic>.from(payload['params'] as Map);
    expect(params['base'], 'ARRR');
    expect(params['rel'], 'LTC');
    expect(params['swap_method'], 'buy');
    expect(params['volume'], '10');
    expect(params['price'], '0.00575');
    expectNoGleec(payload);
  });

  test('limit order payload can represent maker order placement', () {
    final payload = KdfSwapPayloads.setOrder(
      rpcPass: 'secret',
      base: 'LTC',
      rel: 'ARRR',
      price: '97.08737864',
      volume: '1.5',
    );

    expect(payload['method'], 'setprice');
    final params = Map<String, dynamic>.from(payload['params'] as Map);
    expect(params['base'], 'LTC');
    expect(params['rel'], 'ARRR');
    expect(params['price'], '97.08737864');
    expect(params['volume'], '1.5');
    expectNoGleec(payload);
  });

  test('cancel order and withdrawal payloads are explicit', () {
    final cancel = KdfSwapPayloads.cancelOrder(
      rpcPass: 'secret',
      uuid: 'order-uuid',
    );
    final withdraw = KdfSwapPayloads.withdraw(
      rpcPass: 'secret',
      coin: 'ARRR',
      to: 'zs-address',
      amount: Decimal.parse('2.5'),
    );

    expect(cancel['method'], 'cancel_order');
    final cancelParams = Map<String, dynamic>.from(cancel['params'] as Map);
    final withdrawParams = Map<String, dynamic>.from(withdraw['params'] as Map);
    expect(cancelParams['uuid'], 'order-uuid');
    expect(withdraw['method'], 'withdraw');
    expect(withdrawParams['coin'], 'ARRR');
    expect(withdrawParams['to'], 'zs-address');
    expect(withdrawParams['amount'], '2.5');
    expectNoGleec(cancel);
    expectNoGleec(withdraw);
  });

  test('withdraw payload supports max funding refunds', () {
    final payload = KdfSwapPayloads.withdraw(
      rpcPass: 'secret',
      coin: 'LTC',
      to: 'ltc-address',
      max: true,
    );

    final params = Map<String, dynamic>.from(payload['params'] as Map);
    expect(params['coin'], 'LTC');
    expect(params['to'], 'ltc-address');
    expect(params['max'], isTrue);
    expect(params, isNot(contains('amount')));
    expectNoGleec(payload);
  });
}
