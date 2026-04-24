import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/i18n/arb_text_localizer.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_snack.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Verifies a Unified Wallet payment disclosure.
class PaymentDisclosureVerifierScreen extends ConsumerStatefulWidget {
  const PaymentDisclosureVerifierScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  @override
  ConsumerState<PaymentDisclosureVerifierScreen> createState() =>
      _PaymentDisclosureVerifierScreenState();
}

class _PaymentDisclosureVerifierScreenState
    extends ConsumerState<PaymentDisclosureVerifierScreen> {
  final TextEditingController _controller = TextEditingController();
  final FocusNode _focusNode = FocusNode();

  bool _isVerifying = false;
  String? _error;
  PaymentDisclosureVerification? _result;

  @override
  void dispose() {
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  Future<void> _pasteFromClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text?.trim();
    if (text == null || text.isEmpty) {
      if (!mounted) return;
      PSnack.show(
        context: context,
        message: 'Clipboard does not contain a payment disclosure'.tr,
        variant: PSnackVariant.warning,
      );
      return;
    }

    final disclosure = _extractDisclosure(text);
    setState(() {
      _controller.text = disclosure;
      _controller.selection = TextSelection.collapsed(
        offset: _controller.text.length,
      );
      _error = null;
      _result = null;
    });
  }

  Future<void> _verify(String walletId) async {
    final disclosure = _extractDisclosure(_controller.text);
    if (disclosure.isEmpty) {
      setState(() {
        _error = 'Paste a payment disclosure first.'.tr;
        _result = null;
      });
      _focusNode.requestFocus();
      return;
    }

    setState(() {
      _controller.text = disclosure;
      _controller.selection = TextSelection.collapsed(
        offset: disclosure.length,
      );
      _isVerifying = true;
      _error = null;
      _result = null;
    });

    try {
      final result = await FfiBridge.verifyPaymentDisclosure(
        walletId: walletId,
        disclosure: disclosure,
      );
      if (!mounted) return;
      setState(() {
        _result = result;
        _isVerifying = false;
      });
      PSnack.show(
        context: context,
        message: 'Payment disclosure verified'.tr,
        variant: PSnackVariant.success,
      );
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _error = _friendlyError(error);
        _isVerifying = false;
      });
    }
  }

  void _clear() {
    setState(() {
      _controller.clear();
      _error = null;
      _result = null;
    });
    _focusNode.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final walletId = ref.watch(activeWalletProvider);
    final width = MediaQuery.of(context).size.width;
    final isMobile = PSpacing.isMobile(width);
    final content = _VerifierContent(
      controller: _controller,
      focusNode: _focusNode,
      walletId: walletId,
      isVerifying: _isVerifying,
      error: _error,
      result: _result,
      onPaste: _pasteFromClipboard,
      onClear: _clear,
      onVerify: walletId == null ? null : () => _verify(walletId),
      onCopy: _copyText,
      onErrorDismissed: () => setState(() => _error = null),
    );

    if (!widget.useScaffold) {
      return content;
    }

    return PScaffold(
      title: 'Verify Payment Disclosure'.tr,
      appBar: PAppBar(
        title: 'Verify Payment Disclosure'.tr,
        subtitle: 'Confirm a shared single-payment proof.'.tr,
        actions: [
          ConnectionStatusIndicator(full: !isMobile),
          if (!isMobile) const WalletSwitcherButton(compact: true),
        ],
      ),
      body: content,
    );
  }

  Future<void> _copyText(String text, String label) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (!mounted) return;
    PSnack.show(
      context: context,
      message: '$label copied'.tr,
      variant: PSnackVariant.success,
      duration: const Duration(seconds: 2),
    );
  }

  String _friendlyError(Object error) {
    final message = error.toString().replaceFirst('Exception: ', '').trim();
    if (message.contains('network') ||
        message.contains('connect') ||
        message.contains('lightwalletd')) {
      return 'Could not reach the configured lightwalletd endpoint. Check network settings and try again.'
          .tr;
    }
    if (message.contains('Unsupported payment disclosure prefix') ||
        message.contains('Invalid payment disclosure')) {
      return 'This does not look like a supported Pirate payment disclosure.'
          .tr;
    }
    if (message.contains('Failed to decrypt')) {
      return 'The disclosure was found, but it could not decrypt the selected output. Check that the disclosure was copied completely.'
          .tr;
    }
    return message.isEmpty ? 'Verification failed. Try again.'.tr : message;
  }
}

class _VerifierContent extends StatelessWidget {
  const _VerifierContent({
    required this.controller,
    required this.focusNode,
    required this.walletId,
    required this.isVerifying,
    required this.error,
    required this.result,
    required this.onPaste,
    required this.onClear,
    required this.onVerify,
    required this.onCopy,
    required this.onErrorDismissed,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final String? walletId;
  final bool isVerifying;
  final String? error;
  final PaymentDisclosureVerification? result;
  final VoidCallback onPaste;
  final VoidCallback onClear;
  final VoidCallback? onVerify;
  final Future<void> Function(String text, String label) onCopy;
  final VoidCallback onErrorDismissed;

  @override
  Widget build(BuildContext context) {
    final size = MediaQuery.of(context).size;
    final isWide = size.width >= 980;
    final padding = PSpacing.screenPadding(size.width);

    final inputCard = _DisclosureInputCard(
      controller: controller,
      focusNode: focusNode,
      walletId: walletId,
      isVerifying: isVerifying,
      onPaste: onPaste,
      onClear: onClear,
      onVerify: onVerify,
    );
    final resultPanel = _ResultPanel(
      error: error,
      result: result,
      onCopy: onCopy,
      onErrorDismissed: onErrorDismissed,
    );

    return ListView(
      padding: padding,
      children: [
        const _DisclosureHero(),
        const SizedBox(height: PSpacing.lg),
        if (isWide)
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(flex: 6, child: inputCard),
              const SizedBox(width: PSpacing.lg),
              Expanded(flex: 5, child: resultPanel),
            ],
          )
        else ...[
          inputCard,
          const SizedBox(height: PSpacing.lg),
          resultPanel,
        ],
      ],
    );
  }
}

class _DisclosureHero extends StatelessWidget {
  const _DisclosureHero();

  @override
  Widget build(BuildContext context) {
    return PCard(
      elevated: true,
      backgroundColor: AppColors.backgroundElevated,
      child: Container(
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(PSpacing.radiusCard),
          gradient: LinearGradient(
            colors: [
              AppColors.gradientAStart.withValues(alpha: 0.22),
              AppColors.gradientBStart.withValues(alpha: 0.12),
              Colors.transparent,
            ],
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
          ),
        ),
        padding: const EdgeInsets.all(PSpacing.lg),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Container(
              width: 52,
              height: 52,
              decoration: BoxDecoration(
                color: AppColors.accentPrimary.withValues(alpha: 0.16),
                borderRadius: BorderRadius.circular(PSpacing.radiusLG),
                border: Border.all(
                  color: AppColors.accentPrimary.withValues(alpha: 0.35),
                ),
              ),
              child: Icon(
                Icons.verified_user_outlined,
                color: AppColors.accentPrimary,
                size: 28,
              ),
            ),
            const SizedBox(width: PSpacing.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Verify a payment disclosure'.tr,
                    style: PTypography.heading4(color: AppColors.textPrimary),
                  ),
                  const SizedBox(height: PSpacing.xs),
                  Text(
                    "Paste a disclosure key to confirm the transaction, recipient, amount, and memo for one shielded output. It does not require a viewing key and does not reveal anyone else's wallet history."
                        .tr,
                    style: PTypography.bodyMedium(
                      color: AppColors.textSecondary,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _DisclosureInputCard extends StatelessWidget {
  const _DisclosureInputCard({
    required this.controller,
    required this.focusNode,
    required this.walletId,
    required this.isVerifying,
    required this.onPaste,
    required this.onClear,
    required this.onVerify,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final String? walletId;
  final bool isVerifying;
  final VoidCallback onPaste;
  final VoidCallback onClear;
  final VoidCallback? onVerify;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  'Disclosure key'.tr,
                  style: PTypography.titleLarge(color: AppColors.textPrimary),
                ),
              ),
              IconButton(
                tooltip: 'Paste'.tr,
                onPressed: isVerifying ? null : onPaste,
                icon: const Icon(Icons.content_paste_outlined),
                color: AppColors.textSecondary,
              ),
            ],
          ),
          const SizedBox(height: PSpacing.xs),
          Text(
            'Single transaction verification for Sapling outputs and Orchard actions.'
                .tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          const SizedBox(height: PSpacing.md),
          PInput(
            controller: controller,
            focusNode: focusNode,
            label: 'Paste disclosure'.tr,
            hint:
                'pirate-sapling-payment-disclosure1... or pirate-orchard-payment-disclosure1...'
                    .tr,
            helperText:
                'You can paste the raw key or text that contains the key.'.tr,
            maxLines: 7,
            autocorrect: false,
            enableSuggestions: false,
            keyboardType: TextInputType.multiline,
            textInputAction: TextInputAction.newline,
            monospace: true,
            enabled: !isVerifying,
          ),
          const SizedBox(height: PSpacing.lg),
          Wrap(
            spacing: PSpacing.sm,
            runSpacing: PSpacing.sm,
            children: [
              PButton(
                text: 'Verify disclosure'.tr,
                onPressed: walletId == null || isVerifying ? null : onVerify,
                loading: isVerifying,
                icon: const Icon(Icons.verified_outlined),
              ),
              PButton(
                text: 'Paste'.tr,
                variant: PButtonVariant.secondary,
                onPressed: isVerifying ? null : onPaste,
                icon: const Icon(Icons.content_paste_outlined),
              ),
              PButton(
                text: 'Clear'.tr,
                variant: PButtonVariant.ghost,
                onPressed: isVerifying ? null : onClear,
                icon: const Icon(Icons.close),
              ),
            ],
          ),
          if (walletId == null) ...[
            const SizedBox(height: PSpacing.md),
            _InlineNotice(
              icon: Icons.account_balance_wallet_outlined,
              title: 'No active wallet'.tr,
              message:
                  "Verification uses the active wallet's configured lightwalletd endpoint to fetch the transaction."
                      .tr,
              color: AppColors.warning,
              background: AppColors.warningBackground,
              border: AppColors.warningBorder,
            ),
          ],
        ],
      ),
    );
  }
}

class _ResultPanel extends StatelessWidget {
  const _ResultPanel({
    required this.error,
    required this.result,
    required this.onCopy,
    required this.onErrorDismissed,
  });

  final String? error;
  final PaymentDisclosureVerification? result;
  final Future<void> Function(String text, String label) onCopy;
  final VoidCallback onErrorDismissed;

  @override
  Widget build(BuildContext context) {
    if (result != null) {
      return _VerifiedResultCard(result: result!, onCopy: onCopy);
    }
    if (error != null) {
      return _VerificationErrorCard(
        message: error!,
        onDismissed: onErrorDismissed,
      );
    }
    return const _VerificationEmptyCard();
  }
}

class _VerificationEmptyCard extends StatelessWidget {
  const _VerificationEmptyCard();

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _ResultIconHeader(
            icon: Icons.manage_search_outlined,
            color: AppColors.textTertiary,
            title: 'Verification result'.tr,
          ),
          const SizedBox(height: PSpacing.md),
          Text(
            'The decoded payment details will appear here after verification.'
                .tr,
            style: PTypography.bodyMedium(color: AppColors.textSecondary),
          ),
          const SizedBox(height: PSpacing.lg),
          _InfoStep(
            index: '1',
            text: 'Paste a payment disclosure shared by the sender.'.tr,
          ),
          _InfoStep(index: '2', text: 'Verify it against the transaction.'.tr),
          _InfoStep(
            index: '3',
            text: 'Review the amount, recipient, memo, and txid.'.tr,
          ),
        ],
      ),
    );
  }
}

class _VerificationErrorCard extends StatelessWidget {
  const _VerificationErrorCard({
    required this.message,
    required this.onDismissed,
  });

  final String message;
  final VoidCallback onDismissed;

  @override
  Widget build(BuildContext context) {
    return PCard(
      backgroundColor: AppColors.errorBackground,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: _ResultIconHeader(
                  icon: Icons.error_outline,
                  color: AppColors.error,
                  title: 'Verification failed'.tr,
                ),
              ),
              IconButton(
                tooltip: 'Dismiss'.tr,
                onPressed: onDismissed,
                icon: const Icon(Icons.close),
                color: AppColors.textSecondary,
              ),
            ],
          ),
          const SizedBox(height: PSpacing.md),
          Text(
            message,
            style: PTypography.bodyMedium(color: AppColors.textPrimary),
          ),
          const SizedBox(height: PSpacing.md),
          Text(
            'Check that the disclosure was copied completely and that this wallet is connected to the matching Pirate network.'
                .tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
        ],
      ),
    );
  }
}

class _VerifiedResultCard extends StatelessWidget {
  const _VerifiedResultCard({required this.result, required this.onCopy});

  final PaymentDisclosureVerification result;
  final Future<void> Function(String text, String label) onCopy;

  @override
  Widget build(BuildContext context) {
    final summary = _verificationSummary(result);
    final memo = result.memo?.trim();
    return PCard(
      elevated: true,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: _ResultIconHeader(
                  icon: Icons.check_circle_outline,
                  color: AppColors.success,
                  title: 'Disclosure verified'.tr,
                ),
              ),
              IconButton(
                tooltip: 'Copy summary'.tr,
                onPressed: () => onCopy(summary, 'Verification summary'.tr),
                icon: const Icon(Icons.copy_all_outlined),
                color: AppColors.textSecondary,
              ),
            ],
          ),
          const SizedBox(height: PSpacing.sm),
          Text(
            'This disclosure decrypts one ${result.disclosureType} payment only.'
                .tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          const SizedBox(height: PSpacing.lg),
          _AmountBadge(amount: _formatArrr(result.amount)),
          const SizedBox(height: PSpacing.lg),
          _ProofField(
            label: 'Recipient'.tr,
            value: result.address,
            onCopy: () => onCopy(result.address, 'Recipient address'.tr),
          ),
          _ProofField(
            label: 'Transaction ID'.tr,
            value: result.txid,
            onCopy: () => onCopy(result.txid, 'Transaction ID'.tr),
          ),
          _ProofField(
            label: result.disclosureType == 'orchard'
                ? 'Orchard action'.tr
                : 'Sapling output'.tr,
            value: '#${result.outputIndex}',
            compact: true,
          ),
          _ProofField(
            label: 'Memo'.tr,
            value: memo == null || memo.isEmpty ? 'No memo disclosed'.tr : memo,
            onCopy: memo == null || memo.isEmpty
                ? null
                : () => onCopy(memo, 'Memo'.tr),
          ),
          const SizedBox(height: PSpacing.md),
          PButton(
            text: 'Copy verification summary'.tr,
            variant: PButtonVariant.secondary,
            fullWidth: true,
            icon: const Icon(Icons.copy_all_outlined),
            onPressed: () => onCopy(summary, 'Verification summary'.tr),
          ),
        ],
      ),
    );
  }
}

class _ResultIconHeader extends StatelessWidget {
  const _ResultIconHeader({
    required this.icon,
    required this.color,
    required this.title,
  });

  final IconData icon;
  final Color color;
  final String title;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Container(
          padding: const EdgeInsets.all(PSpacing.xs),
          decoration: BoxDecoration(
            color: color.withValues(alpha: 0.14),
            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          ),
          child: Icon(icon, color: color, size: 20),
        ),
        const SizedBox(width: PSpacing.sm),
        Expanded(
          child: Text(
            title,
            style: PTypography.titleLarge(color: AppColors.textPrimary),
          ),
        ),
      ],
    );
  }
}

class _AmountBadge extends StatelessWidget {
  const _AmountBadge({required this.amount});

  final String amount;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.successBackground,
        borderRadius: BorderRadius.circular(PSpacing.radiusLG),
        border: Border.all(color: AppColors.successBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Verified amount'.tr,
            style: PTypography.labelSmall(color: AppColors.success),
          ),
          const SizedBox(height: PSpacing.xxs),
          SelectableText(
            amount,
            style: PTypography.heading4(color: AppColors.textPrimary),
          ),
        ],
      ),
    );
  }
}

class _ProofField extends StatelessWidget {
  const _ProofField({
    required this.label,
    required this.value,
    this.onCopy,
    this.compact = false,
  });

  final String label;
  final String value;
  final VoidCallback? onCopy;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: PSpacing.md),
      child: Container(
        width: double.infinity,
        padding: EdgeInsets.all(compact ? PSpacing.sm : PSpacing.md),
        decoration: BoxDecoration(
          color: AppColors.backgroundElevated.withValues(alpha: 0.72),
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          border: Border.all(color: AppColors.borderSubtle),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    label,
                    style: PTypography.labelSmall(
                      color: AppColors.textTertiary,
                    ),
                  ),
                ),
                if (onCopy != null)
                  IconButton(
                    tooltip: 'Copy $label'.tr,
                    visualDensity: VisualDensity.compact,
                    onPressed: onCopy,
                    icon: const Icon(Icons.copy, size: 18),
                    color: AppColors.textSecondary,
                  ),
              ],
            ),
            const SizedBox(height: PSpacing.xxs),
            SelectableText(
              value,
              style: compact
                  ? PTypography.bodyMedium(color: AppColors.textPrimary)
                  : PTypography.codeMedium(color: AppColors.textSecondary),
            ),
          ],
        ),
      ),
    );
  }
}

class _InfoStep extends StatelessWidget {
  const _InfoStep({required this.index, required this.text});

  final String index;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: PSpacing.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 24,
            height: 24,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: AppColors.infoBackground,
              shape: BoxShape.circle,
              border: Border.all(color: AppColors.infoBorder),
            ),
            child: Text(
              index,
              style: PTypography.labelSmall(color: AppColors.info),
            ),
          ),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              text,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ),
        ],
      ),
    );
  }
}

class _InlineNotice extends StatelessWidget {
  const _InlineNotice({
    required this.icon,
    required this.title,
    required this.message,
    required this.color,
    required this.background,
    required this.border,
  });

  final IconData icon;
  final String title;
  final String message;
  final Color color;
  final Color background;
  final Color border;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        border: Border.all(color: border),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, color: color, size: 20),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: PTypography.labelLarge(color: AppColors.textPrimary),
                ),
                const SizedBox(height: PSpacing.xxs),
                Text(
                  message,
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

String _extractDisclosure(String raw) {
  final trimmed = raw.trim();
  if (trimmed.isEmpty) {
    return '';
  }

  final match = RegExp(
    '(?:zpd:)?(?:pirate-sapling-payment-disclosure|pirate-orchard-payment-disclosure|zdisctest|odisctest|zdiscregtest|odiscregtest)1[023456789acdefghjklmnpqrstuvwxyz]+',
    caseSensitive: false,
  ).firstMatch(trimmed);
  return match?.group(0)?.trim() ?? trimmed;
}

String _formatArrr(BigInt arrrtoshis) {
  final sign = arrrtoshis.isNegative ? '-' : '';
  final absolute = arrrtoshis.abs();
  final whole = absolute ~/ BigInt.from(100000000);
  final fraction = (absolute % BigInt.from(100000000)).toString().padLeft(
    8,
    '0',
  );
  return '$sign$whole.$fraction ARRR';
}

String _verificationSummary(PaymentDisclosureVerification result) {
  final memo = result.memo?.trim();
  return [
    'Payment disclosure verified',
    'Type: ${result.disclosureType}',
    'Amount: ${_formatArrr(result.amount)}',
    'Recipient: ${result.address}',
    'Transaction ID: ${result.txid}',
    'Output/action index: ${result.outputIndex}',
    if (memo != null && memo.isNotEmpty) 'Memo: $memo',
  ].join('\n');
}
