import 'package:decimal/decimal.dart';

const swapDepositWindow = Duration(minutes: 45);
const appTakerFeeRecipientArrrAddress =
    'zs14sk0j58tl0pp3geamszpyc376em6uuvrezhsr2zuxmnefryx6nptjwl7rydtr9rdn73e6da36gv';
final appTakerFeeRate = Decimal.parse('0.0087');
final totalTakerFeeRate = Decimal.parse('0.01');

Decimal _scaleSwapDecimal(Decimal value, {int scale = 8}) {
  return Decimal.parse(value.toStringAsFixed(scale));
}

Decimal _divideSwapDecimal(Decimal value, Decimal divisor, {int scale = 8}) {
  return (value / divisor).toDecimal(scaleOnInfinitePrecision: scale);
}

enum SwapAsset {
  arrr('ARRR'),
  ltc('LTC'),
  varrr('vARRR');

  const SwapAsset(this.ticker);

  final String ticker;

  static SwapAsset parse(String value) {
    final normalized = value.toUpperCase();
    return SwapAsset.values.firstWhere(
      (asset) =>
          asset.ticker.toUpperCase() == normalized ||
          asset.name.toUpperCase() == normalized,
    );
  }
}

enum SwapPair {
  arrrLtc(SwapAsset.arrr, SwapAsset.ltc),
  arrrVarrr(SwapAsset.arrr, SwapAsset.varrr);

  const SwapPair(this.baseAsset, this.relAsset);

  final SwapAsset baseAsset;
  final SwapAsset relAsset;

  String get displayName => '${baseAsset.ticker}/${relAsset.ticker}';
  String get baseTicker => baseAsset.ticker;
  String get relTicker => relAsset.ticker;

  static SwapPair fromAssets({
    required SwapAsset base,
    required SwapAsset rel,
  }) {
    return SwapPair.values.firstWhere(
      (pair) => pair.baseAsset == base && pair.relAsset == rel,
      orElse: () => throw ArgumentError('Unsupported swap pair $base/$rel'),
    );
  }

  static SwapPair parse(Object? value) {
    if (value == null) return SwapPair.arrrLtc;
    final text = value.toString();
    for (final pair in SwapPair.values) {
      if (pair.name == text ||
          pair.displayName.toUpperCase() == text.toUpperCase()) {
        return pair;
      }
    }
    throw ArgumentError('Unsupported swap pair $text');
  }
}

enum SwapSide { buyArrr, sellArrr }

enum SwapIntentStatus {
  prepared,
  waitingForDeposit,
  marketSwapStarted,
  limitOrderPlaced,
  completed,
  cancelled,
  failed,
}

enum SwapProgressStage {
  startingKdf,
  activatingCoins,
  waitingForDeposit,
  matchingMarketOrder,
  placingLimitRemainder,
  withdrawing,
  complete,
  failed,
}

enum SwapOrderKind { market, limit }

enum SwapOrderStatus { active, filled, cancelled, failed, unknown }

Decimal decimalFromJson(Object? value) {
  if (value == null) return Decimal.zero;
  return Decimal.parse(value.toString());
}

String decimalToJson(Decimal value) => value.toString();

class SwapFeeBreakdown {
  SwapFeeBreakdown({
    Decimal? kdfFee,
    Decimal? baseNetworkFee,
    Decimal? relNetworkFee,
    Decimal? withdrawalFee,
    this.raw = const <String, dynamic>{},
  }) : kdfFee = kdfFee ?? Decimal.zero,
       baseNetworkFee = baseNetworkFee ?? Decimal.zero,
       relNetworkFee = relNetworkFee ?? Decimal.zero,
       withdrawalFee = withdrawalFee ?? Decimal.zero;

  factory SwapFeeBreakdown.fromJson(Map<String, dynamic> json) {
    return SwapFeeBreakdown(
      kdfFee: decimalFromJson(json['kdfFee']),
      baseNetworkFee: decimalFromJson(json['baseNetworkFee']),
      relNetworkFee: decimalFromJson(json['relNetworkFee']),
      withdrawalFee: decimalFromJson(json['withdrawalFee']),
      raw: Map<String, dynamic>.from(json['raw'] as Map? ?? const {}),
    );
  }

  final Decimal kdfFee;
  final Decimal baseNetworkFee;
  final Decimal relNetworkFee;
  final Decimal withdrawalFee;
  final Map<String, dynamic> raw;

  Decimal get total => kdfFee + baseNetworkFee + relNetworkFee + withdrawalFee;
  Decimal get networkTotal => baseNetworkFee + relNetworkFee + withdrawalFee;

  Map<String, dynamic> toJson() => {
    'kdfFee': decimalToJson(kdfFee),
    'baseNetworkFee': decimalToJson(baseNetworkFee),
    'relNetworkFee': decimalToJson(relNetworkFee),
    'withdrawalFee': decimalToJson(withdrawalFee),
    'raw': raw,
  };
}

class SwapOrderbookLevel {
  const SwapOrderbookLevel({
    required this.priceLtcPerArrr,
    required this.arrrAmount,
    this.orderId,
    this.raw = const <String, dynamic>{},
  });

  factory SwapOrderbookLevel.fromJson(Map<String, dynamic> json) {
    return SwapOrderbookLevel(
      priceLtcPerArrr: decimalFromJson(json['priceLtcPerArrr']),
      arrrAmount: decimalFromJson(json['arrrAmount']),
      orderId: json['orderId'] as String?,
      raw: Map<String, dynamic>.from(json['raw'] as Map? ?? const {}),
    );
  }

  final Decimal priceLtcPerArrr;
  final Decimal arrrAmount;
  final String? orderId;
  final Map<String, dynamic> raw;

  Decimal get ltcAmount => priceLtcPerArrr * arrrAmount;
  Decimal get relAmount => ltcAmount;
  Decimal get priceRelPerArrr => priceLtcPerArrr;

  Map<String, dynamic> toJson() => {
    'priceLtcPerArrr': decimalToJson(priceLtcPerArrr),
    'arrrAmount': decimalToJson(arrrAmount),
    if (orderId != null) 'orderId': orderId,
    'raw': raw,
  };
}

class SwapMarketFill {
  const SwapMarketFill({
    required this.priceLtcPerArrr,
    required this.arrrAmount,
    required this.ltcAmount,
    this.orderId,
  });

  factory SwapMarketFill.fromJson(Map<String, dynamic> json) {
    return SwapMarketFill(
      priceLtcPerArrr: decimalFromJson(json['priceLtcPerArrr']),
      arrrAmount: decimalFromJson(json['arrrAmount']),
      ltcAmount: decimalFromJson(json['ltcAmount']),
      orderId: json['orderId'] as String?,
    );
  }

  final Decimal priceLtcPerArrr;
  final Decimal arrrAmount;
  final Decimal ltcAmount;
  final String? orderId;

  Decimal get priceRelPerArrr => priceLtcPerArrr;
  Decimal get relAmount => ltcAmount;

  Map<String, dynamic> toJson() => {
    'priceLtcPerArrr': decimalToJson(priceLtcPerArrr),
    'arrrAmount': decimalToJson(arrrAmount),
    'ltcAmount': decimalToJson(ltcAmount),
    if (orderId != null) 'orderId': orderId,
  };
}

class SwapPlan {
  const SwapPlan({
    required this.side,
    required this.referencePriceLtcPerArrr,
    required this.marketArrrAmount,
    required this.marketLtcAmount,
    required this.remainderLtcAmount,
    required this.remainderArrrAmount,
    required this.slippageCap,
    required this.realizedSlippage,
    required this.fills,
    required this.appFeeArrrAmount,
    this.limitPriceLtcPerArrr,
  });

  factory SwapPlan.fromJson(Map<String, dynamic> json) {
    return SwapPlan(
      side: SwapSide.values.byName(json['side'] as String),
      referencePriceLtcPerArrr: decimalFromJson(
        json['referencePriceLtcPerArrr'],
      ),
      marketArrrAmount: decimalFromJson(json['marketArrrAmount']),
      marketLtcAmount: decimalFromJson(json['marketLtcAmount']),
      remainderLtcAmount: decimalFromJson(json['remainderLtcAmount']),
      remainderArrrAmount: decimalFromJson(json['remainderArrrAmount']),
      limitPriceLtcPerArrr: json['limitPriceLtcPerArrr'] == null
          ? null
          : decimalFromJson(json['limitPriceLtcPerArrr']),
      slippageCap: decimalFromJson(json['slippageCap']),
      realizedSlippage: decimalFromJson(json['realizedSlippage']),
      appFeeArrrAmount: decimalFromJson(json['appFeeArrrAmount']),
      fills: (json['fills'] as List? ?? const [])
          .map(
            (value) => SwapMarketFill.fromJson(
              Map<String, dynamic>.from(value as Map),
            ),
          )
          .toList(),
    );
  }

  final SwapSide side;
  final Decimal referencePriceLtcPerArrr;
  final Decimal marketArrrAmount;
  final Decimal marketLtcAmount;
  final Decimal remainderLtcAmount;
  final Decimal remainderArrrAmount;
  final Decimal? limitPriceLtcPerArrr;
  final Decimal slippageCap;
  final Decimal realizedSlippage;
  final List<SwapMarketFill> fills;
  final Decimal appFeeArrrAmount;

  Decimal get referencePriceRelPerArrr => referencePriceLtcPerArrr;
  Decimal get marketRelAmount => marketLtcAmount;
  Decimal get remainderRelAmount => remainderLtcAmount;
  Decimal? get limitPriceRelPerArrr => limitPriceLtcPerArrr;

  Decimal get _feeMultiplier => Decimal.one - totalTakerFeeRate;

  Decimal get _sellMarketGrossArrrAmount {
    if (marketArrrAmount <= Decimal.zero || _feeMultiplier <= Decimal.zero) {
      return Decimal.zero;
    }
    return _divideSwapDecimal(marketArrrAmount, _feeMultiplier, scale: 12);
  }

  Decimal get takerFeeArrrBasis => switch (side) {
    SwapSide.buyArrr => marketArrrAmount,
    SwapSide.sellArrr => _sellMarketGrossArrrAmount,
  };

  Decimal get totalTakerFeeArrrAmount {
    if (takerFeeArrrBasis <= Decimal.zero) return Decimal.zero;
    return _scaleSwapDecimal(takerFeeArrrBasis * totalTakerFeeRate);
  }

  Decimal get estimatedKdfTakerFeeArrrAmount {
    final amount = totalTakerFeeArrrAmount - appFeeArrrAmount;
    return amount > Decimal.zero ? _scaleSwapDecimal(amount) : Decimal.zero;
  }

  Decimal get marketArrrAmountAfterTakerFees {
    if (side == SwapSide.sellArrr) return marketArrrAmount;
    final net = marketArrrAmount - totalTakerFeeArrrAmount;
    return net > Decimal.zero ? net : Decimal.zero;
  }

  Decimal get marketArrrAmountAfterAppFee => marketArrrAmountAfterTakerFees;

  Decimal get marketArrrAmountForEffectivePrice => switch (side) {
    SwapSide.buyArrr => marketArrrAmountAfterTakerFees,
    SwapSide.sellArrr => _sellMarketGrossArrrAmount,
  };

  Decimal? get effectiveMarketPriceRelPerArrr {
    final arrrAmount = marketArrrAmountForEffectivePrice;
    if (marketRelAmount <= Decimal.zero || arrrAmount <= Decimal.zero) {
      return null;
    }
    return _divideSwapDecimal(marketRelAmount, arrrAmount, scale: 12);
  }

  Decimal get requestedPayAmount => switch (side) {
    SwapSide.buyArrr => marketLtcAmount + remainderLtcAmount,
    SwapSide.sellArrr => _scaleSwapDecimal(
      _sellMarketGrossArrrAmount + remainderArrrAmount,
    ),
  };

  Decimal get expectedReceiveAmount => switch (side) {
    SwapSide.buyArrr => marketArrrAmountAfterTakerFees + remainderArrrAmount,
    SwapSide.sellArrr => marketLtcAmount + remainderLtcAmount,
  };

  bool get hasAppFee => appFeeArrrAmount > Decimal.zero;
  bool get hasMarketFill => marketArrrAmount > Decimal.zero;
  bool get hasLimitRemainder =>
      remainderLtcAmount > Decimal.zero || remainderArrrAmount > Decimal.zero;

  Map<String, dynamic> toJson() => {
    'side': side.name,
    'referencePriceLtcPerArrr': decimalToJson(referencePriceLtcPerArrr),
    'marketArrrAmount': decimalToJson(marketArrrAmount),
    'marketLtcAmount': decimalToJson(marketLtcAmount),
    'remainderLtcAmount': decimalToJson(remainderLtcAmount),
    'remainderArrrAmount': decimalToJson(remainderArrrAmount),
    if (limitPriceLtcPerArrr != null)
      'limitPriceLtcPerArrr': decimalToJson(limitPriceLtcPerArrr!),
    'slippageCap': decimalToJson(slippageCap),
    'realizedSlippage': decimalToJson(realizedSlippage),
    'appFeeArrrAmount': decimalToJson(appFeeArrrAmount),
    'fills': fills.map((fill) => fill.toJson()).toList(),
  };
}

class SwapQuote {
  const SwapQuote({
    required this.walletId,
    required this.side,
    required this.baseAsset,
    required this.relAsset,
    required this.requestedLtcAmount,
    required this.plan,
    required this.fees,
    required this.createdAt,
    this.pair = SwapPair.arrrLtc,
    this.preimage,
  });

  final String walletId;
  final SwapSide side;
  final SwapPair pair;
  final SwapAsset baseAsset;
  final SwapAsset relAsset;
  final Decimal requestedLtcAmount;
  final SwapPlan plan;
  final SwapFeeBreakdown fees;
  final DateTime createdAt;
  final Map<String, dynamic>? preimage;

  Map<String, dynamic> toJson() => {
    'walletId': walletId,
    'side': side.name,
    'pair': pair.name,
    'baseAsset': baseAsset.ticker,
    'relAsset': relAsset.ticker,
    'requestedLtcAmount': decimalToJson(requestedLtcAmount),
    'plan': plan.toJson(),
    'fees': fees.toJson(),
    'createdAt': createdAt.toIso8601String(),
    if (preimage != null) 'preimage': preimage,
  };
}

class SwapIntent {
  const SwapIntent({
    required this.id,
    required this.walletId,
    required this.side,
    required this.status,
    required this.createdAt,
    required this.updatedAt,
    required this.plan,
    this.pair = SwapPair.arrrLtc,
    this.ltcDepositAddress,
    this.destinationLtcAddress,
    this.arrReceivingAddress,
    this.marketSwapUuid,
    this.limitOrderUuid,
    this.lastError,
  });

  factory SwapIntent.fromJson(Map<String, dynamic> json) {
    return SwapIntent(
      id: json['id'] as String,
      walletId: json['walletId'] as String,
      side: SwapSide.values.byName(json['side'] as String),
      status: SwapIntentStatus.values.byName(json['status'] as String),
      createdAt: DateTime.parse(json['createdAt'] as String),
      updatedAt: DateTime.parse(json['updatedAt'] as String),
      plan: SwapPlan.fromJson(Map<String, dynamic>.from(json['plan'] as Map)),
      pair: SwapPair.parse(json['pair']),
      ltcDepositAddress: json['ltcDepositAddress'] as String?,
      destinationLtcAddress: json['destinationLtcAddress'] as String?,
      arrReceivingAddress: json['arrReceivingAddress'] as String?,
      marketSwapUuid: json['marketSwapUuid'] as String?,
      limitOrderUuid: json['limitOrderUuid'] as String?,
      lastError: json['lastError'] as String?,
    );
  }

  final String id;
  final String walletId;
  final SwapSide side;
  final SwapIntentStatus status;
  final DateTime createdAt;
  final DateTime updatedAt;
  final SwapPlan plan;
  final SwapPair pair;
  final String? ltcDepositAddress;
  final String? destinationLtcAddress;
  final String? arrReceivingAddress;
  final String? marketSwapUuid;
  final String? limitOrderUuid;
  final String? lastError;

  DateTime? get depositExpiresAt {
    if (status != SwapIntentStatus.waitingForDeposit) return null;
    return createdAt.toUtc().add(swapDepositWindow);
  }

  bool isDepositExpired(DateTime now) {
    final expiresAt = depositExpiresAt;
    if (expiresAt == null) return false;
    return !now.toUtc().isBefore(expiresAt);
  }

  Decimal get requestedPayAmount => plan.requestedPayAmount;

  String? get relDepositAddress => ltcDepositAddress;
  String? get destinationRelAddress => destinationLtcAddress;

  Decimal get expectedReceiveAmount => plan.expectedReceiveAmount;

  SwapIntent copyWith({
    SwapIntentStatus? status,
    DateTime? updatedAt,
    SwapPair? pair,
    String? ltcDepositAddress,
    String? destinationLtcAddress,
    String? arrReceivingAddress,
    String? marketSwapUuid,
    String? limitOrderUuid,
    String? lastError,
  }) {
    return SwapIntent(
      id: id,
      walletId: walletId,
      side: side,
      status: status ?? this.status,
      createdAt: createdAt,
      updatedAt: updatedAt ?? DateTime.now().toUtc(),
      plan: plan,
      pair: pair ?? this.pair,
      ltcDepositAddress: ltcDepositAddress ?? this.ltcDepositAddress,
      destinationLtcAddress:
          destinationLtcAddress ?? this.destinationLtcAddress,
      arrReceivingAddress: arrReceivingAddress ?? this.arrReceivingAddress,
      marketSwapUuid: marketSwapUuid ?? this.marketSwapUuid,
      limitOrderUuid: limitOrderUuid ?? this.limitOrderUuid,
      lastError: lastError,
    );
  }

  Map<String, dynamic> toJson() => {
    'id': id,
    'walletId': walletId,
    'side': side.name,
    'pair': pair.name,
    'status': status.name,
    'createdAt': createdAt.toIso8601String(),
    'updatedAt': updatedAt.toIso8601String(),
    'plan': plan.toJson(),
    if (ltcDepositAddress != null) 'ltcDepositAddress': ltcDepositAddress,
    if (destinationLtcAddress != null)
      'destinationLtcAddress': destinationLtcAddress,
    if (arrReceivingAddress != null) 'arrReceivingAddress': arrReceivingAddress,
    if (marketSwapUuid != null) 'marketSwapUuid': marketSwapUuid,
    if (limitOrderUuid != null) 'limitOrderUuid': limitOrderUuid,
    if (lastError != null) 'lastError': lastError,
  };
}

class SwapOrder {
  const SwapOrder({
    required this.uuid,
    required this.kind,
    required this.status,
    required this.baseAsset,
    required this.relAsset,
    required this.price,
    required this.volume,
    this.raw = const <String, dynamic>{},
  });

  final String uuid;
  final SwapOrderKind kind;
  final SwapOrderStatus status;
  final SwapAsset baseAsset;
  final SwapAsset relAsset;
  final Decimal price;
  final Decimal volume;
  final Map<String, dynamic> raw;
}

class SwapProgress {
  const SwapProgress({
    required this.intentId,
    required this.stage,
    required this.message,
    this.raw = const <String, dynamic>{},
  });

  final String intentId;
  final SwapProgressStage stage;
  final String message;
  final Map<String, dynamic> raw;
}
