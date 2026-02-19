import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart' hide AddressBookColorTag;
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';

class ConsolidateKeyScreen extends ConsumerStatefulWidget {
  const ConsolidateKeyScreen({super.key, required this.keyId});

  final int keyId;

  @override
  ConsumerState<ConsolidateKeyScreen> createState() =>
      _ConsolidateKeyScreenState();
}

class _ConsolidateKeyScreenState extends ConsumerState<ConsolidateKeyScreen> {
  final _addressController = TextEditingController();
  WalletId? _walletId;
  KeyGroupInfo? _key;
  List<AddressBalanceInfo> _addresses = [];
  AddressBalanceInfo? _selectedTargetAddress;
  bool _useOrchard = false;
  bool _isLoading = true;
  bool _isBuilding = false;
  bool _isSending = false;
  String? _error;
  PendingTx? _pending;

  @override
  void dispose() {
    _addressController.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final walletId = ref.read(activeWalletProvider);
    if (_walletId != walletId) {
      _walletId = walletId;
      _loadKey();
    }
  }

  Future<void> _loadKey() async {
    final walletId = _walletId;
    if (walletId == null) return;

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final keys = await FfiBridge.listKeyGroups(walletId);
      final key = keys.firstWhere((k) => k.id == widget.keyId);
      _key = key;
      _useOrchard = key.hasOrchard && !key.hasSapling;
      await _loadAddresses();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  Future<void> _generateAddress() async {
    final walletId = _walletId;
    if (walletId == null) return;
    try {
      final address = await FfiBridge.generateAddressForKey(
        walletId: walletId,
        keyId: widget.keyId,
        useOrchard: _useOrchard,
      );
      _addressController.text = address;
      await _loadAddresses(selectAddress: address);
    } catch (e) {
      if (mounted) {
        setState(() => _error = e.toString());
      }
    }
  }

  Future<void> _loadAddresses({String? selectAddress}) async {
    final walletId = _walletId;
    if (walletId == null) return;
    final addresses = await FfiBridge.listAddressBalances(
      walletId,
      keyId: widget.keyId,
    );
    addresses.sort((a, b) => b.createdAt.compareTo(a.createdAt));
    if (addresses.isEmpty) {
      if (mounted) {
        setState(() {
          _addresses = [];
          _selectedTargetAddress = null;
          _addressController.text = '';
        });
      }
      return;
    }
    AddressBalanceInfo selected = _selectedTargetAddress ?? addresses.first;
    if (selectAddress != null) {
      selected = addresses.firstWhere(
        (addr) => addr.address == selectAddress,
        orElse: () => selected,
      );
    } else if (_selectedTargetAddress != null) {
      selected = addresses.firstWhere(
        (addr) => addr.addressId == selected.addressId,
        orElse: () => selected,
      );
    }
    final resolved = selected;
    if (mounted) {
      setState(() {
        _addresses = addresses;
        _selectedTargetAddress = resolved;
        _addressController.text = resolved.address;
      });
    }
  }

  Future<void> _buildConsolidation() async {
    final walletId = _walletId;
    if (walletId == null) return;
    final target = _addressController.text.trim();
    if (target.isEmpty) {
      setState(() => _error = 'Enter a target address');
      return;
    }

    setState(() {
      _isBuilding = true;
      _error = null;
    });

    try {
      final pending = await FfiBridge.buildConsolidationTx(
        walletId: walletId,
        keyId: widget.keyId,
        targetAddress: target,
      );
      setState(() {
        _pending = pending;
      });
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isBuilding = false);
      }
    }
  }

  Future<void> _send() async {
    final walletId = _walletId;
    final pending = _pending;
    if (walletId == null || pending == null) return;

    setState(() {
      _isSending = true;
      _error = null;
    });

    try {
      final signed = await FfiBridge.signTxForKey(
        walletId: walletId,
        pending: pending,
        keyId: widget.keyId,
      );
      final txid = await FfiBridge.broadcastTx(signed);
      if (!mounted) return;
      await PDialog.show<void>(
        context: context,
        title: 'Consolidation sent'.tr,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Your consolidation transaction has been broadcast.'.tr,
              style: PTypography.bodyMedium(),
            ),
            SizedBox(height: PSpacing.sm),
            Text(
              txid,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ),
        actions: [PDialogAction(label: 'Close'.tr)],
      );
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isSending = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final padding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    return PScaffold(
      appBar: PAppBar(
        title: 'Consolidate balances'.tr,
        subtitle: 'Repack funds into a wallet address'.tr,
        showBackButton: true,
      ),
      body: _isLoading
          ? const Center(child: CircularProgressIndicator())
          : SingleChildScrollView(
              padding: padding,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (_key != null) _buildKeySummary(_key!),
                  SizedBox(height: PSpacing.md),
                  if (_key != null && _key!.hasSapling && _key!.hasOrchard)
                    _buildPoolToggle(),
                  if (_key != null && _key!.hasSapling && _key!.hasOrchard)
                    SizedBox(height: PSpacing.md),
                  _buildTargetAddressSelector(),
                  SizedBox(height: PSpacing.sm),
                  PButton(
                    onPressed: _isBuilding ? null : _generateAddress,
                    variant: PButtonVariant.secondary,
                    child: Text('Generate new address'.tr),
                  ),
                  SizedBox(height: PSpacing.md),
                  PButton(
                    onPressed: _isBuilding ? null : _buildConsolidation,
                    variant: PButtonVariant.primary,
                    child: Text(
                      _isBuilding ? 'Building...' : 'Preview consolidation',
                    ),
                  ),
                  if (_pending != null) ...[
                    SizedBox(height: PSpacing.lg),
                    _buildSummary(_pending!),
                    SizedBox(height: PSpacing.md),
                    PButton(
                      onPressed: _isSending ? null : _send,
                      variant: PButtonVariant.primary,
                      child: Text(
                        _isSending ? 'Sending...' : 'Confirm and send',
                      ),
                    ),
                  ],
                  if (_error != null) ...[
                    SizedBox(height: PSpacing.md),
                    Text(
                      _error!,
                      style: PTypography.bodySmall(color: AppColors.error),
                    ),
                  ],
                ],
              ),
            ),
    );
  }

  Widget _buildKeySummary(KeyGroupInfo key) {
    final label = _displayKeyLabel(key);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: PTypography.heading3()),
        SizedBox(height: PSpacing.xs),
        Text(
          'Birthday ${key.birthdayHeight}',
          style: PTypography.bodySmall(color: AppColors.textSecondary),
        ),
      ],
    );
  }

  Widget _buildPoolToggle() {
    return Row(
      children: [
        Expanded(
          child: PButton(
            onPressed: _useOrchard ? null : () => _setPool(true),
            variant: _useOrchard
                ? PButtonVariant.primary
                : PButtonVariant.secondary,
            child: Text('Orchard'.tr),
          ),
        ),
        SizedBox(width: PSpacing.sm),
        Expanded(
          child: PButton(
            onPressed: _useOrchard ? () => _setPool(false) : null,
            variant: _useOrchard
                ? PButtonVariant.secondary
                : PButtonVariant.primary,
            child: Text('Sapling'.tr),
          ),
        ),
      ],
    );
  }

  void _setPool(bool useOrchard) {
    setState(() {
      _useOrchard = useOrchard;
      _pending = null;
    });
    _generateAddress();
  }

  Widget _buildTargetAddressSelector() {
    if (_addresses.isEmpty) {
      return Text(
        'No wallet addresses available yet.'.tr,
        style: PTypography.bodySmall(color: AppColors.textSecondary),
      );
    }

    return DropdownMenuFormField<AddressBalanceInfo>(
      key: ValueKey(_selectedTargetAddress?.addressId),
      initialSelection: _selectedTargetAddress,
      width: double.infinity,
      label: Text('Target wallet address'.tr),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: AppColors.surfaceElevated,
      ),
      dropdownMenuEntries: _addresses
          .map(
            (address) => DropdownMenuEntry<AddressBalanceInfo>(
              value: address,
              label: _dropdownLabel(address),
            ),
          )
          .toList(),
      onSelected: (value) {
        if (value == null) return;
        setState(() {
          _selectedTargetAddress = value;
          _addressController.text = value.address;
          _pending = null;
        });
      },
    );
  }

  String _dropdownLabel(AddressBalanceInfo address) {
    final label = address.label?.trim();
    if (label != null && label.isNotEmpty) {
      return '$label • ${_truncate(address.address)}';
    }
    return _truncate(address.address);
  }

  Widget _buildSummary(PendingTx pending) {
    return Container(
      padding: EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.backgroundPanel,
        borderRadius: BorderRadius.circular(PSpacing.radiusCard),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Preview'.tr, style: PTypography.heading4()),
          SizedBox(height: PSpacing.sm),
          _summaryRow('Amount', _formatArrr(pending.totalAmount)),
          _summaryRow('Fee', _formatArrr(pending.fee)),
          _summaryRow('Inputs', '${pending.numInputs}'),
        ],
      ),
    );
  }

  Widget _summaryRow(String label, String value) {
    return Padding(
      padding: EdgeInsets.only(bottom: PSpacing.xs),
      child: Row(
        children: [
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              value,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.right,
              style: PTypography.bodyMedium(),
            ),
          ),
        ],
      ),
    );
  }

  String _formatArrr(BigInt value) {
    final amount = value.toDouble() / 100000000.0;
    return '${amount.toStringAsFixed(8)} ARRR';
  }

  String _displayKeyLabel(KeyGroupInfo key) {
    if (key.keyType == KeyTypeInfo.seed) {
      final label = key.label?.trim();
      if (label == null || label.isEmpty || label == 'Seed') {
        return 'Default wallet keys';
      }
    }
    return key.label ?? 'Key ${key.id}';
  }

  String _truncate(String address) {
    if (address.length <= 20) return address;
    return '${address.substring(0, 10)}...${address.substring(address.length - 10)}';
  }
}
