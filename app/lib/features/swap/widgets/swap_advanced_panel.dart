import 'package:decimal/decimal.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/ltc_address.dart';
import '../../../core/swaps/swap_amount_utils.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/molecules/p_card.dart';
import '../swap_viewmodel.dart';

/// Order book view mode (matches MEXC's three-icon toggle).
enum OrderBookViewMode { both, bidsOnly, asksOnly }

typedef OrderbookDepthSelectionCallback =
    void Function({
      required SwapSide side,
      required Decimal priceRelPerArrr,
      required Decimal payAmount,
    });

String? _usdPriceLabel({
  required Decimal relPrice,
  required double? relUsdPrice,
  String? suffix,
}) {
  if (relUsdPrice == null || relUsdPrice <= 0) return null;
  if (relPrice <= Decimal.zero) return null;

  final relAmount = double.tryParse(relPrice.toString());
  if (relAmount == null || relAmount <= 0) return null;

  final value = relAmount * relUsdPrice;
  final label = '≈ \$${_formatUsd(value)}';
  return suffix == null ? label : '$label $suffix';
}

String _formatUsd(double value) {
  final fractionDigits = value >= 1
      ? 2
      : value >= 0.01
      ? 4
      : 6;
  return value.toStringAsFixed(fractionDigits);
}

class SwapAdvancedPanel extends StatelessWidget {
  const SwapAdvancedPanel({
    required this.state,
    required this.walletBalance,
    required this.onPayAmountChanged,
    required this.onLtcAddressChanged,
    required this.onSlippageChanged,
    required this.onLimitPriceChanged,
    required this.onOrderTypeChanged,
    required this.onSideChanged,
    required this.onOrderbookDepthSelected,
    this.relUsdPrice,
    this.averageArrrUsdPriceLabel,
    this.onMax,
    super.key,
  });

  final SwapViewModelState state;
  final String? walletBalance;
  final ValueChanged<String> onPayAmountChanged;
  final ValueChanged<String> onLtcAddressChanged;
  final ValueChanged<double> onSlippageChanged;
  final ValueChanged<String> onLimitPriceChanged;
  final ValueChanged<SwapAdvancedOrderType> onOrderTypeChanged;
  final ValueChanged<SwapSide> onSideChanged;
  final OrderbookDepthSelectionCallback onOrderbookDepthSelected;
  final double? relUsdPrice;
  final String? averageArrrUsdPriceLabel;
  final VoidCallback? onMax;

  @override
  Widget build(BuildContext context) {
    final isWide = MediaQuery.of(context).size.width >= 900;
    final orderBook = SwapOrderBookPanel(
      pair: state.pair,
      asks: state.orderbookAsks,
      bids: state.orderbookBids,
      isLoading: state.isLoadingOrderbook,
      relUsdPrice: relUsdPrice,
      onDepthSelected: onOrderbookDepthSelected,
    );
    final form = _TradeForm(
      state: state,
      walletBalance: walletBalance,
      relUsdPrice: relUsdPrice,
      averageArrrUsdPriceLabel: averageArrrUsdPriceLabel,
      onPayAmountChanged: onPayAmountChanged,
      onLtcAddressChanged: onLtcAddressChanged,
      onSlippageChanged: onSlippageChanged,
      onLimitPriceChanged: onLimitPriceChanged,
      onOrderTypeChanged: onOrderTypeChanged,
      onSideChanged: onSideChanged,
      onMax: onMax,
    );

    if (isWide) {
      return Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(child: orderBook),
          const SizedBox(width: PSpacing.lg),
          SizedBox(width: 380, child: form),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        form,
        const SizedBox(height: PSpacing.lg),
        orderBook,
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// MEXC-style Order Book
// ---------------------------------------------------------------------------

class SwapOrderBookPanel extends StatefulWidget {
  const SwapOrderBookPanel({
    required this.pair,
    required this.asks,
    required this.bids,
    required this.isLoading,
    required this.onDepthSelected,
    this.relUsdPrice,
    super.key,
  });

  final SwapPair pair;
  final List<SwapOrderbookLevel> asks;
  final List<SwapOrderbookLevel> bids;
  final bool isLoading;
  final OrderbookDepthSelectionCallback onDepthSelected;
  final double? relUsdPrice;

  @override
  State<SwapOrderBookPanel> createState() => _SwapOrderBookPanelState();
}

class _SwapOrderBookPanelState extends State<SwapOrderBookPanel> {
  // Price-tick options for orderbook aggregation, MEXC-style.
  // ARRR/LTC trades around 1e-5 so we offer a range of tick sizes.
  late final List<Decimal> _groupings = [
    Decimal.parse('0.0000001'),
    Decimal.parse('0.000001'),
    Decimal.parse('0.00001'),
    Decimal.parse('0.0001'),
    Decimal.parse('0.001'),
  ];
  late Decimal _selectedGrouping =
      _groupings[1]; // 0.000001 default for ARRR/LTC
  OrderBookViewMode _viewMode = OrderBookViewMode.both;

  /// Aggregates orderbook levels by rounding price to the selected tick.
  /// Bids round down (floor) so groups represent "at-or-above" depth.
  /// Asks round up (ceil) so groups represent "at-or-below" depth.
  List<_AggregatedLevel> _aggregate(
    List<SwapOrderbookLevel> raw,
    Decimal tick, {
    required bool isBid,
  }) {
    if (raw.isEmpty || tick <= Decimal.zero) return const [];
    final Map<String, _AggregatedLevel> map = {};

    for (final level in raw) {
      final price = level.priceLtcPerArrr;
      if (price <= Decimal.zero) continue;

      // Compute steps using BigInt arithmetic for precision.
      final scaledPrice = (price / tick).toDouble();
      var roundedSteps = isBid ? scaledPrice.floor() : scaledPrice.ceil();
      // If price is below the smallest tick, snap to one tick to keep visible.
      if (roundedSteps <= 0) roundedSteps = isBid ? 0 : 1;
      if (roundedSteps <= 0) continue;

      final groupedPrice = tick * Decimal.fromInt(roundedSteps);
      final key = groupedPrice.toString();

      final existing = map[key];
      if (existing != null) {
        existing.amount = existing.amount + level.arrrAmount;
      } else {
        map[key] = _AggregatedLevel(
          price: groupedPrice,
          amount: level.arrrAmount,
        );
      }
    }

    final list = map.values.toList();
    if (isBid) {
      list.sort((a, b) => b.price.compareTo(a.price));
    } else {
      list.sort((a, b) => a.price.compareTo(b.price));
    }

    // Compute cumulative totals for depth bars.
    Decimal cumulativeArrr = Decimal.zero;
    Decimal cumulativeLtc = Decimal.zero;
    for (final l in list) {
      cumulativeArrr += l.amount;
      cumulativeLtc += l.amount * l.price;
      l
        ..cumulativeAmount = cumulativeArrr
        ..cumulativeTotal = cumulativeLtc;
    }
    return list;
  }

  int _decimalsForTick(Decimal tick) {
    // Express tick in plain decimal then count fractional digits.
    final str = tick.toDouble().toStringAsFixed(12);
    if (!str.contains('.')) return 0;
    final fractional = str.split('.')[1].replaceAll(RegExp(r'0+$'), '');
    return fractional.length;
  }

  @override
  Widget build(BuildContext context) {
    final aggregatedAsks = _aggregate(
      widget.asks,
      _selectedGrouping,
      isBid: false,
    );
    final aggregatedBids = _aggregate(
      widget.bids,
      _selectedGrouping,
      isBid: true,
    );

    final maxCumulative = _maxCumulative(aggregatedAsks, aggregatedBids);
    final priceDecimals = _decimalsForTick(_selectedGrouping);
    final midPrice = _midPrice(widget.asks, widget.bids);

    return PCard(
      padding: const EdgeInsets.all(PSpacing.md),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Header: title + view toggles + decimal selector
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Order Book'.tr,
                style: PTypography.titleMedium(color: AppColors.textPrimary),
              ),
              Row(
                children: [
                  _ViewModeToggle(
                    mode: _viewMode,
                    onChanged: (m) => setState(() => _viewMode = m),
                  ),
                ],
              ),
            ],
          ),
          const SizedBox(height: PSpacing.sm),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                _viewMode == OrderBookViewMode.bidsOnly
                    ? 'Bids only'.tr
                    : _viewMode == OrderBookViewMode.asksOnly
                    ? 'Asks only'.tr
                    : 'All orders'.tr,
                style: PTypography.labelSmall(color: AppColors.textTertiary),
              ),
              _GroupingSelector(
                value: _selectedGrouping,
                options: _groupings,
                onChanged: (v) => setState(() => _selectedGrouping = v),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.sm),
          // Column headers
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: PSpacing.xs),
            child: Row(
              children: [
                Expanded(
                  flex: 3,
                  child: Text(
                    'Price ({ticker})'.trArgs({
                      'ticker': widget.pair.relTicker,
                    }),
                    style: PTypography.labelSmall(
                      color: AppColors.textTertiary,
                    ),
                  ),
                ),
                Expanded(
                  flex: 3,
                  child: Text(
                    'Amount (ARRR)'.tr,
                    style: PTypography.labelSmall(
                      color: AppColors.textTertiary,
                    ),
                    textAlign: TextAlign.right,
                  ),
                ),
                Expanded(
                  flex: 3,
                  child: Text(
                    'Total ({ticker})'.trArgs({
                      'ticker': widget.pair.relTicker,
                    }),
                    style: PTypography.labelSmall(
                      color: AppColors.textTertiary,
                    ),
                    textAlign: TextAlign.right,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: PSpacing.xs),

          if (widget.isLoading)
            const Padding(
              padding: EdgeInsets.all(PSpacing.xl),
              child: Center(child: CircularProgressIndicator()),
            )
          else
            _buildBody(
              aggregatedAsks: aggregatedAsks,
              aggregatedBids: aggregatedBids,
              maxCumulative: maxCumulative,
              priceDecimals: priceDecimals,
              midPrice: midPrice,
            ),
        ],
      ),
    );
  }

  Widget _buildBody({
    required List<_AggregatedLevel> aggregatedAsks,
    required List<_AggregatedLevel> aggregatedBids,
    required Decimal maxCumulative,
    required int priceDecimals,
    required Decimal? midPrice,
  }) {
    // For asks: highest price at top, lowest just above mid (closest to spread).
    // We pass asks in descending order so item[0] = highest, last item = lowest.
    final asksDesc = aggregatedAsks.reversed.toList();
    switch (_viewMode) {
      case OrderBookViewMode.both:
        return Column(
          children: [
            SizedBox(
              height: 200,
              child: _OrderBookList(
                levels: asksDesc.take(20).toList(),
                color: AppColors.error,
                maxCumulative: maxCumulative,
                priceDecimals: priceDecimals,
                referencePrice: midPrice,
                relUsdPrice: widget.relUsdPrice,
                relTicker: widget.pair.relTicker,
                onDepthSelected: widget.onDepthSelected,
                isBid: false,
                anchorBottom: true,
              ),
            ),
            _MidPriceRow(
              price: midPrice,
              asks: widget.asks,
              bids: widget.bids,
              relUsdPrice: widget.relUsdPrice,
            ),
            SizedBox(
              height: 200,
              child: _OrderBookList(
                levels: aggregatedBids.take(20).toList(),
                color: AppColors.success,
                maxCumulative: maxCumulative,
                priceDecimals: priceDecimals,
                referencePrice: midPrice,
                relUsdPrice: widget.relUsdPrice,
                relTicker: widget.pair.relTicker,
                onDepthSelected: widget.onDepthSelected,
                isBid: true,
              ),
            ),
          ],
        );
      case OrderBookViewMode.asksOnly:
        return Column(
          children: [
            SizedBox(
              height: 400,
              child: _OrderBookList(
                levels: asksDesc,
                color: AppColors.error,
                maxCumulative: maxCumulative,
                priceDecimals: priceDecimals,
                referencePrice: midPrice,
                relUsdPrice: widget.relUsdPrice,
                relTicker: widget.pair.relTicker,
                onDepthSelected: widget.onDepthSelected,
                isBid: false,
                anchorBottom: true,
              ),
            ),
            _MidPriceRow(
              price: midPrice,
              asks: widget.asks,
              bids: widget.bids,
              relUsdPrice: widget.relUsdPrice,
            ),
          ],
        );
      case OrderBookViewMode.bidsOnly:
        return Column(
          children: [
            _MidPriceRow(
              price: midPrice,
              asks: widget.asks,
              bids: widget.bids,
              relUsdPrice: widget.relUsdPrice,
            ),
            SizedBox(
              height: 400,
              child: _OrderBookList(
                levels: aggregatedBids,
                color: AppColors.success,
                maxCumulative: maxCumulative,
                priceDecimals: priceDecimals,
                referencePrice: midPrice,
                relUsdPrice: widget.relUsdPrice,
                relTicker: widget.pair.relTicker,
                onDepthSelected: widget.onDepthSelected,
                isBid: true,
              ),
            ),
          ],
        );
    }
  }

  Decimal _maxCumulative(
    List<_AggregatedLevel> asks,
    List<_AggregatedLevel> bids,
  ) {
    Decimal m = Decimal.zero;
    for (final l in asks.take(20)) {
      if (l.cumulativeAmount > m) m = l.cumulativeAmount;
    }
    for (final l in bids.take(20)) {
      if (l.cumulativeAmount > m) m = l.cumulativeAmount;
    }
    return m;
  }

  Decimal? _midPrice(
    List<SwapOrderbookLevel> asks,
    List<SwapOrderbookLevel> bids,
  ) {
    if (asks.isEmpty && bids.isEmpty) return null;
    if (asks.isEmpty) return bids.first.priceLtcPerArrr;
    if (bids.isEmpty) return asks.first.priceLtcPerArrr;
    return ((asks.first.priceLtcPerArrr + bids.first.priceLtcPerArrr) /
            Decimal.fromInt(2))
        .toDecimal(scaleOnInfinitePrecision: 16);
  }
}

/// Internal helper class for aggregated orderbook rows.
class _AggregatedLevel {
  _AggregatedLevel({required this.price, required this.amount});

  final Decimal price;
  Decimal amount;
  Decimal cumulativeAmount = Decimal.zero;
  Decimal cumulativeTotal = Decimal.zero;
}

class _OrderBookList extends StatefulWidget {
  const _OrderBookList({
    required this.levels,
    required this.color,
    required this.maxCumulative,
    required this.priceDecimals,
    required this.isBid,
    required this.relTicker,
    required this.onDepthSelected,
    this.referencePrice,
    this.relUsdPrice,
    this.anchorBottom = false,
  });

  final List<_AggregatedLevel> levels;
  final Color color;
  final Decimal maxCumulative;
  final int priceDecimals;
  final bool isBid;
  final String relTicker;
  final OrderbookDepthSelectionCallback onDepthSelected;
  final Decimal? referencePrice;
  final double? relUsdPrice;

  /// When true, scrolls to bottom on first build (used for asks so lowest
  /// price stays visible right above the mid-price separator).
  final bool anchorBottom;

  @override
  State<_OrderBookList> createState() => _OrderBookListState();
}

class _OrderBookListState extends State<_OrderBookList> {
  final _scrollController = ScrollController();
  int? _hoveredIndex;
  int? _selectedIndex;

  int? get _activeIndex => _hoveredIndex ?? _selectedIndex;

  @override
  void didUpdateWidget(_OrderBookList oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_hoveredIndex != null && _hoveredIndex! >= widget.levels.length) {
      _hoveredIndex = null;
    }
    if (_selectedIndex != null && _selectedIndex! >= widget.levels.length) {
      _selectedIndex = null;
    }
    if (widget.anchorBottom &&
        oldWidget.levels.length != widget.levels.length) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_scrollController.hasClients) {
          _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
        }
      });
    }
  }

  @override
  void initState() {
    super.initState();
    if (widget.anchorBottom) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_scrollController.hasClients) {
          _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
        }
      });
    }
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (widget.levels.isEmpty) {
      return Center(
        child: Text(
          'No orders'.tr,
          style: PTypography.bodySmall(color: AppColors.textTertiary),
        ),
      );
    }

    final activeIndex = _activeIndex;
    final activeLevel =
        activeIndex == null || activeIndex >= widget.levels.length
        ? null
        : widget.levels[activeIndex];

    return MouseRegion(
      onExit: (_) => setState(() => _hoveredIndex = null),
      child: Stack(
        children: [
          ListView.builder(
            controller: _scrollController,
            padding: EdgeInsets.zero,
            itemCount: widget.levels.length,
            itemBuilder: (context, index) {
              final level = widget.levels[index];
              final fraction = widget.maxCumulative > Decimal.zero
                  ? (level.cumulativeAmount / widget.maxCumulative)
                        .toDouble()
                        .clamp(0.0, 1.0)
                  : 0.0;
              final isHighlighted =
                  activeLevel != null && _isInSelectedRange(level, activeLevel);
              return _OrderBookRow(
                price: level.price,
                amount: level.amount,
                total: level.cumulativeTotal,
                color: widget.color,
                depthFraction: fraction,
                priceDecimals: widget.priceDecimals,
                isHighlighted: isHighlighted,
                onHover: () => setState(() => _hoveredIndex = index),
                onTap: () {
                  setState(() => _selectedIndex = index);
                  widget.onDepthSelected(
                    side: widget.isBid ? SwapSide.sellArrr : SwapSide.buyArrr,
                    priceRelPerArrr: level.price,
                    payAmount: widget.isBid
                        ? level.cumulativeAmount
                        : level.cumulativeTotal,
                  );
                },
                usdPriceLabel: _usdPriceLabel(
                  relPrice: level.price,
                  relUsdPrice: widget.relUsdPrice,
                ),
              );
            },
          ),
          if (activeLevel != null)
            Positioned(
              left: PSpacing.xs,
              top: widget.isBid ? PSpacing.xs : null,
              bottom: widget.isBid ? null : PSpacing.xs,
              child: IgnorePointer(
                child: _DepthStatsCard(
                  level: activeLevel,
                  color: widget.color,
                  relTicker: widget.relTicker,
                  referencePrice: widget.referencePrice,
                  relUsdPrice: widget.relUsdPrice,
                ),
              ),
            ),
        ],
      ),
    );
  }

  bool _isInSelectedRange(_AggregatedLevel level, _AggregatedLevel selected) {
    return widget.isBid
        ? level.price >= selected.price
        : level.price <= selected.price;
  }
}

class _OrderBookRow extends StatelessWidget {
  const _OrderBookRow({
    required this.price,
    required this.amount,
    required this.total,
    required this.color,
    required this.depthFraction,
    required this.priceDecimals,
    required this.isHighlighted,
    required this.onHover,
    required this.onTap,
    this.usdPriceLabel,
  });

  final Decimal price;
  final Decimal amount;
  final Decimal total;
  final Color color;
  final double depthFraction;
  final int priceDecimals;
  final bool isHighlighted;
  final VoidCallback onHover;
  final VoidCallback onTap;
  final String? usdPriceLabel;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => onHover(),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: onTap,
          child: SizedBox(
            height: usdPriceLabel == null ? 22 : 34,
            child: Stack(
              alignment: Alignment.centerRight,
              children: [
                // Cumulative depth bar (right-aligned, MEXC-style).
                FractionallySizedBox(
                  alignment: Alignment.centerRight,
                  widthFactor: depthFraction,
                  child: Container(color: color.withValues(alpha: 0.13)),
                ),
                if (isHighlighted)
                  Positioned.fill(
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: color.withValues(alpha: 0.15),
                      ),
                    ),
                  ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: PSpacing.xs),
                  child: Row(
                    children: [
                      Expanded(
                        flex: 3,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Text(
                              formatSwapAmountFixed(
                                price,
                                fractionDigits: priceDecimals.clamp(2, 8),
                              ),
                              style: TextStyle(
                                color: color,
                                fontSize: 12,
                                fontFeatures: const [
                                  FontFeature.tabularFigures(),
                                ],
                                fontWeight: FontWeight.w500,
                              ),
                            ),
                            if (usdPriceLabel != null)
                              Text(
                                usdPriceLabel!,
                                style: TextStyle(
                                  color: AppColors.textTertiary,
                                  fontSize: 10,
                                  fontFeatures: const [
                                    FontFeature.tabularFigures(),
                                  ],
                                ),
                              ),
                          ],
                        ),
                      ),
                      Expanded(
                        flex: 3,
                        child: Text(
                          _formatAmount(amount),
                          textAlign: TextAlign.right,
                          style: TextStyle(
                            color: AppColors.textPrimary,
                            fontSize: 12,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ),
                      Expanded(
                        flex: 3,
                        child: Text(
                          _formatAmount(total, decimals: 4),
                          textAlign: TextAlign.right,
                          style: TextStyle(
                            color: AppColors.textSecondary,
                            fontSize: 12,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  String _formatAmount(Decimal value, {int decimals = 2}) {
    if (value.compareTo(Decimal.parse('1000')) >= 0) {
      return value.toBigInt().toString().replaceAllMapped(
        RegExp(r'(\d)(?=(\d{3})+(?!\d))'),
        (m) => '${m[1]},',
      );
    }
    return formatSwapAmount(value, fractionDigits: decimals);
  }
}

class _DepthStatsCard extends StatelessWidget {
  const _DepthStatsCard({
    required this.level,
    required this.color,
    required this.relTicker,
    this.referencePrice,
    this.relUsdPrice,
  });

  final _AggregatedLevel level;
  final Color color;
  final String relTicker;
  final Decimal? referencePrice;
  final double? relUsdPrice;

  @override
  Widget build(BuildContext context) {
    final avgPrice = level.cumulativeAmount > Decimal.zero
        ? (level.cumulativeTotal / level.cumulativeAmount).toDecimal(
            scaleOnInfinitePrecision: 8,
          )
        : level.price;
    final avgUsdLabel = _usdPriceLabel(
      relPrice: avgPrice,
      relUsdPrice: relUsdPrice,
    );

    return DecoratedBox(
      decoration: BoxDecoration(
        color: AppColors.backgroundElevated.withValues(alpha: 0.96),
        border: Border.all(color: color.withValues(alpha: 0.32)),
        borderRadius: BorderRadius.circular(10),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.22),
            blurRadius: 18,
            offset: const Offset(0, 8),
          ),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.all(PSpacing.sm),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _DepthStatRow(
              label: 'Avg. Price:'.tr,
              value: avgUsdLabel == null
                  ? formatSwapAmount(avgPrice, fractionDigits: 8)
                  : '${formatSwapAmount(avgPrice, fractionDigits: 8)} $avgUsdLabel',
            ),
            const SizedBox(height: 6),
            _DepthStatRow(
              label: 'Sum ARRR:'.tr,
              value: formatSwapAmount(
                level.cumulativeAmount,
                fractionDigits: 2,
              ),
            ),
            const SizedBox(height: 6),
            _DepthStatRow(
              label: 'Sum {ticker}:'.trArgs({'ticker': relTicker}),
              value: formatSwapAmount(level.cumulativeTotal, fractionDigits: 4),
            ),
          ],
        ),
      ),
    );
  }
}

class _DepthStatRow extends StatelessWidget {
  const _DepthStatRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 240,
      child: Row(
        children: [
          Expanded(
            child: Text(
              label,
              style: TextStyle(
                color: AppColors.textSecondary,
                fontSize: 12,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          Text(
            value,
            textAlign: TextAlign.right,
            style: TextStyle(
              color: AppColors.textPrimary,
              fontSize: 12,
              fontWeight: FontWeight.w700,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }
}

class _MidPriceRow extends StatelessWidget {
  const _MidPriceRow({
    required this.price,
    required this.asks,
    required this.bids,
    this.relUsdPrice,
  });

  final Decimal? price;
  final List<SwapOrderbookLevel> asks;
  final List<SwapOrderbookLevel> bids;
  final double? relUsdPrice;

  @override
  Widget build(BuildContext context) {
    if (price == null) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: PSpacing.sm),
        child: Center(
          child: Text(
            '---',
            style: PTypography.titleMedium(color: AppColors.textTertiary),
          ),
        ),
      );
    }

    // Determine if last move was up or down (based on best bid vs best ask).
    final goingUp =
        bids.isNotEmpty &&
        asks.isNotEmpty &&
        bids.first.priceLtcPerArrr.compareTo(asks.first.priceLtcPerArrr) >= 0;
    final color = goingUp ? AppColors.success : AppColors.error;
    final usdLabel = _usdPriceLabel(relPrice: price!, relUsdPrice: relUsdPrice);

    return Container(
      padding: const EdgeInsets.symmetric(vertical: PSpacing.sm),
      decoration: BoxDecoration(
        border: Border(
          top: BorderSide(color: AppColors.borderSubtle),
          bottom: BorderSide(color: AppColors.borderSubtle),
        ),
      ),
      child: Row(
        children: [
          Text(
            formatSwapAmount(price!, fractionDigits: 8),
            style: TextStyle(
              color: color,
              fontSize: 16,
              fontWeight: FontWeight.w700,
              letterSpacing: -0.3,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
          const SizedBox(width: PSpacing.xs),
          Icon(
            goingUp ? Icons.arrow_upward : Icons.arrow_downward,
            color: color,
            size: 14,
          ),
          const SizedBox(width: PSpacing.md),
          Text(
            '≈ 1 ARRR'.tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          if (usdLabel != null) ...[
            const SizedBox(width: PSpacing.md),
            Text(
              usdLabel,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ],
      ),
    );
  }
}

class _ViewModeToggle extends StatelessWidget {
  const _ViewModeToggle({required this.mode, required this.onChanged});

  final OrderBookViewMode mode;
  final ValueChanged<OrderBookViewMode> onChanged;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _ToggleIcon(
          icon: _BookIconType.both,
          isSelected: mode == OrderBookViewMode.both,
          onTap: () => onChanged(OrderBookViewMode.both),
          tooltip: 'All orders'.tr,
        ),
        const SizedBox(width: 6),
        _ToggleIcon(
          icon: _BookIconType.bidsOnly,
          isSelected: mode == OrderBookViewMode.bidsOnly,
          onTap: () => onChanged(OrderBookViewMode.bidsOnly),
          tooltip: 'Bids only'.tr,
        ),
        const SizedBox(width: 6),
        _ToggleIcon(
          icon: _BookIconType.asksOnly,
          isSelected: mode == OrderBookViewMode.asksOnly,
          onTap: () => onChanged(OrderBookViewMode.asksOnly),
          tooltip: 'Asks only'.tr,
        ),
      ],
    );
  }
}

enum _BookIconType { both, bidsOnly, asksOnly }

class _ToggleIcon extends StatelessWidget {
  const _ToggleIcon({
    required this.icon,
    required this.isSelected,
    required this.onTap,
    required this.tooltip,
  });

  final _BookIconType icon;
  final bool isSelected;
  final VoidCallback onTap;
  final String tooltip;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: Material(
        color: isSelected ? AppColors.backgroundElevated : Colors.transparent,
        borderRadius: BorderRadius.circular(4),
        child: InkWell(
          borderRadius: BorderRadius.circular(4),
          onTap: onTap,
          child: Container(
            padding: const EdgeInsets.all(5),
            child: SizedBox(
              width: 14,
              height: 14,
              child: CustomPaint(
                painter: _BookIconPainter(type: icon, isSelected: isSelected),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _BookIconPainter extends CustomPainter {
  _BookIconPainter({required this.type, required this.isSelected});

  final _BookIconType type;
  final bool isSelected;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..strokeCap = StrokeCap.round
      ..strokeWidth = 1.6;

    final dimAlpha = isSelected ? 0.25 : 0.4;

    // Determine top/bottom colors based on icon type
    final (Color topColor, Color bottomColor) = switch (type) {
      _BookIconType.both => (AppColors.error, AppColors.success),
      _BookIconType.bidsOnly => (
        AppColors.error.withValues(alpha: dimAlpha),
        AppColors.success,
      ),
      _BookIconType.asksOnly => (
        AppColors.error,
        AppColors.success.withValues(alpha: dimAlpha),
      ),
    };

    // Top three lines (asks, longer at top, shorter at bottom = closer to mid)
    paint.color = topColor;
    final widths1 = [size.width, size.width * 0.7, size.width * 0.5];
    for (var i = 0; i < 3; i++) {
      final y = i * 2.5 + 1;
      canvas.drawLine(
        Offset(size.width - widths1[i], y),
        Offset(size.width, y),
        paint,
      );
    }

    // Bottom three lines (bids, shorter at top, longer at bottom)
    paint.color = bottomColor;
    final widths2 = [size.width * 0.5, size.width * 0.7, size.width];
    for (var i = 0; i < 3; i++) {
      final y = (i * 2.5) + 8;
      canvas.drawLine(
        Offset(size.width - widths2[i], y),
        Offset(size.width, y),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _BookIconPainter oldDelegate) =>
      oldDelegate.type != type || oldDelegate.isSelected != isSelected;
}

class _GroupingSelector extends StatelessWidget {
  const _GroupingSelector({
    required this.value,
    required this.options,
    required this.onChanged,
  });

  final Decimal value;
  final List<Decimal> options;
  final ValueChanged<Decimal> onChanged;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: PSpacing.xs),
      decoration: BoxDecoration(
        color: AppColors.backgroundElevated,
        borderRadius: BorderRadius.circular(4),
      ),
      child: DropdownButton<Decimal>(
        value: value,
        dropdownColor: AppColors.backgroundElevated,
        style: PTypography.labelSmall(color: AppColors.textPrimary),
        underline: const SizedBox(),
        icon: Icon(
          Icons.arrow_drop_down,
          color: AppColors.textSecondary,
          size: 16,
        ),
        isDense: true,
        items: options.map((d) {
          return DropdownMenuItem(
            value: d,
            child: Text(
              _formatTickLabel(d),
              style: PTypography.labelSmall(
                color: d == value
                    ? AppColors.accentPrimary
                    : AppColors.textPrimary,
              ),
            ),
          );
        }).toList(),
        onChanged: (v) {
          if (v != null) onChanged(v);
        },
      ),
    );
  }

  String _formatTickLabel(Decimal d) {
    // Always render in plain decimal notation (no scientific exponent).
    final value = d.toDouble();
    final str = value.toStringAsFixed(10);
    final trimmed = str
        .replaceAll(RegExp(r'0+$'), '')
        .replaceAll(RegExp(r'\.$'), '');
    return trimmed.isEmpty ? '0' : trimmed;
  }
}

// ---------------------------------------------------------------------------
// Trade Form (Buy/Sell Market/Limit) - unchanged from prior version
// ---------------------------------------------------------------------------

class _TradeForm extends StatelessWidget {
  const _TradeForm({
    required this.state,
    required this.walletBalance,
    required this.relUsdPrice,
    required this.averageArrrUsdPriceLabel,
    required this.onPayAmountChanged,
    required this.onLtcAddressChanged,
    required this.onSlippageChanged,
    required this.onLimitPriceChanged,
    required this.onOrderTypeChanged,
    required this.onSideChanged,
    this.onMax,
  });

  final SwapViewModelState state;
  final String? walletBalance;
  final double? relUsdPrice;
  final String? averageArrrUsdPriceLabel;
  final ValueChanged<String> onPayAmountChanged;
  final ValueChanged<String> onLtcAddressChanged;
  final ValueChanged<double> onSlippageChanged;
  final ValueChanged<String> onLimitPriceChanged;
  final ValueChanged<SwapAdvancedOrderType> onOrderTypeChanged;
  final ValueChanged<SwapSide> onSideChanged;
  final VoidCallback? onMax;

  @override
  Widget build(BuildContext context) {
    final isBuy = state.isBuy;
    final activeColor = isBuy ? AppColors.success : AppColors.error;
    final ltcAddressError =
        !isBuy &&
            state.pair.relAsset == SwapAsset.ltc &&
            state.ltcPayoutAddress.trim().isNotEmpty
        ? checkLtcAddress(state.ltcPayoutAddress).error
        : null;
    final limitUsdLabel = _limitUsdLabel();
    final estimatedReceiveAmount = formatSwapAmountTextRounded(
      state.receiveAmountText,
    );

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DecoratedBox(
            decoration: BoxDecoration(
              color: AppColors.backgroundElevated,
              borderRadius: BorderRadius.circular(PSpacing.radiusMD),
            ),
            child: Row(
              children: [
                Expanded(
                  child: _TradeTab(
                    label: 'Buy'.tr,
                    isSelected: isBuy,
                    activeColor: AppColors.success,
                    onTap: () => onSideChanged(SwapSide.buyArrr),
                  ),
                ),
                Expanded(
                  child: _TradeTab(
                    label: 'Sell'.tr,
                    isSelected: !isBuy,
                    activeColor: AppColors.error,
                    onTap: () => onSideChanged(SwapSide.sellArrr),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: PSpacing.lg),
          Row(
            children: [
              _OrderTypeChip(
                label: 'Market'.tr,
                isSelected:
                    state.advancedOrderType == SwapAdvancedOrderType.market,
                onTap: () => onOrderTypeChanged(SwapAdvancedOrderType.market),
              ),
              const SizedBox(width: PSpacing.sm),
              _OrderTypeChip(
                label: 'Limit'.tr,
                isSelected:
                    state.advancedOrderType == SwapAdvancedOrderType.limit,
                onTap: () => onOrderTypeChanged(SwapAdvancedOrderType.limit),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.lg),
          if (state.advancedOrderType == SwapAdvancedOrderType.limit) ...[
            PInput(
              label: 'Limit price ({ticker} per ARRR)'.trArgs({
                'ticker': state.pair.relTicker,
              }),
              hint: '0.0000',
              value: state.limitPriceText,
              onChanged: onLimitPriceChanged,
              errorText: state.limitPriceError,
              keyboardType: const TextInputType.numberWithOptions(
                decimal: true,
              ),
              inputFormatters: [
                FilteringTextInputFormatter.allow(RegExp(r'^\d*\.?\d{0,8}')),
              ],
              suffixIcon: Padding(
                padding: const EdgeInsets.all(PSpacing.md),
                child: Text(
                  state.pair.relTicker,
                  style: PTypography.bodyMedium(color: AppColors.textSecondary),
                ),
              ),
            ),
            if (limitUsdLabel != null) ...[
              const SizedBox(height: PSpacing.xs),
              Text(
                '${'USD price'.tr}: $limitUsdLabel',
                style: PTypography.bodySmall(color: AppColors.textSecondary),
              ),
            ],
            const SizedBox(height: PSpacing.md),
          ],
          Row(
            children: [
              Expanded(
                child: Text(
                  'Amount'.tr,
                  style: PTypography.labelMedium(
                    color: AppColors.textSecondary,
                  ),
                ),
              ),
              if (!isBuy && onMax != null)
                GestureDetector(
                  onTap: onMax,
                  child: Padding(
                    padding: const EdgeInsets.only(bottom: PSpacing.xs),
                    child: Text(
                      'Max'.tr,
                      style: PTypography.labelSmall(
                        color: AppColors.accentPrimary,
                      ),
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: PSpacing.xs),
          PInput(
            hint: '0.00',
            value: state.payAmountText,
            onChanged: onPayAmountChanged,
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
            inputFormatters: [
              FilteringTextInputFormatter.allow(RegExp(r'^\d*\.?\d{0,8}')),
            ],
            suffixIcon: Padding(
              padding: const EdgeInsets.all(PSpacing.md),
              child: Text(
                isBuy ? state.pair.relTicker : 'ARRR',
                style: PTypography.bodyMedium(color: AppColors.textSecondary),
              ),
            ),
            helperText: !isBuy && walletBalance != null
                ? '${'Avail'.tr}: $walletBalance ARRR'
                : null,
          ),
          if (state.advancedOrderType == SwapAdvancedOrderType.market) ...[
            const SizedBox(height: PSpacing.lg),
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text(
                  'Slippage tolerance'.tr,
                  style: PTypography.labelMedium(
                    color: AppColors.textSecondary,
                  ),
                ),
                Text(
                  '${state.slippagePercent.toStringAsFixed(1)}%',
                  style: PTypography.labelMedium(color: activeColor),
                ),
              ],
            ),
            Slider(
              value: state.slippagePercent,
              min: 0.5,
              max: 25.0,
              divisions: 49,
              activeColor: activeColor,
              inactiveColor: AppColors.borderSubtle,
              onChanged: onSlippageChanged,
            ),
          ],
          if (!isBuy) ...[
            const SizedBox(height: PSpacing.md),
            PInput(
              label: '{ticker} receiving address'.trArgs({
                'ticker': state.pair.relTicker,
              }),
              value: state.ltcPayoutAddress,
              onChanged: onLtcAddressChanged,
              monospace: true,
              autocorrect: false,
              enableSuggestions: false,
              errorText: ltcAddressError,
              suffixIcon: IconButton(
                tooltip: 'Paste'.tr,
                icon: Icon(
                  Icons.content_paste,
                  size: 18,
                  color: AppColors.textSecondary,
                ),
                onPressed: () async {
                  final data = await Clipboard.getData('text/plain');
                  final pasted = data?.text?.trim();
                  if (pasted != null && pasted.isNotEmpty) {
                    onLtcAddressChanged(pasted);
                  }
                },
              ),
            ),
          ],
          if (state.receiveAmountText.isNotEmpty) ...[
            const SizedBox(height: PSpacing.lg),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: activeColor.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                border: Border.all(color: activeColor.withValues(alpha: 0.2)),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      'Estimated receive'.tr,
                      style: PTypography.bodyMedium(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ),
                  const SizedBox(width: PSpacing.md),
                  Flexible(
                    child: Text(
                      '$estimatedReceiveAmount ${isBuy ? 'ARRR' : state.pair.relTicker}',
                      textAlign: TextAlign.end,
                      style: PTypography.bodyMedium(
                        color: activeColor,
                      ).copyWith(fontWeight: PTypography.bold),
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (averageArrrUsdPriceLabel != null) ...[
            const SizedBox(height: PSpacing.sm),
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text(
                  'Avg ARRR price'.tr,
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                ),
                Text(
                  averageArrrUsdPriceLabel!,
                  style: PTypography.bodySmall(color: AppColors.textPrimary),
                ),
              ],
            ),
          ],
          if (state.capNotice != null) ...[
            const SizedBox(height: PSpacing.md),
            Container(
              padding: const EdgeInsets.all(PSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.warning.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                border: Border.all(
                  color: AppColors.warning.withValues(alpha: 0.3),
                ),
              ),
              child: Row(
                children: [
                  Icon(Icons.info_outline, color: AppColors.warning, size: 16),
                  const SizedBox(width: PSpacing.xs),
                  Expanded(
                    child: Text(
                      state.capNotice!,
                      style: PTypography.bodySmall(color: AppColors.warning),
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (state.quoteError != null) ...[
            const SizedBox(height: PSpacing.md),
            Text(
              state.quoteError!,
              style: PTypography.bodySmall(color: AppColors.error),
            ),
          ],
        ],
      ),
    );
  }

  String? _limitUsdLabel() {
    if (state.advancedOrderType != SwapAdvancedOrderType.limit) return null;
    final price = Decimal.tryParse(state.limitPriceText.trim());
    if (price == null || price <= Decimal.zero) return null;
    return _usdPriceLabel(
      relPrice: price,
      relUsdPrice: relUsdPrice,
      suffix: 'per ARRR'.tr,
    );
  }
}

class _TradeTab extends StatelessWidget {
  const _TradeTab({
    required this.label,
    required this.isSelected,
    required this.activeColor,
    required this.onTap,
  });

  final String label;
  final bool isSelected;
  final Color activeColor;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: PSpacing.md),
        decoration: BoxDecoration(
          color: isSelected
              ? activeColor.withValues(alpha: 0.15)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          border: Border.all(
            color: isSelected
                ? activeColor.withValues(alpha: 0.5)
                : Colors.transparent,
          ),
        ),
        child: Center(
          child: Text(
            label,
            style: PTypography.titleMedium(
              color: isSelected ? activeColor : AppColors.textSecondary,
            ),
          ),
        ),
      ),
    );
  }
}

class _OrderTypeChip extends StatelessWidget {
  const _OrderTypeChip({
    required this.label,
    required this.isSelected,
    required this.onTap,
  });

  final String label;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: PSpacing.md,
          vertical: PSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: isSelected ? AppColors.backgroundSurface : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusFull),
          border: Border.all(
            color: isSelected ? AppColors.borderStrong : AppColors.borderSubtle,
          ),
        ),
        child: Text(
          label,
          style: PTypography.labelMedium(
            color: isSelected ? AppColors.textPrimary : AppColors.textSecondary,
          ),
        ),
      ),
    );
  }
}
