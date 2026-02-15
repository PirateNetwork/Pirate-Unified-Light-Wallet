import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

class SweepKeyScreen extends ConsumerStatefulWidget {
  const SweepKeyScreen({super.key, required this.keyId});

  final int keyId;

  @override
  ConsumerState<SweepKeyScreen> createState() => _SweepKeyScreenState();
}

class _SweepKeyScreenState extends ConsumerState<SweepKeyScreen> {
  final _addressController = TextEditingController();
  WalletId? _walletId;
  KeyGroupInfo? _key;
  List<AddressBalanceInfo> _addresses = [];
  Set<int> _selectedAddressIds = {};
  bool _useOrchard = false;
  bool _isLoading = true;
  bool _isBuilding = false;
  bool _isSending = false;
  String? _error;
  PendingTx? _pending;
  List<int>? _pendingKeyIds;
  List<int>? _pendingAddressIds;

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
      final addresses = await FfiBridge.listAddressBalances(
        walletId,
        keyId: widget.keyId,
      );
      addresses.sort((a, b) => b.spendable.compareTo(a.spendable));
      _key = key;
      _addresses = addresses;
      _useOrchard = key.hasOrchard && !key.hasSapling;
      final addressIds = _addresses.map((addr) => addr.addressId).toSet();
      _selectedAddressIds.removeWhere((id) => !addressIds.contains(id));
      if (_addressController.text.trim().isEmpty) {
        try {
          _addressController.text = await FfiBridge.currentReceiveAddress(
            walletId,
          );
        } catch (_) {
          // Ignore target address fallback errors.
        }
      }
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
    } catch (e) {
      if (mounted) {
        setState(() => _error = e.toString());
      }
    }
  }

  Future<void> _buildSweep() async {
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
      final addressIds = _selectedAddressIds.isNotEmpty
          ? _selectedAddressIds.toList()
          : null;
      final keyIds = addressIds == null ? [widget.keyId] : null;
      final pending = await FfiBridge.buildSweepTx(
        walletId: walletId,
        targetAddress: target,
        keyIds: keyIds,
        addressIds: addressIds,
      );
      setState(() {
        _pending = pending;
        _pendingKeyIds = keyIds;
        _pendingAddressIds = addressIds;
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
      final signed = await FfiBridge.signTxFiltered(
        walletId: walletId,
        pending: pending,
        keyIds: _pendingKeyIds,
        addressIds: _pendingAddressIds,
      );
      final txid = await FfiBridge.broadcastTx(signed);
      if (!mounted) return;
      await PDialog.show<void>(
        context: context,
        title: 'Sweep sent',
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Your sweep transaction has been broadcast.',
              style: PTypography.bodyMedium(),
            ),
            SizedBox(height: PSpacing.sm),
            Text(
              txid,
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ),
        actions: [const PDialogAction(label: 'Close')],
      );
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isSending = false);
      }
    }
  }

  BigInt _selectedSpendable() {
    if (_selectedAddressIds.isNotEmpty) {
      var total = BigInt.zero;
      for (final addr in _addresses) {
        if (_selectedAddressIds.contains(addr.addressId)) {
          total += addr.spendable;
        }
      }
      return total;
    }
    var total = BigInt.zero;
    for (final addr in _addresses) {
      total += addr.spendable;
    }
    return total;
  }

  @override
  Widget build(BuildContext context) {
    final padding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    return PScaffold(
      appBar: const PAppBar(
        title: 'Sweep balance',
        subtitle: 'Send all funds to a chosen address',
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
                  _buildSweepSourceSelector(),
                  SizedBox(height: PSpacing.md),
                  PInput(
                    controller: _addressController,
                    label: 'Target address',
                    hint: 'Any address (external or wallet)',
                    maxLines: 2,
                  ),
                  SizedBox(height: PSpacing.sm),
                  PCard(
                    backgroundColor: AppColors.warningBackground,
                    child: Padding(
                      padding: EdgeInsets.all(PSpacing.sm),
                      child: Row(
                        children: [
                          Icon(
                            Icons.warning_amber_rounded,
                            color: AppColors.warning,
                            size: 18,
                          ),
                          SizedBox(width: PSpacing.sm),
                          Expanded(
                            child: Text(
                              'Sweep can send funds to addresses not owned by this wallet.',
                              style: PTypography.bodySmall(
                                color: AppColors.warning,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  SizedBox(height: PSpacing.sm),
                  PButton(
                    onPressed: _isBuilding ? null : _generateAddress,
                    variant: PButtonVariant.secondary,
                    child: const Text('Generate new address'),
                  ),
                  SizedBox(height: PSpacing.md),
                  PButton(
                    onPressed: _isBuilding ? null : _buildSweep,
                    variant: PButtonVariant.primary,
                    child: Text(_isBuilding ? 'Building...' : 'Preview sweep'),
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
          'Spendable ${_formatArrr(_selectedSpendable())}',
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
            child: const Text('Orchard'),
          ),
        ),
        SizedBox(width: PSpacing.sm),
        Expanded(
          child: PButton(
            onPressed: _useOrchard ? () => _setPool(false) : null,
            variant: _useOrchard
                ? PButtonVariant.secondary
                : PButtonVariant.primary,
            child: const Text('Sapling'),
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

  String get _sweepFromLabel {
    if (_selectedAddressIds.isEmpty) {
      return 'All addresses';
    }
    final count = _selectedAddressIds.length;
    final balance = _formatArrr(_selectedSpendable());
    return 'Addresses ($count) - $balance';
  }

  Future<void> _openSweepSourceSelector() async {
    if (_addresses.isEmpty) return;
    final pendingIds = Set<int>.from(_selectedAddressIds);

    await PBottomSheet.showAdaptive<void>(
      context: context,
      backgroundColor: AppColors.backgroundBase,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
      ),
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setModalState) {
            void toggleAddress(AddressBalanceInfo address) {
              setModalState(() {
                if (!pendingIds.remove(address.addressId)) {
                  pendingIds.add(address.addressId);
                }
              });
            }

            void commitSelection() {
              setState(() {
                _selectedAddressIds = Set<int>.from(pendingIds);
                _pending = null;
                _pendingKeyIds = null;
                _pendingAddressIds = null;
              });
            }

            final gutter = PSpacing.responsiveGutter(
              MediaQuery.of(context).size.width,
            );

            return SafeArea(
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  gutter,
                  PSpacing.lg,
                  gutter,
                  PSpacing.lg,
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Sweep from', style: PTypography.heading3()),
                    const SizedBox(height: PSpacing.md),
                    Flexible(
                      child: ListView.builder(
                        itemCount: () {
                          var count = 1;
                          if (_addresses.isNotEmpty) {
                            count += 3 + _addresses.length;
                          }
                          return count;
                        }(),
                        itemBuilder: (context, index) {
                          var cursor = 0;
                          if (index == cursor) {
                            return _buildSweepOption(
                              title: 'All addresses',
                              subtitle: 'Sweep every spendable note.',
                              selected: pendingIds.isEmpty,
                              onTap: () => setModalState(pendingIds.clear),
                            );
                          }
                          cursor += 1;

                          if (_addresses.isNotEmpty) {
                            if (index == cursor) {
                              return const SizedBox(height: PSpacing.md);
                            }
                            cursor += 1;
                            if (index == cursor) {
                              return Text(
                                'Addresses',
                                style: PTypography.labelMedium(),
                              );
                            }
                            cursor += 1;
                            if (index == cursor) {
                              return const SizedBox(height: PSpacing.xs);
                            }
                            cursor += 1;
                            final addressIndex = index - cursor;
                            if (addressIndex >= 0 &&
                                addressIndex < _addresses.length) {
                              final address = _addresses[addressIndex];
                              final name =
                                  address.label ?? _truncate(address.address);
                              final balance = _formatArrr(address.spendable);
                              final selected = pendingIds.contains(
                                address.addressId,
                              );
                              return _buildMultiSweepOption(
                                title: name,
                                subtitle:
                                    '$balance - ${_truncate(address.address)}',
                                selected: selected,
                                onTap: () => toggleAddress(address),
                              );
                            }
                          }

                          return const SizedBox.shrink();
                        },
                      ),
                    ),
                    const SizedBox(height: PSpacing.md),
                    PButton(
                      onPressed: () {
                        commitSelection();
                        Navigator.of(context).pop();
                      },
                      variant: PButtonVariant.primary,
                      child: const Text('Done'),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  }

  Widget _buildSweepSourceSelector() {
    if (_addresses.isEmpty) {
      return Text(
        'No addresses available for this key.',
        style: PTypography.bodySmall(color: AppColors.textSecondary),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Sweep from', style: PTypography.labelMedium()),
        SizedBox(height: PSpacing.xs),
        PCard(
          onTap: _openSweepSourceSelector,
          backgroundColor: AppColors.surfaceElevated,
          child: Padding(
            padding: EdgeInsets.all(PSpacing.md),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(_sweepFromLabel, style: PTypography.bodyMedium()),
                      SizedBox(height: 2),
                      Text(
                        'Tap to choose addresses',
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ),
                ),
                Icon(Icons.expand_more, color: AppColors.textSecondary),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildSweepOption({
    required String title,
    required String subtitle,
    required bool selected,
    required VoidCallback onTap,
  }) {
    return PCard(
      onTap: onTap,
      backgroundColor: selected
          ? AppColors.selectedBackground
          : AppColors.backgroundSurface,
      child: Padding(
        padding: EdgeInsets.all(PSpacing.md),
        child: Row(
          children: [
            Icon(
              selected
                  ? Icons.check_circle
                  : Icons.account_balance_wallet_outlined,
              color: selected
                  ? AppColors.accentPrimary
                  : AppColors.textSecondary,
              size: 20,
            ),
            SizedBox(width: PSpacing.sm),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: PTypography.bodyMedium()),
                  SizedBox(height: 2),
                  Text(
                    subtitle,
                    style: PTypography.bodySmall(
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

  Widget _buildMultiSweepOption({
    required String title,
    required String subtitle,
    required bool selected,
    required VoidCallback onTap,
  }) {
    return PCard(
      onTap: onTap,
      backgroundColor: selected
          ? AppColors.selectedBackground
          : AppColors.backgroundSurface,
      child: Padding(
        padding: EdgeInsets.all(PSpacing.md),
        child: Row(
          children: [
            Icon(
              selected ? Icons.check_box : Icons.check_box_outline_blank,
              color: selected
                  ? AppColors.accentPrimary
                  : AppColors.textSecondary,
              size: 20,
            ),
            SizedBox(width: PSpacing.sm),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: PTypography.bodyMedium()),
                  SizedBox(height: 2),
                  Text(
                    subtitle,
                    style: PTypography.bodySmall(
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
          Text('Preview', style: PTypography.heading4()),
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
    if (address.length <= 18) return address;
    return '${address.substring(0, 10)}...${address.substring(address.length - 6)}';
  }
}
