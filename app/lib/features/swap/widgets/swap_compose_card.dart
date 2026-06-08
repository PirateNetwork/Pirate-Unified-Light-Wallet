import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/swaps/ltc_address.dart';
import '../../../core/swaps/swap_models.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_input.dart';

typedef SwapAssetSelectionCallback =
    void Function({required bool isPayRow, required SwapAsset asset});

class SwapComposeCard extends StatelessWidget {
  const SwapComposeCard({
    required this.pair,
    required this.side,
    required this.payAmount,
    required this.receiveAmount,
    required this.isQuoting,
    required this.quoteError,
    required this.walletBalance,
    required this.ltcPayoutAddress,
    required this.slippagePercent,
    required this.onPayAmountChanged,
    required this.onReceiveAmountChanged,
    required this.onFlip,
    required this.onLtcAddressChanged,
    required this.onSlippageChanged,
    this.onMax,
    this.onAssetSelected,
    this.capNotice,
    this.feeLabel,
    this.rateLabel,
    this.averageArrrUsdPriceLabel,
    super.key,
  });

  final SwapPair pair;
  final SwapSide side;
  final String payAmount;
  final String receiveAmount;
  final bool isQuoting;
  final String? quoteError;
  final String? capNotice;
  final String? walletBalance;
  final String ltcPayoutAddress;
  final double slippagePercent;
  final ValueChanged<String> onPayAmountChanged;
  final ValueChanged<String> onReceiveAmountChanged;
  final VoidCallback onFlip;
  final ValueChanged<String> onLtcAddressChanged;
  final ValueChanged<double> onSlippageChanged;
  final VoidCallback? onMax;
  final SwapAssetSelectionCallback? onAssetSelected;
  final String? feeLabel;
  final String? rateLabel;
  final String? averageArrrUsdPriceLabel;

  bool get isBuy => side == SwapSide.buyArrr;

  Future<void> _showAssetPicker(
    BuildContext context, {
    required bool isPayRow,
    required SwapAsset currentAsset,
  }) async {
    final selected = await showModalBottomSheet<SwapAsset>(
      context: context,
      backgroundColor: AppColors.backgroundSurface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(
          top: Radius.circular(PSpacing.radiusXL),
        ),
      ),
      builder: (sheetContext) {
        return SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(PSpacing.lg),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  'Select coin'.tr,
                  style: PTypography.titleMedium(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.md),
                for (final candidate in SwapAsset.values.where(
                  (asset) => asset != currentAsset,
                ))
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(candidate.ticker),
                    subtitle: Text(_assetPickerSubtitle(candidate).tr),
                    onTap: () => Navigator.of(sheetContext).pop(candidate),
                  ),
              ],
            ),
          ),
        );
      },
    );
    if (selected != null && selected != currentAsset) {
      onAssetSelected?.call(isPayRow: isPayRow, asset: selected);
    }
  }

  String _assetPickerSubtitle(SwapAsset asset) {
    return switch (asset) {
      SwapAsset.arrr => 'Switch to paying or receiving ARRR',
      SwapAsset.ltc => 'Use LTC for this ARRR swap',
      SwapAsset.varrr => 'Use vARRR for this ARRR swap',
    };
  }

  String? get _ltcAddressError {
    if (isBuy || ltcPayoutAddress.trim().isEmpty) return null;
    if (pair.relAsset != SwapAsset.ltc) return null;
    final check = checkLtcAddress(ltcPayoutAddress);
    return check.isValid ? null : check.error;
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Main Swap Card
        DecoratedBox(
          decoration: BoxDecoration(
            color: AppColors.backgroundSurface,
            borderRadius: BorderRadius.circular(PSpacing.radiusXL),
            border: Border.all(color: AppColors.borderSubtle),
          ),
          child: Stack(
            alignment: Alignment.center,
            children: [
              Column(
                children: [
                  _PremiumAssetInput(
                    label: 'You pay'.tr,
                    asset: isBuy ? pair.relAsset : SwapAsset.arrr,
                    amount: payAmount,
                    hint: '0',
                    balanceLabel: isBuy
                        ? 'From external wallet'.tr
                        : walletBalance == null
                        ? 'Wallet balance'.tr
                        : '$walletBalance ARRR',
                    onChanged: onPayAmountChanged,
                    onMax: isBuy ? null : onMax,
                    onAssetTap: onAssetSelected == null
                        ? null
                        : () => _showAssetPicker(
                            context,
                            isPayRow: true,
                            currentAsset: isBuy
                                ? pair.relAsset
                                : SwapAsset.arrr,
                          ),
                    isTop: true,
                  ),
                  Divider(height: 1, color: AppColors.borderSubtle),
                  _PremiumAssetInput(
                    label: 'You receive'.tr,
                    asset: isBuy ? SwapAsset.arrr : pair.relAsset,
                    amount: receiveAmount,
                    hint: isQuoting ? '...' : '0',
                    balanceLabel: isBuy
                        ? 'To Pirate wallet'.tr
                        : 'To external wallet'.tr,
                    onChanged: onReceiveAmountChanged,
                    onAssetTap: onAssetSelected == null
                        ? null
                        : () => _showAssetPicker(
                            context,
                            isPayRow: false,
                            currentAsset: isBuy
                                ? SwapAsset.arrr
                                : pair.relAsset,
                          ),
                    isTop: false,
                  ),
                ],
              ),
              // Floating Flip Button with cutout effect
              Container(
                decoration: BoxDecoration(
                  color: AppColors.backgroundSurface,
                  shape: BoxShape.circle,
                  border: Border.all(color: AppColors.borderSubtle, width: 1),
                ),
                padding: const EdgeInsets.all(4), // Creates the gap
                child: Material(
                  color: AppColors.backgroundElevated,
                  shape: const CircleBorder(),
                  child: InkWell(
                    onTap: onFlip,
                    customBorder: const CircleBorder(),
                    child: Padding(
                      padding: const EdgeInsets.all(8.0),
                      child: Icon(
                        Icons.swap_vert,
                        color: AppColors.textPrimary,
                        size: 20,
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),

        if (rateLabel != null ||
            feeLabel != null ||
            averageArrrUsdPriceLabel != null) ...[
          const SizedBox(height: PSpacing.md),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: PSpacing.sm),
            child: Column(
              children: [
                if (rateLabel != null)
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        'Exchange rate'.tr,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                      Text(
                        rateLabel!,
                        style: PTypography.bodySmall(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ],
                  ),
                if (rateLabel != null &&
                    (feeLabel != null || averageArrrUsdPriceLabel != null))
                  const SizedBox(height: PSpacing.xs),
                if (feeLabel != null)
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        'Estimated fees'.tr,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                      Text(
                        feeLabel!,
                        style: PTypography.bodySmall(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ],
                  ),
                if (feeLabel != null && averageArrrUsdPriceLabel != null)
                  const SizedBox(height: PSpacing.xs),
                if (averageArrrUsdPriceLabel != null)
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        'Avg ARRR price'.tr,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                      Text(
                        averageArrrUsdPriceLabel!,
                        style: PTypography.bodySmall(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
        ],

        const SizedBox(height: PSpacing.lg),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: PSpacing.sm),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
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
                    '${slippagePercent.toStringAsFixed(1)}%',
                    style: PTypography.labelMedium(
                      color: AppColors.accentPrimary,
                    ),
                  ),
                ],
              ),
              Slider(
                value: slippagePercent,
                min: 0.5,
                max: 25.0,
                divisions: 49,
                activeColor: AppColors.accentPrimary,
                inactiveColor: AppColors.borderSubtle,
                onChanged: onSlippageChanged,
              ),
            ],
          ),
        ),

        if (!isBuy) ...[
          const SizedBox(height: PSpacing.xl),
          Text(
            '${pair.relTicker} receiving address'.tr,
            style: PTypography.labelMedium(color: AppColors.textPrimary),
          ),
          const SizedBox(height: PSpacing.xs),
          PInput(
            hint: pair.relAsset == SwapAsset.ltc
                ? 'ltc1... or L...'.tr
                : 'Enter ${pair.relTicker} address'.tr,
            value: ltcPayoutAddress,
            onChanged: onLtcAddressChanged,
            monospace: true,
            autocorrect: false,
            enableSuggestions: false,
            errorText: _ltcAddressError,
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

        if (capNotice != null) ...[
          const SizedBox(height: PSpacing.lg),
          Container(
            padding: const EdgeInsets.all(PSpacing.md),
            decoration: BoxDecoration(
              color: AppColors.warning.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(PSpacing.radiusMD),
              border: Border.all(
                color: AppColors.warning.withValues(alpha: 0.3),
              ),
            ),
            child: Row(
              children: [
                Icon(Icons.info_outline, color: AppColors.warning, size: 20),
                const SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Text(
                    capNotice!,
                    style: PTypography.bodySmall(color: AppColors.warning),
                  ),
                ),
              ],
            ),
          ),
        ],

        if (quoteError != null) ...[
          const SizedBox(height: PSpacing.lg),
          Container(
            padding: const EdgeInsets.all(PSpacing.md),
            decoration: BoxDecoration(
              color: AppColors.error.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(PSpacing.radiusMD),
            ),
            child: Row(
              children: [
                Icon(Icons.error_outline, color: AppColors.error, size: 20),
                const SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Text(
                    quoteError!,
                    style: PTypography.bodySmall(color: AppColors.error),
                  ),
                ),
              ],
            ),
          ),
        ],
      ],
    );
  }
}

class _PremiumAssetInput extends StatefulWidget {
  const _PremiumAssetInput({
    required this.label,
    required this.asset,
    required this.amount,
    required this.hint,
    required this.balanceLabel,
    required this.onChanged,
    required this.isTop,
    this.onMax,
    this.onAssetTap,
  });

  final String label;
  final SwapAsset asset;
  final String amount;
  final String hint;
  final String balanceLabel;
  final ValueChanged<String> onChanged;
  final bool isTop;
  final VoidCallback? onMax;
  final VoidCallback? onAssetTap;

  @override
  State<_PremiumAssetInput> createState() => _PremiumAssetInputState();
}

class _PremiumAssetInputState extends State<_PremiumAssetInput> {
  late TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.amount);
  }

  @override
  void didUpdateWidget(_PremiumAssetInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.amount != widget.amount &&
        _controller.text != widget.amount) {
      final selection = _controller.selection;
      _controller.text = widget.amount;
      if (selection.isValid && selection.baseOffset <= widget.amount.length) {
        _controller.selection = selection;
      } else {
        _controller.selection = TextSelection.collapsed(
          offset: widget.amount.length,
        );
      }
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(PSpacing.lg),
      decoration: BoxDecoration(
        color: widget.isTop
            ? Colors.transparent
            : AppColors.backgroundBase.withValues(alpha: 0.3),
        borderRadius: BorderRadius.vertical(
          top: Radius.circular(widget.isTop ? PSpacing.radiusXL : 0),
          bottom: Radius.circular(widget.isTop ? 0 : PSpacing.radiusXL),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            widget.label,
            style: PTypography.labelMedium(color: AppColors.textSecondary),
          ),
          const SizedBox(height: PSpacing.sm),
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Expanded(
                child: TextField(
                  controller: _controller,
                  onChanged: widget.onChanged,
                  keyboardType: const TextInputType.numberWithOptions(
                    decimal: true,
                  ),
                  inputFormatters: [
                    FilteringTextInputFormatter.allow(
                      RegExp(r'^\d*\.?\d{0,8}'),
                    ),
                  ],
                  style: TextStyle(
                    fontSize: 36,
                    fontWeight: FontWeight.w600,
                    color: AppColors.textPrimary,
                    letterSpacing: -1,
                    height: 1.2,
                  ),
                  decoration: InputDecoration(
                    hintText: widget.hint,
                    hintStyle: TextStyle(
                      fontSize: 36,
                      fontWeight: FontWeight.w600,
                      color: AppColors.textSecondary.withValues(alpha: 0.3),
                      letterSpacing: -1,
                      height: 1.2,
                    ),
                    border: InputBorder.none,
                    isDense: true,
                    contentPadding: EdgeInsets.zero,
                  ),
                ),
              ),
              const SizedBox(width: PSpacing.md),
              GestureDetector(
                onTap: widget.onAssetTap,
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: PSpacing.md,
                    vertical: PSpacing.sm,
                  ),
                  decoration: BoxDecoration(
                    color: AppColors.backgroundElevated,
                    borderRadius: BorderRadius.circular(100),
                    border: Border.all(color: AppColors.borderSubtle),
                    boxShadow: [
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.1),
                        blurRadius: 8,
                        offset: const Offset(0, 2),
                      ),
                    ],
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        widget.asset.ticker,
                        style: PTypography.titleMedium(
                          color: AppColors.textPrimary,
                        ),
                      ),
                      if (widget.onAssetTap != null) ...[
                        const SizedBox(width: 2),
                        Icon(
                          Icons.keyboard_arrow_down,
                          size: 18,
                          color: AppColors.textSecondary,
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: PSpacing.sm),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                '', // Placeholder for fiat value if needed later
                style: PTypography.bodySmall(color: AppColors.textSecondary),
              ),
              Row(
                children: [
                  Icon(
                    Icons.account_balance_wallet_outlined,
                    size: 14,
                    color: AppColors.textSecondary,
                  ),
                  const SizedBox(width: 4),
                  Text(
                    widget.balanceLabel,
                    style: PTypography.bodySmall(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  if (widget.onMax != null) ...[
                    const SizedBox(width: PSpacing.sm),
                    GestureDetector(
                      onTap: widget.onMax,
                      child: Container(
                        padding: const EdgeInsets.symmetric(
                          horizontal: PSpacing.sm,
                          vertical: 2,
                        ),
                        decoration: BoxDecoration(
                          color: AppColors.accentPrimary.withValues(
                            alpha: 0.12,
                          ),
                          borderRadius: BorderRadius.circular(
                            PSpacing.radiusFull,
                          ),
                        ),
                        child: Text(
                          'Max'.tr,
                          style: PTypography.labelSmall(
                            color: AppColors.accentPrimary,
                          ),
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ],
          ),
        ],
      ),
    );
  }
}
