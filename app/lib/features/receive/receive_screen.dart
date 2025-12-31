import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../core/ffi/ffi_bridge.dart' show AddressBookColorTag;
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart' show PTypography;
import '../../design/tokens/colors.dart' show AppColors;
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import 'widgets/address_qr_widget.dart';
import 'widgets/address_history_list.dart';
import 'receive_viewmodel.dart';

/// Receive screen for displaying payment addresses with QR codes
class ReceiveScreen extends ConsumerStatefulWidget {
  const ReceiveScreen({super.key});

  @override
  ConsumerState<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends ConsumerState<ReceiveScreen> {
  late final TextEditingController _amountController;
  late final TextEditingController _memoController;
  late final TextInputFormatter _amountFormatter;
  final ScrollController _scrollController = ScrollController();

  @override
  void initState() {
    super.initState();
    _amountController = TextEditingController();
    _memoController = TextEditingController();
    _amountFormatter = TextInputFormatter.withFunction((oldValue, newValue) {
      final text = newValue.text;
      if (text.isEmpty) return newValue;
      if (!RegExp(r'^\d+(\.\d{0,8})?$').hasMatch(text)) {
        return oldValue;
      }
      return newValue;
    });
    _amountController.addListener(_handleRequestInputChanged);
    _memoController.addListener(_handleRequestInputChanged);
  }

  @override
  void dispose() {
    _amountController.dispose();
    _memoController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  void _handleRequestInputChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  String _buildPirateUri({
    required String address,
    required String amount,
    required String memo,
  }) {
    final params = <String, String>{};
    if (amount.isNotEmpty) {
      params['amount'] = amount;
    }
    if (memo.isNotEmpty) {
      params['memo'] = memo;
    }
    if (params.isEmpty) return address;

    final query = params.entries
        .map((entry) =>
            '${Uri.encodeQueryComponent(entry.key)}=${Uri.encodeQueryComponent(entry.value)}')
        .join('&');
    return 'pirate:$address?$query';
  }

  @override
  Widget build(BuildContext context) {
    try {
      // Safely watch the provider
      final state = ref.watch(receiveViewModelProvider);
      final viewModel = ref.read(receiveViewModelProvider.notifier);
      
      final amountText = _amountController.text.trim();
      final memoText = _memoController.text.trim();
      final hasRequestData = amountText.isNotEmpty || memoText.isNotEmpty;
      final requestUri = state.currentAddress == null
          ? null
          : _buildPirateUri(
              address: state.currentAddress!,
              amount: amountText,
              memo: memoText,
            );

      return PScaffold(
        appBar: const PAppBar(
          title: 'Receive',
          subtitle: 'Share a QR code to get paid.',
          actions: [WalletSwitcherButton(compact: true)],
          showBackButton: true,
          centerTitle: true,
        ),
        body: Builder(
          builder: (context) {
            try {
              return CustomScrollView(
                controller: _scrollController,
                slivers: [
          // Content
          SliverPadding(
            padding: EdgeInsets.all(PSpacing.lg),
            sliver: SliverList(
              delegate: SliverChildListDelegate([
                // Always show at least one state - ensure we never have an empty list
                // Loading state
                if (state.isLoading && state.currentAddress == null)
                  const Center(
                    child: Padding(
                      padding: EdgeInsets.all(32.0),
                      child: CircularProgressIndicator(),
                    ),
                  )
                else if (state.error != null && state.currentAddress == null)
                  // Error state
                  PCard(
                    child: Padding(
                      padding: EdgeInsets.all(PSpacing.md),
                      child: Column(
                        children: [
                          Icon(
                            Icons.error_outline,
                            size: 48,
                            color: AppColors.error,
                          ),
                          SizedBox(height: PSpacing.sm),
                          Text(
                            'Error loading address',
                            style: PTypography.heading3(),
                          ),
                          SizedBox(height: PSpacing.xs),
                          Text(
                            state.error!,
                            style: PTypography.bodySmall(),
                            textAlign: TextAlign.center,
                          ),
                          SizedBox(height: PSpacing.md),
                          PButton(
                            onPressed: viewModel.loadCurrentAddress,
                            variant: PButtonVariant.secondary,
                            child: const Text('Retry'),
                          ),
                        ],
                      ),
                    ),
                  )
                else if (state.currentAddress == null)
                  // Empty state (no address, no error, not loading)
                  PCard(
                    child: Padding(
                      padding: EdgeInsets.all(PSpacing.md),
                      child: Column(
                        children: [
                          Icon(
                            Icons.qr_code_2_outlined,
                            size: 48,
                            color: AppColors.textSecondary,
                          ),
                          SizedBox(height: PSpacing.sm),
                          Text(
                            'No address loaded',
                            style: PTypography.heading3(),
                          ),
                          SizedBox(height: PSpacing.xs),
                          Text(
                            'Tap the button below to generate a receive address',
                            style: PTypography.bodySmall(),
                            textAlign: TextAlign.center,
                          ),
                          SizedBox(height: PSpacing.md),
                          PButton(
                            onPressed: viewModel.loadCurrentAddress,
                            variant: PButtonVariant.primary,
                            child: const Text('Load Address'),
                          ),
                        ],
                      ),
                    ),
                  ),

                // Success state
                if (state.currentAddress != null) ...[
                  // QR Code Card
                  AddressQRWidget(
                    address: state.currentAddress!,
                    qrData: requestUri,
                    onCopy: () => viewModel.copyAddress(
                      context,
                      value: requestUri ?? state.currentAddress!,
                      successMessage: hasRequestData
                          ? 'Payment request copied! Will clear in 60 seconds'
                          : null,
                    ),
                    onShare: () => viewModel.shareAddress(
                      context,
                      value: requestUri ?? state.currentAddress!,
                      successMessage: hasRequestData
                          ? 'Payment request ready to share'
                          : null,
                    ),
                    copyTooltip: hasRequestData ? 'Copy request' : 'Copy address',
                    shareTooltip:
                        hasRequestData ? 'Share request' : 'Share address',
                  ),

                  SizedBox(height: PSpacing.lg),

                  // Payment request options
                  PCard(
                    backgroundColor: AppColors.backgroundPanel,
                    child: Padding(
                      padding: EdgeInsets.all(PSpacing.md),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            'Payment request (optional)',
                            style: PTypography.labelMedium(
                              color: AppColors.textSecondary,
                            ),
                          ),
                          SizedBox(height: PSpacing.sm),
                          PInput(
                            controller: _amountController,
                            label: 'Amount',
                            hint: '0.00 ARRR',
                            keyboardType: const TextInputType.numberWithOptions(
                              decimal: true,
                            ),
                            inputFormatters: [_amountFormatter],
                            helperText: 'Adds amount to the QR code',
                            autocorrect: false,
                            enableSuggestions: false,
                            textInputAction: TextInputAction.next,
                          ),
                          SizedBox(height: PSpacing.md),
                          PInput(
                            controller: _memoController,
                            label: 'Memo (optional)',
                            hint: 'Optional note for the sender',
                            maxLines: 3,
                            maxLength: 512,
                            helperText: 'Included in the payment request',
                          ),
                        ],
                      ),
                    ),
                  ),

                  SizedBox(height: PSpacing.lg),

                  // Privacy notice if address was shared
                  if (state.addressWasShared)
                    Padding(
                      padding: EdgeInsets.only(bottom: PSpacing.md),
                      child: Container(
                        padding: EdgeInsets.all(PSpacing.sm),
                        decoration: BoxDecoration(
                          color: AppColors.warningBackground,
                          borderRadius: BorderRadius.circular(8),
                          border: Border.all(color: AppColors.warningBorder),
                        ),
                        child: Row(
                          children: [
                            Icon(
                              Icons.info_outline,
                              color: AppColors.warning,
                              size: 18,
                            ),
                            SizedBox(width: PSpacing.xs),
                            Expanded(
                              child: Text(
                                'Address shared. Generate a new one for privacy.',
                                style: PTypography.bodySmall().copyWith(
                                  color: AppColors.warning,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  
                  // Action Buttons
                  Row(
                    children: [
                          Expanded(
                            child: PButton(
                              onPressed: viewModel.generateNewAddress,
                              icon: const Icon(Icons.refresh),
                              variant: state.addressWasShared 
                                  ? PButtonVariant.primary 
                                  : PButtonVariant.secondary,
                              child: const Text('New address'),
                            ),
                          ),
                      SizedBox(width: PSpacing.md),
                          Expanded(
                            child: PButton(
                              onPressed: () => viewModel.copyAddress(
                                context,
                                value: requestUri ?? state.currentAddress!,
                                successMessage: hasRequestData
                                    ? 'Payment request copied! Will clear in 60 seconds'
                                    : null,
                              ),
                              icon: const Icon(Icons.copy),
                              variant: state.addressWasShared 
                                  ? PButtonVariant.secondary 
                                  : PButtonVariant.primary,
                              child: Text(
                                hasRequestData
                                    ? 'Copy request'
                                    : 'Copy address',
                              ),
                            ),
                          ),
                    ],
                  ),

                  SizedBox(height: PSpacing.xl),

                  // Privacy Notice
                  PCard(
                    backgroundColor: AppColors.backgroundPanel,
                    child: Padding(
                      padding: EdgeInsets.all(PSpacing.md),
                      child: Column(
                        children: [
                          Icon(
                            Icons.shield_outlined,
                            color: AppColors.gradientAStart,
                            size: 24,
                          ),
                          SizedBox(height: PSpacing.sm),
                          Text(
                            'Each address is fresh and unlinkable for maximum privacy',
                            style: PTypography.bodySmall().copyWith(
                              color: AppColors.textSecondary,
                            ),
                            textAlign: TextAlign.center,
                          ),
                        ],
                      ),
                    ),
                  ),

                  SizedBox(height: PSpacing.xl),

                  // Address History Section
                  Text(
                    'Previous addresses',
                    style: PTypography.heading3(),
                  ),
                  SizedBox(height: PSpacing.sm),
                  Text(
                    'Your previously generated addresses. All remain valid.',
                    style: PTypography.bodySmall().copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  SizedBox(height: PSpacing.md),

                  // Address History List
                  AddressHistoryList(
                    addresses: state.addressHistory,
                    onCopy: (address) => viewModel.copySpecificAddress(context, address),
                    onLabel: (address) => _showLabelDialog(context, viewModel, address),
                    onColorTag: (address) =>
                        _showColorTagPicker(context, viewModel, address),
                    onOpen: (address) => context.go('/activity'),
                  ),
                  if (state.addressHistory.length >= 6) ...[
                    SizedBox(height: PSpacing.md),
                    Center(
                      child: PTextButton(
                        label: 'Back to top',
                        onPressed: () {
                          if (_scrollController.hasClients) {
                            _scrollController.animateTo(
                              0,
                              duration: const Duration(milliseconds: 250),
                              curve: Curves.easeOut,
                            );
                          }
                        },
                        variant: PTextButtonVariant.subtle,
                      ),
                    ),
                  ],
                ]
              ]),
            ),
          ),
          ],
                );
            } catch (e, stackTrace) {
              // If there's an error in the widget tree, show error screen
              debugPrint('Error in CustomScrollView: $e');
              debugPrint('Stack trace: $stackTrace');
              return Center(
                child: Padding(
                  padding: const EdgeInsets.all(PSpacing.xl),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(
                        Icons.error_outline,
                        size: 64,
                        color: AppColors.error,
                      ),
                      const SizedBox(height: PSpacing.md),
                      Text(
                        'Error rendering content',
                        style: PTypography.heading3(),
                        textAlign: TextAlign.center,
                      ),
                      const SizedBox(height: PSpacing.sm),
                      Text(
                        e.toString(),
                        style: PTypography.bodySmall(),
                        textAlign: TextAlign.center,
                      ),
                      const SizedBox(height: PSpacing.xl),
                      PButton(
                        onPressed: () {
                          ref.invalidate(receiveViewModelProvider);
                        },
                        variant: PButtonVariant.primary,
                        child: const Text('Retry'),
                      ),
                    ],
                  ),
                ),
              );
            }
          },
        ),
      );
    } catch (e, stackTrace) {
      // If there's an error, show error screen instead of grey screen
      debugPrint('Error in ReceiveScreen build: $e');
      debugPrint('Stack trace: $stackTrace');
      return PScaffold(
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(PSpacing.xl),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(
                  Icons.error_outline,
                  size: 64,
                  color: AppColors.error,
                ),
                const SizedBox(height: PSpacing.md),
                Text(
                  'Error loading receive screen',
                  style: PTypography.heading3(),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: PSpacing.sm),
                Text(
                  e.toString(),
                  style: PTypography.bodySmall(),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: PSpacing.xl),
                PButton(
                  onPressed: () {
                    // Try to reload by invalidating the provider
                    ref.invalidate(receiveViewModelProvider);
                  },
                  variant: PButtonVariant.primary,
                  child: const Text('Retry'),
                ),
              ],
            ),
          ),
        ),
      );
    }
  }

  /// Show dialog to label an address
  void _showLabelDialog(
    BuildContext context,
    ReceiveViewModel viewModel,
    AddressInfo address,
  ) {
    final controller = TextEditingController(text: address.label);

    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.backgroundElevated,
        title: Text('Label address', style: PTypography.heading3()),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Add a label to remember this address',
              style: PTypography.bodySmall().copyWith(
                color: AppColors.textSecondary,
              ),
            ),
            SizedBox(height: PSpacing.md),
            TextField(
              controller: controller,
              decoration: const InputDecoration(
                hintText: 'e.g., Exchange Payment',
                border: OutlineInputBorder(),
              ),
              maxLength: 50,
            ),
          ],
        ),
        actions: [
          PTextButton(
            label: 'Cancel',
            onPressed: () => Navigator.pop(context),
            variant: PTextButtonVariant.subtle,
          ),
          PButton(
            onPressed: () {
              viewModel.labelAddress(address.address, controller.text);
              Navigator.pop(context);
            },
            size: PButtonSize.small,
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }

  Future<void> _showColorTagPicker(
    BuildContext context,
    ReceiveViewModel viewModel,
    AddressInfo address,
  ) async {
    final selection = await showModalBottomSheet<AddressBookColorTag>(
      context: context,
      backgroundColor: AppColors.backgroundSurface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(
          top: Radius.circular(PSpacing.radiusLG),
        ),
      ),
      builder: (context) {
        return Padding(
          padding: EdgeInsets.all(PSpacing.lg),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Color tag',
                style: PTypography.heading4(),
              ),
              SizedBox(height: PSpacing.md),
              Wrap(
                spacing: PSpacing.sm,
                runSpacing: PSpacing.sm,
                children: AddressBookColorTag.values.map((tag) {
                  final isSelected = address.colorTag == tag;
                  return InkWell(
                    onTap: () => Navigator.of(context).pop(tag),
                    borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                    child: Container(
                      padding: EdgeInsets.symmetric(
                        horizontal: PSpacing.sm,
                        vertical: PSpacing.xs,
                      ),
                      decoration: BoxDecoration(
                        color: isSelected
                            ? AppColors.selectedBackground
                            : AppColors.backgroundPanel,
                        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                        border: Border.all(
                          color: isSelected
                              ? AppColors.borderStrong
                              : AppColors.borderSubtle,
                        ),
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Container(
                            width: 12,
                            height: 12,
                            decoration: BoxDecoration(
                              color: Color(tag.colorValue),
                              shape: BoxShape.circle,
                            ),
                          ),
                          SizedBox(width: PSpacing.xs),
                          Text(
                            tag.displayName,
                            style: PTypography.bodySmall(
                              color: AppColors.textPrimary,
                            ),
                          ),
                        ],
                      ),
                    ),
                  );
                }).toList(),
              ),
            ],
          ),
        );
      },
    );

    if (selection != null) {
      await viewModel.setAddressColorTag(address.address, selection);
    }
  }
}
