import 'dart:async';

import 'package:flutter/material.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/security/clipboard_manager.dart';
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_card.dart';

class SwapDepositPanel extends StatefulWidget {
  const SwapDepositPanel({
    required this.ltcAmount,
    required this.assetTicker,
    required this.depositAddress,
    required this.depositExpiresAt,
    required this.progressMessage,
    required this.isExecuting,
    required this.depositDetected,
    required this.onContinue,
    required this.onCancel,
    required this.onExpired,
    required this.onRecheck,
    super.key,
  });

  final String ltcAmount;
  final String assetTicker;
  final String? depositAddress;
  final DateTime? depositExpiresAt;
  final String? progressMessage;
  final bool isExecuting;
  final bool depositDetected;
  final VoidCallback onContinue;
  final VoidCallback onCancel;
  final FutureOr<void> Function() onExpired;
  final FutureOr<void> Function() onRecheck;

  @override
  State<SwapDepositPanel> createState() => _SwapDepositPanelState();
}

class _SwapDepositPanelState extends State<SwapDepositPanel> {
  Timer? _timer;
  Duration? _remaining;
  bool _didEmitExpired = false;

  @override
  void initState() {
    super.initState();
    _resetTimer();
  }

  @override
  void didUpdateWidget(SwapDepositPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.depositExpiresAt != widget.depositExpiresAt) {
      _didEmitExpired = false;
      _resetTimer();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _resetTimer() {
    _timer?.cancel();
    _updateRemaining();
    if (widget.depositExpiresAt != null) {
      _timer = Timer.periodic(const Duration(seconds: 1), (_) {
        _updateRemaining();
      });
    }
  }

  void _updateRemaining() {
    final expiresAt = widget.depositExpiresAt;
    if (expiresAt == null) {
      if (mounted) setState(() => _remaining = null);
      return;
    }

    final remaining = expiresAt.toUtc().difference(DateTime.now().toUtc());
    final clamped = remaining.isNegative ? Duration.zero : remaining;
    if (mounted) setState(() => _remaining = clamped);

    if (clamped == Duration.zero && !_didEmitExpired) {
      _didEmitExpired = true;
      _timer?.cancel();
      Future<void>.microtask(widget.onExpired);
    }
  }

  Color get _statusColor =>
      widget.depositDetected ? AppColors.success : AppColors.info;

  @override
  Widget build(BuildContext context) {
    final address = widget.depositAddress ?? '';
    final remaining = _remaining;
    final isExpired = remaining == Duration.zero;
    final assetTicker = widget.assetTicker;

    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.all(PSpacing.sm),
                decoration: BoxDecoration(
                  color: AppColors.warning.withValues(alpha: 0.1),
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  Icons.hourglass_top_outlined,
                  color: AppColors.warning,
                ),
              ),
              const SizedBox(width: PSpacing.md),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Awaiting deposit'.tr,
                      style: PTypography.heading5(color: AppColors.textPrimary),
                    ),
                    Text(
                      'Send at least this amount of {ticker}. Once enough confirmed balance is detected, the swap starts automatically.'
                          .trArgs({'ticker': assetTicker}),
                      style: PTypography.bodySmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          if (remaining != null) ...[
            const SizedBox(height: PSpacing.lg),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: isExpired
                    ? AppColors.error.withValues(alpha: 0.1)
                    : AppColors.warning.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                border: Border.all(
                  color: isExpired
                      ? AppColors.error.withValues(alpha: 0.2)
                      : AppColors.warning.withValues(alpha: 0.2),
                ),
              ),
              child: Row(
                children: [
                  Icon(
                    isExpired ? Icons.timer_off_outlined : Icons.timer_outlined,
                    color: isExpired ? AppColors.error : AppColors.warning,
                    size: 20,
                  ),
                  const SizedBox(width: PSpacing.sm),
                  Expanded(
                    child: Text(
                      isExpired
                          ? 'Deposit window expired. Do not send more to this quote. If you already sent {ticker}, check the funding balance once it confirms.'
                                .trArgs({'ticker': assetTicker})
                          : '${'Deposit window'.tr}: ${_formatRemaining(remaining)} ${'remaining'.tr}',
                      style: PTypography.bodySmall(
                        color: isExpired ? AppColors.error : AppColors.warning,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
          const SizedBox(height: PSpacing.xl),
          Container(
            padding: const EdgeInsets.all(PSpacing.lg),
            decoration: BoxDecoration(
              color: AppColors.backgroundElevated,
              borderRadius: BorderRadius.circular(PSpacing.radiusLG),
              border: Border.all(color: AppColors.borderSubtle),
            ),
            child: Column(
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      'Amount'.tr,
                      style: PTypography.bodyMedium(
                        color: AppColors.textSecondary,
                      ),
                    ),
                    Row(
                      children: [
                        Text(
                          '${widget.ltcAmount} $assetTicker',
                          style: PTypography.heading4(
                            color: AppColors.textPrimary,
                          ),
                        ),
                        const SizedBox(width: PSpacing.xs),
                        IconButton(
                          icon: Icon(
                            Icons.copy,
                            size: 18,
                            color: AppColors.textSecondary,
                          ),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(),
                          onPressed: () async {
                            await ClipboardManager.copyWithAutoClear(
                              widget.ltcAmount,
                            );
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('Amount copied'.tr)),
                              );
                            }
                          },
                        ),
                      ],
                    ),
                  ],
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: PSpacing.md),
                  child: Divider(color: AppColors.borderSubtle, height: 1),
                ),
                if (address.isNotEmpty) ...[
                  Center(
                    child: Container(
                      padding: const EdgeInsets.all(PSpacing.sm),
                      decoration: BoxDecoration(
                        color: Colors.white,
                        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                      ),
                      child: QrImageView(
                        data: address,
                        size: 160,
                        backgroundColor: Colors.white,
                      ),
                    ),
                  ),
                  const SizedBox(height: PSpacing.md),
                  Container(
                    padding: const EdgeInsets.all(PSpacing.sm),
                    decoration: BoxDecoration(
                      color: AppColors.backgroundSurface,
                      borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                      border: Border.all(color: AppColors.borderSubtle),
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: SelectableText(
                            address,
                            style: PTypography.bodySmall(
                              color: AppColors.textPrimary,
                            ).copyWith(fontFamily: 'monospace'),
                            textAlign: TextAlign.center,
                          ),
                        ),
                        IconButton(
                          icon: Icon(
                            Icons.copy,
                            size: 18,
                            color: AppColors.textSecondary,
                          ),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(),
                          onPressed: () async {
                            await ClipboardManager.copyWithAutoClear(address);
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('Address copied'.tr)),
                              );
                            }
                          },
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
          if (widget.progressMessage != null) ...[
            const SizedBox(height: PSpacing.xl),
            Container(
              padding: const EdgeInsets.all(PSpacing.md),
              decoration: BoxDecoration(
                color: _statusColor.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                border: Border.all(color: _statusColor.withValues(alpha: 0.2)),
              ),
              child: Row(
                children: [
                  if (widget.isExecuting)
                    SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: _statusColor,
                      ),
                    )
                  else
                    Icon(
                      widget.depositDetected
                          ? Icons.check_circle_outline
                          : Icons.search,
                      size: 18,
                      color: _statusColor,
                    ),
                  const SizedBox(width: PSpacing.md),
                  Expanded(
                    child: Text(
                      widget.progressMessage!,
                      style: PTypography.bodyMedium(color: _statusColor),
                    ),
                  ),
                ],
              ),
            ),
          ],
          if (!widget.isExecuting && !isExpired) ...[
            const SizedBox(height: PSpacing.sm),
            Align(
              alignment: Alignment.centerRight,
              child: TextButton.icon(
                onPressed: () => widget.onRecheck(),
                icon: Icon(
                  Icons.refresh,
                  size: 16,
                  color: AppColors.accentPrimary,
                ),
                label: Text(
                  'Check now'.tr,
                  style: PTypography.labelMedium(
                    color: AppColors.accentPrimary,
                  ),
                ),
              ),
            ),
          ],
          const SizedBox(height: PSpacing.md),
          PButton(
            text: widget.isExecuting
                ? 'Processing...'.tr
                : isExpired
                ? 'Deposit window expired'.tr
                : widget.depositDetected
                ? 'Start swap now'.tr
                : 'Check for deposit'.tr,
            fullWidth: true,
            loading: widget.isExecuting,
            onPressed: widget.isExecuting || isExpired
                ? null
                : widget.onContinue,
          ),
          const SizedBox(height: PSpacing.sm),
          PButton(
            text: 'Cancel swap'.tr,
            variant: PButtonVariant.ghost,
            fullWidth: true,
            onPressed: widget.isExecuting ? null : widget.onCancel,
          ),
        ],
      ),
    );
  }

  String _formatRemaining(Duration duration) {
    final hours = duration.inHours;
    final minutes = duration.inMinutes.remainder(60);
    final seconds = duration.inSeconds.remainder(60);
    if (hours > 0) {
      return '$hours:${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
    }
    return '$minutes:${seconds.toString().padLeft(2, '0')}';
  }
}
