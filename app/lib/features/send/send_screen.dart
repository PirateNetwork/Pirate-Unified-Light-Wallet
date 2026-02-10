/// Send screen - Send-to-Many ARRR with per-output memos
library;

import 'dart:async';
import 'dart:ui' as ui;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:zxing2/qrcode.dart';

import '../../design/deep_space_theme.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/molecules/watch_only_banner.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/primary_action_dock.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/errors/transaction_errors.dart';

/// Maximum memo length in bytes
const int kMaxMemoBytes = 512;

/// Maximum recipients per transaction
const int kMaxRecipients = 50;

/// Additional fee per extra output in arrrtoshis.
const int kAdditionalOutputFeeArrrtoshis = 5000;

class PiratePaymentRequest {
  final String address;
  final String? amount;
  final String? memo;

  const PiratePaymentRequest({required this.address, this.amount, this.memo});
}

PiratePaymentRequest? _parsePiratePaymentRequest(String input) {
  final raw = input.trim();
  if (raw.isEmpty) return null;
  final lower = raw.toLowerCase();
  if (!lower.startsWith('pirate:')) return null;

  var normalized = raw;
  if (lower.startsWith('pirate://')) {
    normalized = 'pirate:${raw.substring('pirate://'.length)}';
  }

  final schemeIndex = normalized.indexOf(':');
  final payload = normalized.substring(schemeIndex + 1);
  if (payload.isEmpty) return null;

  final parts = payload.split('?');
  var address = parts.first;
  if (address.endsWith('/')) {
    address = address.substring(0, address.length - 1);
  }
  if (address.isEmpty) return null;

  String? amount;
  String? memo;
  if (parts.length > 1) {
    final query = parts.sublist(1).join('?');
    if (query.isNotEmpty) {
      try {
        final params = Uri.splitQueryString(query);
        amount = _sanitizeRequestAmount(params['amount']);
        memo = params['memo'];
        if ((memo == null || memo.isEmpty) && params['message'] != null) {
          memo = params['message'];
        }
      } catch (_) {
        // Ignore malformed query strings.
      }
    }
  }

  return PiratePaymentRequest(address: address, amount: amount, memo: memo);
}

String? _sanitizeRequestAmount(String? value) {
  if (value == null) return null;
  final trimmed = value.trim();
  if (trimmed.isEmpty) return null;
  final normalized = trimmed.replaceAll(',', '');
  if (!RegExp(r'^\d+(\.\d{0,8})?$').hasMatch(normalized)) return null;
  return normalized;
}

/// Output entry for send-to-many
class OutputEntry {
  String address;
  String amount;
  String memo;
  bool isValid;
  String? error;
  final TextEditingController addressController;
  final TextEditingController amountController;
  final TextEditingController memoController;

  OutputEntry()
    : address = '',
      amount = '',
      memo = '',
      isValid = false,
      error = null,
      addressController = TextEditingController(),
      amountController = TextEditingController(),
      memoController = TextEditingController();

  /// Get amount in arrrtoshis
  int get arrrtoshis {
    final value = double.tryParse(amount) ?? 0;
    return (value * 100000000).round();
  }

  /// Get memo byte length
  int get memoByteLength => memo.codeUnits.length;

  /// Check if memo is near limit
  bool get isMemoNearLimit => memoByteLength > 400;

  /// Sync from controllers
  void syncFromControllers() {
    address = addressController.text;
    amount = amountController.text;
    memo = memoController.text;
  }

  /// Dispose controllers
  void dispose() {
    addressController.dispose();
    amountController.dispose();
    memoController.dispose();
  }
}

/// Send flow steps
enum SendStep {
  recipients, // Multi-output recipient list
  review,
  sending,
  complete,
  error,
}

enum FeePreset { low, standard, high, custom }

extension FeePresetLabel on FeePreset {
  String get label {
    switch (this) {
      case FeePreset.low:
        return 'Low';
      case FeePreset.standard:
        return 'Standard';
      case FeePreset.high:
        return 'High';
      case FeePreset.custom:
        return 'Custom';
    }
  }
}

/// Send screen with multi-output support
class SendScreen extends ConsumerStatefulWidget {
  const SendScreen({super.key});

  @override
  ConsumerState<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends ConsumerState<SendScreen> {
  SendStep _currentStep = SendStep.recipients;

  // Multiple outputs for send-to-many
  final List<OutputEntry> _outputs = [OutputEntry()];
  bool _isApplyingPaymentRequest = false;

  // Fee information
  double _calculatedFee = 0.0001; // Will be recalculated on init
  int _selectedFeeArrrtoshis = 10000;
  int _defaultFeeArrrtoshis = 10000;
  int _baseMinFeeArrrtoshis = 10000;
  int _minFeeArrrtoshis = 10000;
  int _maxFeeArrrtoshis = 1000000;
  FeePreset _feePreset = FeePreset.standard;
  int? _customFeeArrrtoshis;
  double _totalAmount = 0;
  double _change = 0;
  bool _isValidating = false;
  bool _isSending = false;
  String? _errorMessage;
  String? _txId;
  PendingTx? _pendingTx;

  // Sending progress stages
  String _sendingStage = 'Building transaction...';

  // Watch-only check
  bool _isWatchOnly = false;
  double? _cachedBalance;

  WalletId? _walletId;
  bool _isLoadingSpendSources = false;
  List<KeyGroupInfo> _spendableKeys = [];
  List<KeyGroupInfo> _selectableKeys = [];
  List<AddressBalanceInfo> _addressBalances = [];
  KeyGroupInfo? _selectedKey;
  List<AddressBalanceInfo> _selectedAddresses = [];
  List<int>? _pendingKeyIds;
  List<int>? _pendingAddressIds;
  bool _autoConsolidationPromptShown = false;

  @override
  void initState() {
    super.initState();
    _checkWatchOnlyStatus();
    _updateFeePreview();
    _loadFeeInfo();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final walletId = ref.read(activeWalletProvider);
    if (_walletId != walletId) {
      _walletId = walletId;
      _autoConsolidationPromptShown = false;
      _loadSpendSources();
    }
  }

  @override
  void dispose() {
    for (final output in _outputs) {
      output.dispose();
    }
    super.dispose();
  }

  /// Check if current wallet is watch-only
  Future<void> _checkWatchOnlyStatus() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;

    try {
      final capabilities = await FfiBridge.getWatchOnlyCapabilities(walletId);
      setState(() {
        _isWatchOnly = capabilities.isWatchOnly;
      });
    } catch (_) {
      // Ignore errors
    }
  }

  Future<void> _loadFeeInfo() async {
    try {
      final info = await FfiBridge.getFeeInfo();
      if (!mounted) return;
      final minFee = info.minFee.toInt();
      final maxFee = info.maxFee.toInt();
      final baseFee = info.defaultFee.toInt();

      setState(() {
        _defaultFeeArrrtoshis = baseFee;
        _baseMinFeeArrrtoshis = minFee;
        _maxFeeArrrtoshis = maxFee;
        _updateFeePreview();
      });
    } catch (_) {
      // Ignore fee info errors.
    }
  }

  Future<void> _loadSpendSources() async {
    final walletId = _walletId;
    if (walletId == null) return;
    setState(() => _isLoadingSpendSources = true);
    try {
      final keys = await FfiBridge.listKeyGroups(walletId);
      final spendableKeys = keys.where((k) => k.spendable).toList();
      final addressBalances = await FfiBridge.listAddressBalances(walletId);
      final spendableKeyIds = spendableKeys.map((k) => k.id).toSet();
      final filteredAddresses =
          addressBalances
              .where(
                (addr) =>
                    addr.keyId != null && spendableKeyIds.contains(addr.keyId),
              )
              .toList()
            ..sort((a, b) => b.spendable.compareTo(a.spendable));

      final spendableByKey = <int, BigInt>{};
      for (final address in filteredAddresses) {
        final keyId = address.keyId;
        if (keyId == null) continue;
        spendableByKey[keyId] =
            (spendableByKey[keyId] ?? BigInt.zero) + address.spendable;
      }
      spendableKeys.sort(
        (a, b) => (spendableByKey[b.id] ?? BigInt.zero).compareTo(
          spendableByKey[a.id] ?? BigInt.zero,
        ),
      );
      final selectableKeys = spendableKeys
          .where((k) => k.keyType != KeyTypeInfo.seed)
          .toList();

      setState(() {
        _spendableKeys = spendableKeys;
        _selectableKeys = selectableKeys;
        _addressBalances = filteredAddresses;
      });

      if (_selectedKey != null &&
          !_selectableKeys.any((key) => key.id == _selectedKey!.id)) {
        _selectedKey = null;
      }
      if (_selectedAddresses.isNotEmpty) {
        final selectedIds = _selectedAddresses
            .map((addr) => addr.addressId)
            .toSet();
        _selectedAddresses = _addressBalances
            .where((addr) => selectedIds.contains(addr.addressId))
            .toList();
      }
    } catch (_) {
      // Ignore spend source load errors
    } finally {
      if (mounted) {
        setState(() => _isLoadingSpendSources = false);
      }
    }
    await _maybeShowAutoConsolidationPrompt();
  }

  Future<void> _maybeShowAutoConsolidationPrompt() async {
    if (_autoConsolidationPromptShown || _isWatchOnly) return;
    final walletId = _walletId;
    if (walletId == null || _spendableKeys.isEmpty) return;

    try {
      final enabled = await FfiBridge.getAutoConsolidationEnabled(walletId);
      if (enabled) return;
      final threshold = await FfiBridge.getAutoConsolidationThreshold();
      final candidateCount = await FfiBridge.getAutoConsolidationCandidateCount(
        walletId,
      );
      if (candidateCount < threshold) return;
      if (!mounted) return;

      _autoConsolidationPromptShown = true;
      final enable = await PDialog.show<bool>(
        context: context,
        title: 'Enable auto consolidation?',
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'You have $candidateCount untagged notes. Auto consolidation can combine them during sends to keep the wallet fast.',
              style: AppTypography.body,
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              'Only unlabeled and untagged addresses are included.',
              style: AppTypography.bodySmall.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
          ],
        ),
        actions: [
          PDialogAction(
            label: 'Not now',
            variant: PButtonVariant.secondary,
            result: false,
          ),
          PDialogAction(
            label: 'Enable',
            variant: PButtonVariant.primary,
            result: true,
          ),
        ],
      );

      if ((enable ?? false) && mounted) {
        await FfiBridge.setAutoConsolidationEnabled(walletId, true);
        ref.invalidate(autoConsolidationEnabledProvider);
      }
    } catch (_) {
      // Ignore prompt errors.
    }
  }

  /// Get available balance from provider
  double get _availableBalance {
    final balanceAsync = ref.watch(balanceStreamProvider);
    return balanceAsync.when(
      data: (balance) {
        if (balance?.spendable == null) return 0.0;
        final spendable = balance!.spendable;
        final value = spendable.toDouble() / 100000000.0;
        _cachedBalance = value;
        return value;
      },
      loading: () => _cachedBalance ?? 0.0,
      error: (_, _) => _cachedBalance ?? 0.0,
    );
  }

  /// Get pending balance from provider
  double get _pendingBalance {
    final balanceAsync = ref.watch(balanceStreamProvider);
    return balanceAsync.when(
      data: (balance) {
        if (balance?.pending == null) return 0.0;
        final pending = balance!.pending;
        return pending.toDouble() / 100000000.0;
      },
      loading: () => 0.0,
      error: (_, _) => 0.0,
    );
  }

  double get _availableBalanceForSelection {
    final overall = _availableBalance;
    if (_selectedAddresses.isNotEmpty) {
      return _toArrr(_selectedAddressSpendable());
    }
    if (_selectedKey != null) {
      final spendable = _spendableForKey(_selectedKey!.id);
      return _toArrr(spendable);
    }
    return overall;
  }

  double get _pendingBalanceForSelection {
    final overall = _pendingBalance;
    if (_selectedAddresses.isNotEmpty) {
      return _toArrr(_selectedAddressPending());
    }
    if (_selectedKey != null) {
      final pending = _pendingForKey(_selectedKey!.id);
      return _toArrr(pending);
    }
    return overall;
  }

  BigInt _spendableForKey(int keyId) {
    var total = BigInt.zero;
    for (final addr in _addressBalances) {
      if (addr.keyId == keyId) {
        total += addr.spendable;
      }
    }
    return total;
  }

  BigInt _pendingForKey(int keyId) {
    var total = BigInt.zero;
    for (final addr in _addressBalances) {
      if (addr.keyId == keyId) {
        total += addr.pending;
      }
    }
    return total;
  }

  String get _spendFromLabel {
    if (_selectedAddresses.isNotEmpty) {
      final count = _selectedAddresses.length;
      final balance = _formatArrr(_selectedAddressSpendable());
      return 'Addresses ($count) - $balance';
    }
    if (_selectedKey != null) {
      final label = _displayKeyLabel(_selectedKey!);
      final balance = _formatArrr(_spendableForKey(_selectedKey!.id));
      return '$label - $balance';
    }
    return 'Auto (all keys)';
  }

  Future<void> _openSpendFromSelector() async {
    final loadFuture = _isLoadingSpendSources ? null : _loadSpendSources();
    if (!mounted) return;

    KeyGroupInfo? pendingKey = _selectedKey;
    final pendingAddressIds = _selectedAddresses
        .map((addr) => addr.addressId)
        .toSet();

    await showModalBottomSheet<void>(
      context: context,
      backgroundColor: AppColors.backgroundBase,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
      ),
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setModalState) {
            final screenWidth = MediaQuery.of(context).size.width;
            final gutter = PSpacing.responsiveGutter(screenWidth);
            void commitSelection() {
              final selectedAddresses = _addressBalances
                  .where((addr) => pendingAddressIds.contains(addr.addressId))
                  .toList();
              setState(() {
                _selectedKey = pendingKey;
                _selectedAddresses = selectedAddresses;
                _pendingTx = null;
                _pendingKeyIds = null;
                _pendingAddressIds = null;
              });
            }

            void toggleAddress(AddressBalanceInfo address) {
              setModalState(() {
                pendingKey = null;
                final id = address.addressId;
                if (!pendingAddressIds.remove(id)) {
                  pendingAddressIds.add(id);
                }
              });
            }

            void selectAuto() {
              setState(() {
                _selectedKey = null;
                _selectedAddresses = [];
                _pendingTx = null;
                _pendingKeyIds = null;
                _pendingAddressIds = null;
              });
              Navigator.of(context).pop();
            }

            void selectKey(KeyGroupInfo key) {
              setState(() {
                _selectedKey = key;
                _selectedAddresses = [];
                _pendingTx = null;
                _pendingKeyIds = null;
                _pendingAddressIds = null;
              });
              Navigator.of(context).pop();
            }

            return SafeArea(
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  gutter,
                  AppSpacing.lg,
                  gutter,
                  AppSpacing.lg,
                ),
                child: FutureBuilder<void>(
                  future: loadFuture,
                  builder: (context, snapshot) {
                    final isRefreshing =
                        snapshot.connectionState == ConnectionState.waiting;
                    final showLoading =
                        isRefreshing &&
                        _spendableKeys.isEmpty &&
                        _addressBalances.isEmpty;

                    return Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Spend from', style: AppTypography.h4),
                        const SizedBox(height: AppSpacing.md),
                        if (isRefreshing)
                          const LinearProgressIndicator(minHeight: 2),
                        if (isRefreshing) const SizedBox(height: AppSpacing.md),
                        if (showLoading)
                          const Padding(
                            padding: EdgeInsets.symmetric(
                              vertical: AppSpacing.lg,
                            ),
                            child: Center(child: CircularProgressIndicator()),
                          )
                        else
                          Flexible(
                            child: ListView.builder(
                              itemCount: () {
                                final keyCount = _selectableKeys.length;
                                final addressCount = _addressBalances.length;
                                var count = 1;
                                if (keyCount > 0) {
                                  count += 3 + keyCount;
                                }
                                if (addressCount > 0) {
                                  count += 3 + addressCount;
                                }
                                return count;
                              }(),
                              itemBuilder: (context, index) {
                                var cursor = 0;
                                if (index == cursor) {
                                  return _buildSpendOption(
                                    context,
                                    title: 'Auto (all keys)',
                                    subtitle:
                                        'Let the wallet choose notes automatically.',
                                    selected:
                                        pendingKey == null &&
                                        pendingAddressIds.isEmpty,
                                    onTap: selectAuto,
                                  );
                                }
                                cursor += 1;

                                if (_selectableKeys.isNotEmpty) {
                                  if (index == cursor) {
                                    return const SizedBox(
                                      height: AppSpacing.md,
                                    );
                                  }
                                  cursor += 1;
                                  if (index == cursor) {
                                    return Text(
                                      'Keys',
                                      style: AppTypography.labelMedium,
                                    );
                                  }
                                  cursor += 1;
                                  if (index == cursor) {
                                    return const SizedBox(
                                      height: AppSpacing.xs,
                                    );
                                  }
                                  cursor += 1;
                                  final keyIndex = index - cursor;
                                  if (keyIndex >= 0 &&
                                      keyIndex < _selectableKeys.length) {
                                    final key = _selectableKeys[keyIndex];
                                    final balance = _formatArrr(
                                      _spendableForKey(key.id),
                                    );
                                    return _buildSpendOption(
                                      context,
                                      title: _displayKeyLabel(key),
                                      subtitle: 'Spendable $balance',
                                      selected:
                                          pendingKey?.id == key.id &&
                                          pendingAddressIds.isEmpty,
                                      onTap: () => selectKey(key),
                                    );
                                  }
                                  cursor += _selectableKeys.length;
                                }

                                if (_addressBalances.isNotEmpty) {
                                  if (index == cursor) {
                                    return const SizedBox(
                                      height: AppSpacing.md,
                                    );
                                  }
                                  cursor += 1;
                                  if (index == cursor) {
                                    return Text(
                                      'Addresses',
                                      style: AppTypography.labelMedium,
                                    );
                                  }
                                  cursor += 1;
                                  if (index == cursor) {
                                    return const SizedBox(
                                      height: AppSpacing.xs,
                                    );
                                  }
                                  cursor += 1;
                                  final addressIndex = index - cursor;
                                  if (addressIndex >= 0 &&
                                      addressIndex < _addressBalances.length) {
                                    final address =
                                        _addressBalances[addressIndex];
                                    final name =
                                        address.label ??
                                        _truncateAddress(address.address);
                                    final balance = _formatArrr(
                                      address.spendable,
                                    );
                                    final selected = pendingAddressIds.contains(
                                      address.addressId,
                                    );
                                    return _buildMultiSpendOption(
                                      context,
                                      title: name,
                                      subtitle:
                                          '$balance - ${_truncateAddress(address.address)}',
                                      selected: selected,
                                      onTap: () => toggleAddress(address),
                                    );
                                  }
                                }

                                return const SizedBox.shrink();
                              },
                            ),
                          ),
                        const SizedBox(height: AppSpacing.md),
                        PButton(
                          onPressed: () {
                            commitSelection();
                            Navigator.of(context).pop();
                          },
                          variant: PButtonVariant.primary,
                          child: const Text('Done'),
                        ),
                      ],
                    );
                  },
                ),
              ),
            );
          },
        );
      },
    );
  }

  Future<void> _openFeeSelector() async {
    final inputFormatter = TextInputFormatter.withFunction((
      oldValue,
      newValue,
    ) {
      final text = newValue.text;
      if (text.isEmpty) return newValue;
      if (!RegExp(r'^\d+(\.\d{0,8})?$').hasMatch(text)) {
        return oldValue;
      }
      return newValue;
    });

    var pendingPreset = _feePreset;
    var pendingFee = _selectedFeeArrrtoshis;
    String? feeError;
    final controller = TextEditingController(
      text: _feeArrrFromArrrtoshis(pendingFee).toStringAsFixed(8),
    );

    void syncController() {
      final text = _feeArrrFromArrrtoshis(pendingFee).toStringAsFixed(8);
      controller.value = controller.value.copyWith(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
      );
    }

    void setPendingFee(int fee) {
      pendingFee = _clampFee(fee);
      feeError = null;
      syncController();
    }

    void applyPreset(FeePreset preset, StateSetter setModalState) {
      pendingPreset = preset;
      if (preset != FeePreset.custom) {
        setPendingFee(_feeForPreset(preset, _currentBaseFeeArrrtoshis()));
      } else if (_customFeeArrrtoshis != null) {
        setPendingFee(_customFeeArrrtoshis!);
      }
      setModalState(() {});
    }

    try {
      await PBottomSheet.show<void>(
        context: context,
        title: 'Network fee',
        content: StatefulBuilder(
          builder: (context, setModalState) {
            void onSliderChanged(double value) {
              pendingPreset = FeePreset.custom;
              setPendingFee(value.round());
              setModalState(() {});
            }

            void onCustomChanged(String value) {
              pendingPreset = FeePreset.custom;
              final parsed = double.tryParse(value);
              if (parsed == null) {
                feeError = 'Enter a valid fee amount.';
                setModalState(() {});
                return;
              }
              final feeArrrtoshis = (parsed * 100000000).round();
              if (feeArrrtoshis < _minFeeArrrtoshis ||
                  feeArrrtoshis > _maxFeeArrrtoshis) {
                feeError =
                    'Fee must be between ${_formatFeeArrrtoshis(_minFeeArrrtoshis)} and ${_formatFeeArrrtoshis(_maxFeeArrrtoshis)}.';
                setModalState(() {});
                return;
              }
              pendingFee = feeArrrtoshis;
              feeError = null;
              setModalState(() {});
            }

            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Choose a fee speed', style: AppTypography.bodyMedium),
                const SizedBox(height: AppSpacing.sm),
                Wrap(
                  spacing: AppSpacing.xs,
                  runSpacing: AppSpacing.xs,
                  children: FeePreset.values.map((preset) {
                    return _FeePresetChip(
                      label: preset.label,
                      isSelected: pendingPreset == preset,
                      onTap: () => applyPreset(preset, setModalState),
                    );
                  }).toList(),
                ),
                const SizedBox(height: AppSpacing.md),
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        'Selected',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppTypography.caption,
                      ),
                    ),
                    const SizedBox(width: AppSpacing.sm),
                    Text(
                      _formatFeeArrrtoshis(pendingFee),
                      style: AppTypography.mono.copyWith(fontSize: 12),
                    ),
                  ],
                ),
                Slider(
                  value: pendingFee.toDouble(),
                  min: _minFeeArrrtoshis.toDouble(),
                  max: _maxFeeArrrtoshis.toDouble(),
                  divisions: 100,
                  onChanged: onSliderChanged,
                  activeColor: AppColors.accentPrimary,
                  inactiveColor: AppColors.borderSubtle,
                ),
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        'Low',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppTypography.caption,
                      ),
                    ),
                    Text('High', style: AppTypography.caption),
                  ],
                ),
                const SizedBox(height: AppSpacing.md),
                PInput(
                  label: 'Custom fee (ARRR)',
                  controller: controller,
                  hint: _feeArrrFromArrrtoshis(
                    _minFeeArrrtoshis,
                  ).toStringAsFixed(8),
                  errorText: feeError,
                  keyboardType: const TextInputType.numberWithOptions(
                    decimal: true,
                  ),
                  inputFormatters: [inputFormatter],
                  onChanged: onCustomChanged,
                ),
                const SizedBox(height: AppSpacing.xs),
                Text(
                  'Min ${_formatFeeArrrtoshis(_minFeeArrrtoshis)} - Max ${_formatFeeArrrtoshis(_maxFeeArrrtoshis)}',
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
                const SizedBox(height: AppSpacing.sm),
                Text(
                  'Pirate uses a fixed minimum fee. Higher fees may not speed up confirmations.',
                  style: AppTypography.bodySmall.copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
                const SizedBox(height: AppSpacing.lg),
                Row(
                  children: [
                    Expanded(
                      child: PButton(
                        text: 'Cancel',
                        variant: PButtonVariant.secondary,
                        onPressed: () => Navigator.of(context).pop(),
                      ),
                    ),
                    const SizedBox(width: AppSpacing.sm),
                    Expanded(
                      child: PButton(
                        text: 'Apply',
                        onPressed: () {
                          if (pendingPreset == FeePreset.custom) {
                            final parsed = double.tryParse(
                              controller.text.trim(),
                            );
                            if (parsed == null) {
                              setModalState(() {
                                feeError = 'Enter a valid fee amount.';
                              });
                              return;
                            }
                            final feeArrrtoshis = (parsed * 100000000).round();
                            if (feeArrrtoshis < _minFeeArrrtoshis ||
                                feeArrrtoshis > _maxFeeArrrtoshis) {
                              setModalState(() {
                                feeError =
                                    'Fee must be between ${_formatFeeArrrtoshis(_minFeeArrrtoshis)} and ${_formatFeeArrrtoshis(_maxFeeArrrtoshis)}.';
                              });
                              return;
                            }
                            pendingFee = feeArrrtoshis;
                            _customFeeArrrtoshis = feeArrrtoshis;
                          }

                          setState(() {
                            _feePreset = pendingPreset;
                            _selectedFeeArrrtoshis = pendingFee;
                            _pendingTx = null;
                            _pendingKeyIds = null;
                            _pendingAddressIds = null;
                            _updateFeePreview();
                          });
                          Navigator.of(context).pop();
                        },
                      ),
                    ),
                  ],
                ),
              ],
            );
          },
        ),
      );
    } finally {
      controller.dispose();
    }
  }

  Widget _buildSpendOption(
    BuildContext context, {
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
        padding: const EdgeInsets.all(AppSpacing.md),
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
            const SizedBox(width: AppSpacing.sm),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: AppTypography.bodyMedium),
                  const SizedBox(height: 2),
                  Text(
                    subtitle,
                    style: AppTypography.bodySmall.copyWith(
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

  Widget _buildMultiSpendOption(
    BuildContext context, {
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
        padding: const EdgeInsets.all(AppSpacing.md),
        child: Row(
          children: [
            Icon(
              selected ? Icons.check_box : Icons.check_box_outline_blank,
              color: selected
                  ? AppColors.accentPrimary
                  : AppColors.textSecondary,
              size: 20,
            ),
            const SizedBox(width: AppSpacing.sm),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: AppTypography.bodyMedium),
                  const SizedBox(height: 2),
                  Text(
                    subtitle,
                    style: AppTypography.bodySmall.copyWith(
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

  String _formatArrr(BigInt value) {
    final amount = value.toDouble() / 100000000.0;
    return '${amount.toStringAsFixed(8)} ARRR';
  }

  double _toArrr(BigInt value) {
    return value.toDouble() / 100000000.0;
  }

  double _feeArrrFromArrrtoshis(int feeArrrtoshis) {
    return feeArrrtoshis / 100000000.0;
  }

  String _formatFeeArrrtoshis(int feeArrrtoshis) {
    return '${_feeArrrFromArrrtoshis(feeArrrtoshis).toStringAsFixed(8)} ARRR';
  }

  int _feeForPreset(FeePreset preset, int baseFee) {
    switch (preset) {
      case FeePreset.low:
        return (baseFee * 0.5).round();
      case FeePreset.standard:
        return baseFee;
      case FeePreset.high:
        return baseFee * 2;
      case FeePreset.custom:
        return _customFeeArrrtoshis ?? baseFee;
    }
  }

  int _clampFee(int fee, {int? minFee, int? maxFee}) {
    final minValue = minFee ?? _minFeeArrrtoshis;
    final maxValue = maxFee ?? _maxFeeArrrtoshis;
    return fee.clamp(minValue, maxValue);
  }

  int _extraOutputCount() {
    if (_outputs.length <= 1) return 0;
    return _outputs.length - 1;
  }

  int _currentMinFeeArrrtoshis() {
    final fee =
        _baseMinFeeArrrtoshis +
        (_extraOutputCount() * kAdditionalOutputFeeArrrtoshis);
    return fee.clamp(_baseMinFeeArrrtoshis, _maxFeeArrrtoshis);
  }

  int _currentBaseFeeArrrtoshis() {
    final minFee = _currentMinFeeArrrtoshis();
    final fee =
        _defaultFeeArrrtoshis +
        (_extraOutputCount() * kAdditionalOutputFeeArrrtoshis);
    return fee.clamp(minFee, _maxFeeArrrtoshis);
  }

  String _truncateAddress(String address) {
    if (address.length <= 16) return address;
    return '${address.substring(0, 8)}...${address.substring(address.length - 6)}';
  }

  String _displayKeyLabel(KeyGroupInfo key) {
    if (key.keyType == KeyTypeInfo.seed) {
      final label = key.label?.trim();
      if (label == null || label.isEmpty || label == 'Seed') {
        return 'Default wallet keys';
      }
    }
    return key.label ?? _defaultKeyLabel(key);
  }

  String _defaultKeyLabel(KeyGroupInfo key) {
    switch (key.keyType) {
      case KeyTypeInfo.seed:
        return 'Default wallet keys';
      case KeyTypeInfo.importedSpending:
        return 'Imported spending key';
      case KeyTypeInfo.importedViewing:
        return 'Viewing key';
    }
  }

  BigInt _selectedAddressSpendable() {
    var total = BigInt.zero;
    for (final addr in _selectedAddresses) {
      total += addr.spendable;
    }
    return total;
  }

  BigInt _selectedAddressPending() {
    var total = BigInt.zero;
    for (final addr in _selectedAddresses) {
      total += addr.pending;
    }
    return total;
  }

  /// Add new output entry
  void _addOutput() {
    if (_outputs.length >= kMaxRecipients) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Maximum $kMaxRecipients recipients allowed')),
      );
      return;
    }
    setState(() {
      _outputs.add(OutputEntry());
      _updateFeePreview(); // Update fee when adding recipient
    });
  }

  bool get _supportsCameraScan {
    if (kIsWeb) return false;
    return defaultTargetPlatform == TargetPlatform.android ||
        defaultTargetPlatform == TargetPlatform.iOS;
  }

  bool get _supportsImageImport {
    if (kIsWeb) return false;
    return defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.linux;
  }

  void _handleOutputChanged(int index) {
    if (index < 0 || index >= _outputs.length) return;
    _applyPiratePaymentRequest(index);
    _outputs[index].syncFromControllers();
    setState(_updateFeePreview);
  }

  void _applyPiratePaymentRequest(int index) {
    if (_isApplyingPaymentRequest) return;
    final output = _outputs[index];
    final request = _parsePiratePaymentRequest(output.addressController.text);
    if (request == null) return;

    _isApplyingPaymentRequest = true;
    output.addressController.text = request.address;
    if (request.amount != null && request.amount!.isNotEmpty) {
      output.amountController.text = request.amount!;
    }
    if (request.memo != null && request.memo!.isNotEmpty) {
      output.memoController.text = request.memo!;
    }
    _isApplyingPaymentRequest = false;
  }

  Future<void> _openQrScanner(int index) async {
    if (!_supportsCameraScan) return;
    final result = await Navigator.of(context).push<String>(
      MaterialPageRoute(
        builder: (_) => const _SendQrScannerScreen(),
        fullscreenDialog: true,
      ),
    );

    if (!mounted || result == null || result.isEmpty) return;
    _outputs[index].addressController.text = result;
    _handleOutputChanged(index);
  }

  Future<void> _importQrImage(int index) async {
    if (!_supportsImageImport) return;
    final file = await openFile(
      acceptedTypeGroups: [
        const XTypeGroup(
          label: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'],
        ),
      ],
    );
    if (file == null) return;

    final bytes = await file.readAsBytes();
    final result = await _decodeQrFromImageBytes(bytes);
    bytes.fillRange(0, bytes.length, 0);

    if (!mounted) return;
    if (result == null || result.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('No QR code found in that image'),
          duration: Duration(seconds: 2),
        ),
      );
      return;
    }

    _outputs[index].addressController.text = result;
    _handleOutputChanged(index);
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('QR code imported'),
        duration: Duration(seconds: 2),
      ),
    );
  }

  Future<String?> _decodeQrFromImageBytes(Uint8List bytes) async {
    try {
      final uiImage = await _decodeUiImage(bytes);
      final byteData = await uiImage.toByteData(
        format: ui.ImageByteFormat.rawRgba,
      );
      if (byteData == null) {
        uiImage.dispose();
        return null;
      }

      final rgba = byteData.buffer.asUint8List(
        byteData.offsetInBytes,
        byteData.lengthInBytes,
      );
      final pixels = _rgbaToArgbPixels(rgba, uiImage.width, uiImage.height);
      uiImage.dispose();

      final source = RGBLuminanceSource(uiImage.width, uiImage.height, pixels);
      final bitmap = BinaryBitmap(HybridBinarizer(source));
      final reader = QRCodeReader();
      final result = reader.decode(bitmap);
      return result.text;
    } catch (_) {
      return null;
    }
  }

  Future<ui.Image> _decodeUiImage(Uint8List bytes) {
    final completer = Completer<ui.Image>();
    ui.decodeImageFromList(bytes, completer.complete);
    return completer.future;
  }

  Int32List _rgbaToArgbPixels(Uint8List rgbaBytes, int width, int height) {
    final pixels = Int32List(width * height);
    for (int i = 0; i < pixels.length; i++) {
      final offset = i * 4;
      final r = rgbaBytes[offset];
      final g = rgbaBytes[offset + 1];
      final b = rgbaBytes[offset + 2];
      final a = rgbaBytes[offset + 3];
      pixels[i] = (a << 24) | (r << 16) | (g << 8) | b;
    }
    return pixels;
  }

  /// Remove output entry
  void _removeOutput(int index) {
    if (_outputs.length <= 1) return;
    setState(() {
      _outputs[index].dispose();
      _outputs.removeAt(index);
      _updateFeePreview();
    });
  }

  /// Update fee preview using the selected fee.
  void _updateFeePreview() {
    final minFee = _currentMinFeeArrrtoshis();
    final baseFee = _currentBaseFeeArrrtoshis();
    int selectedFee = _feePreset == FeePreset.custom
        ? (_customFeeArrrtoshis ?? _selectedFeeArrrtoshis)
        : _feeForPreset(_feePreset, baseFee);
    selectedFee = selectedFee.clamp(minFee, _maxFeeArrrtoshis);
    if (_feePreset == FeePreset.custom) {
      _customFeeArrrtoshis = selectedFee;
    }

    double parseController(TextEditingController controller) {
      final raw = controller.text.trim();
      if (raw.isEmpty) return 0.0;
      final value = double.tryParse(raw);
      if (value == null || value.isNaN || value.isInfinite) return 0.0;
      return value <= 0 ? 0.0 : value;
    }

    _minFeeArrrtoshis = minFee;
    _selectedFeeArrrtoshis = selectedFee;
    _calculatedFee = _feeArrrFromArrrtoshis(selectedFee);
    _totalAmount = _outputs.fold(0.0, (sum, o) {
      return sum + parseController(o.amountController);
    });
  }

  /// Validate all outputs with detailed error mapping
  bool _validateOutputs() {
    bool allValid = true;
    _errorMessage = null;

    for (int i = 0; i < _outputs.length; i++) {
      final output = _outputs[i]
        ..syncFromControllers()
        ..error = null
        ..isValid = true;

      // Validate address using error mapper
      final addressError = TransactionErrorMapper.validateAddress(
        output.address,
      );
      if (addressError != null) {
        output
          ..error = addressError.message
          ..isValid = false;
        allValid = false;
        continue; // Skip further validation for this output
      }

      // Validate amount
      if (output.amount.isEmpty) {
        output
          ..error = 'Amount is required'
          ..isValid = false;
        allValid = false;
      } else {
        final value = double.tryParse(output.amount);
        if (value == null || value <= 0) {
          output
            ..error = 'Amount must be greater than zero'
            ..isValid = false;
          allValid = false;
        } else if (value < 0.00000001) {
          output
            ..error = 'Amount too small (minimum 0.00000001 ARRR)'
            ..isValid = false;
          allValid = false;
        }
      }

      // Validate memo using error mapper
      final memoError = TransactionErrorMapper.validateMemo(output.memo);
      if (memoError != null) {
        output
          ..error = memoError.message
          ..isValid = false;
        allValid = false;
      }
    }

    // Check total doesn't exceed balance
    _updateFeePreview();
    final total = _totalAmount + _calculatedFee;
    final available = _availableBalanceForSelection;
    final pending = _pendingBalanceForSelection;
    if (total > available) {
      if (available <= 0 && pending > 0) {
        _errorMessage =
            'Insufficient spendable funds: need ${total.toStringAsFixed(8)} ARRR, '
            'have ${available.toStringAsFixed(8)} ARRR. '
            '${pending.toStringAsFixed(8)} ARRR is pending and becomes spendable after 10 confirmations.';
      } else {
        _errorMessage =
            'Insufficient spendable funds: need ${total.toStringAsFixed(8)} ARRR, '
            'have ${available.toStringAsFixed(8)} ARRR';
      }
      allValid = false;
    }

    setState(() {});
    return allValid;
  }

  /// Proceed to review - builds transaction via FFI
  Future<void> _proceedToReview() async {
    // Check watch-only first
    if (_isWatchOnly) {
      await WatchOnlyWarningDialog.show(context);
      return;
    }

    setState(() => _isValidating = true);

    // Validate all outputs locally first
    final isValid = _validateOutputs();

    if (!isValid) {
      setState(() => _isValidating = false);
      return;
    }

    try {
      // Get active wallet
      final walletId = ref.read(activeWalletProvider);
      if (walletId == null) {
        throw StateError('No active wallet');
      }

      // Convert outputs to FFI format
      final ffiOutputs = _outputs
          .map(
            (o) => Output(
              addr: o.address,
              amount: BigInt.from(o.arrrtoshis),
              memo: o.memo.isNotEmpty ? o.memo : null,
            ),
          )
          .toList();

      final addressIds = _selectedAddresses.isNotEmpty
          ? _selectedAddresses.map((addr) => addr.addressId).toList()
          : null;
      final keyIds = addressIds != null
          ? null
          : (_selectedKey != null ? [_selectedKey!.id] : null);

      // Build transaction via FFI
      final pendingTx = await FfiBridge.buildTx(
        walletId: walletId,
        outputs: ffiOutputs,
        keyIds: keyIds,
        addressIds: addressIds,
        fee: _selectedFeeArrrtoshis,
      );

      // Store pending tx and update UI
      setState(() {
        _pendingTx = pendingTx;
        _pendingKeyIds = keyIds;
        _pendingAddressIds = addressIds;
        _selectedFeeArrrtoshis = pendingTx.fee.toInt();
        _calculatedFee = pendingTx.fee.toDouble() / 100000000.0;
        _totalAmount = pendingTx.totalAmount.toDouble() / 100000000.0;
        _change = pendingTx.change.toDouble() / 100000000.0;
        _isValidating = false;
        _currentStep = SendStep.review;
      });
    } catch (e) {
      // Map error to human-readable message
      final txError = TransactionErrorMapper.mapError(e);
      setState(() {
        _errorMessage = txError.displayMessage;
        _isValidating = false;
      });
    }
  }

  /// Send transaction - sign and broadcast via FFI
  Future<void> _sendTransaction() async {
    if (_pendingTx == null) {
      setState(() {
        _errorMessage = 'Transaction not built. Please try again.';
        _currentStep = SendStep.error;
      });
      return;
    }

    setState(() {
      _currentStep = SendStep.sending;
      _isSending = true;
      _sendingStage = 'Signing transaction...';
    });

    try {
      // Get active wallet
      final walletId = ref.read(activeWalletProvider);
      if (walletId == null) {
        throw StateError('No active wallet');
      }

      // Sign transaction via FFI
      setState(() => _sendingStage = 'Generating Cryptographic Proofs...');
      final signedTx = (_pendingKeyIds != null || _pendingAddressIds != null)
          ? await FfiBridge.signTxFiltered(
              walletId: walletId,
              pending: _pendingTx!,
              keyIds: _pendingKeyIds,
              addressIds: _pendingAddressIds,
            )
          : await FfiBridge.signTx(walletId, _pendingTx!);

      // Broadcast transaction via FFI
      setState(() => _sendingStage = 'Broadcasting to network...');
      final txId = await FfiBridge.broadcastTx(signedTx);

      // Success!
      _txId = txId;

      setState(() {
        _currentStep = SendStep.complete;
        _isSending = false;
      });

      // Refresh balance and transaction history
      ref
        ..invalidate(balanceProvider)
        ..invalidate(transactionsProvider);

      // Show success dialog
      if (mounted) {
        await _showSuccessDialog();
      }
    } catch (e) {
      // Map error to human-readable message
      final txError = TransactionErrorMapper.mapError(e);
      setState(() {
        _currentStep = SendStep.error;
        _errorMessage = txError.displayMessage;
        _isSending = false;
      });
    }
  }

  /// Show success dialog
  Future<void> _showSuccessDialog() async {
    final goHome = await PDialog.show<bool>(
      context: context,
      barrierDismissible: false,
      title: 'Sent',
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Your transaction is on its way.', style: AppTypography.body),
          if (_totalAmount > 0) ...[
            const SizedBox(height: AppSpacing.sm),
            Text(
              'Amount: ${_totalAmount.toStringAsFixed(8)} ARRR',
              style: AppTypography.bodySmall.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
          ],
          const SizedBox(height: AppSpacing.md),
          Text(
            'Transaction ID',
            style: AppTypography.caption.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
          const SizedBox(height: AppSpacing.xs),
          SelectableText(
            _txId ?? '',
            style: AppTypography.mono.copyWith(
              fontSize: 12,
              color: AppColors.accentPrimary,
            ),
          ),
        ],
      ),
      actions: [
        PDialogAction(
          label: 'Copy transaction ID',
          variant: PButtonVariant.secondary,
          onPressed: () {
            Clipboard.setData(ClipboardData(text: _txId ?? ''));
            ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(content: Text('Transaction ID copied')),
            );
          },
        ),
        PDialogAction(
          label: 'Done',
          variant: PButtonVariant.primary,
          result: true,
        ),
      ],
    );
    if ((goHome ?? false) && mounted) {
      context.go('/home');
    }
  }

  @override
  Widget build(BuildContext context) {
    final title = _currentStep == SendStep.review
        ? 'Review'
        : _currentStep == SendStep.sending
        ? 'Sending'
        : 'Send';
    final isMobile = PSpacing.isMobile(MediaQuery.of(context).size.width);

    return PScaffold(
      title: 'Send',
      appBar: PAppBar(
        title: title,
        subtitle: _isWatchOnly
            ? 'This is view only. Sending is off.'
            : 'Paste an address.',
        onBack: _isSending ? null : () => context.pop(),
        showBackButton: true,
        actions: [
          if (_currentStep == SendStep.recipients &&
              _outputs.length < kMaxRecipients)
            PIconButton(
              icon: const Icon(Icons.add_circle_outline),
              tooltip: 'Add recipient',
              onPressed: _addOutput,
            ),
          ConnectionStatusIndicator(
            full: !isMobile,
            onTap: () => context.push('/settings/privacy-shield'),
          ),
          if (!isMobile) const WalletSwitcherButton(compact: true),
        ],
      ),
      body: _buildStepContent(),
    );
  }

  Widget _buildStepContent() {
    switch (_currentStep) {
      case SendStep.recipients:
        return _RecipientsStep(
          outputs: _outputs,
          availableBalance: _availableBalanceForSelection,
          calculatedFee: _calculatedFee,
          totalAmount: _totalAmount,
          errorMessage: _errorMessage,
          isValidating: _isValidating,
          spendFromLabel: _spendFromLabel,
          feePreset: _feePreset,
          spendFromEnabled: !_isSending,
          onSelectSpendFrom: _openSpendFromSelector,
          onEditFee: _openFeeSelector,
          onRemoveOutput: _removeOutput,
          onAddOutput: _addOutput,
          onOutputChanged: _handleOutputChanged,
          onScan: _supportsCameraScan ? _openQrScanner : null,
          onImport: _supportsImageImport ? _importQrImage : null,
          onContinue: _proceedToReview,
        );

      case SendStep.review:
        return _ReviewStep(
          outputs: _outputs,
          fee: _calculatedFee,
          totalAmount: _totalAmount,
          change: _change,
          pendingTx: _pendingTx,
          spendFromLabel: _spendFromLabel,
          onConfirm: _sendTransaction,
          onEdit: () => setState(() => _currentStep = SendStep.recipients),
        );

      case SendStep.sending:
        return _SendingStep(stage: _sendingStage);

      case SendStep.complete:
        return const SizedBox(); // Handled by dialog

      case SendStep.error:
        return _ErrorStep(
          error: _errorMessage ?? 'Unknown error',
          onRetry: () => setState(() => _currentStep = SendStep.recipients),
        );
    }
  }
}

/// Recipients input step with add/remove
class _RecipientsStep extends StatelessWidget {
  final List<OutputEntry> outputs;
  final double availableBalance;
  final double calculatedFee;
  final double totalAmount;
  final String? errorMessage;
  final bool isValidating;
  final String spendFromLabel;
  final FeePreset feePreset;
  final bool spendFromEnabled;
  final VoidCallback onSelectSpendFrom;
  final VoidCallback onEditFee;
  final void Function(int) onRemoveOutput;
  final VoidCallback onAddOutput;
  final void Function(int) onOutputChanged;
  final void Function(int)? onScan;
  final void Function(int)? onImport;
  final VoidCallback onContinue;

  const _RecipientsStep({
    required this.outputs,
    required this.availableBalance,
    required this.calculatedFee,
    required this.totalAmount,
    required this.errorMessage,
    required this.isValidating,
    required this.spendFromLabel,
    required this.feePreset,
    required this.spendFromEnabled,
    required this.onSelectSpendFrom,
    required this.onEditFee,
    required this.onRemoveOutput,
    required this.onAddOutput,
    required this.onOutputChanged,
    this.onScan,
    this.onImport,
    required this.onContinue,
  });

  double _parseAmount(TextEditingController controller) {
    final raw = controller.text.trim();
    if (raw.isEmpty) return 0.0;
    final value = double.tryParse(raw);
    if (value == null || value.isNaN || value.isInfinite) return 0.0;
    return value <= 0 ? 0.0 : value;
  }

  double _totalOtherAmounts(int excludeIndex) {
    var total = 0.0;
    for (var i = 0; i < outputs.length; i++) {
      if (i == excludeIndex) continue;
      total += _parseAmount(outputs[i].amountController);
    }
    return total;
  }

  double _maxAmountForOutput(int index) {
    final otherTotal = _totalOtherAmounts(index);
    final maxAmount = availableBalance - calculatedFee - otherTotal;
    if (maxAmount.isNaN || maxAmount.isInfinite) return 0.0;
    return maxAmount < 0 ? 0.0 : maxAmount;
  }

  @override
  Widget build(BuildContext context) {
    final total = totalAmount + calculatedFee;
    final hasEnough = total <= availableBalance;
    final spendableAfterFee = availableBalance - calculatedFee;
    final spendableForPercent =
        spendableAfterFee.isFinite && spendableAfterFee > 0
        ? spendableAfterFee
        : 0.0;

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            AppSpacing.md,
            AppSpacing.md,
            AppSpacing.md,
            0,
          ),
          child: PCard(
            onTap: spendFromEnabled ? onSelectSpendFrom : null,
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(
                    Icons.account_balance_wallet_outlined,
                    color: AppColors.textSecondary,
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Spend from', style: AppTypography.bodyMedium),
                        const SizedBox(height: 2),
                        Text(
                          spendFromLabel,
                          style: AppTypography.bodySmall.copyWith(
                            color: AppColors.textSecondary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  Icon(Icons.chevron_right, color: AppColors.textTertiary),
                ],
              ),
            ),
          ),
        ),
        Expanded(
          child: ListView.builder(
            padding: const EdgeInsets.all(AppSpacing.md),
            itemCount: outputs.length + 1, // +1 for add button
            itemBuilder: (context, index) {
              if (index == outputs.length) {
                // Add recipient button
                return Padding(
                  padding: const EdgeInsets.symmetric(vertical: AppSpacing.md),
                  child: PButton(
                    onPressed: outputs.length < kMaxRecipients
                        ? onAddOutput
                        : null,
                    variant: PButtonVariant.outline,
                    fullWidth: true,
                    icon: const Icon(Icons.add),
                    child: const Text('Add recipient'),
                  ),
                );
              }

              final scanHandler = onScan == null ? null : () => onScan!(index);
              final importHandler = onImport == null
                  ? null
                  : () => onImport!(index);

              return _OutputCard(
                key: ObjectKey(outputs[index]),
                index: index,
                output: outputs[index],
                maxAmount: _maxAmountForOutput(index),
                spendableForPercent: spendableForPercent,
                canRemove: outputs.length > 1,
                onRemove: () => onRemoveOutput(index),
                onChanged: () => onOutputChanged(index),
                onScan: scanHandler,
                onImport: importHandler,
              );
            },
          ),
        ),

        // Footer with fee preview and continue button
        PrimaryActionDock(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // Fee preview
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Available:',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: AppTypography.caption,
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Text(
                    '${availableBalance.toStringAsFixed(8)} ARRR',
                    style: AppTypography.mono.copyWith(fontSize: 12),
                  ),
                ],
              ),
              const SizedBox(height: AppSpacing.xs),
              Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Network fee', style: AppTypography.caption),
                        Text(
                          feePreset.label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: AppTypography.caption.copyWith(
                            color: AppColors.textSecondary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Wrap(
                    spacing: AppSpacing.xs,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      Text(
                        '${calculatedFee.toStringAsFixed(8)} ARRR',
                        style: AppTypography.mono.copyWith(fontSize: 12),
                      ),
                      PTextButton(
                        label: 'Edit',
                        compact: true,
                        variant: PTextButtonVariant.subtle,
                        onPressed: onEditFee,
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: AppSpacing.xs),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Total:',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: AppTypography.caption.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Text(
                    '${total.toStringAsFixed(8)} ARRR',
                    style: AppTypography.mono.copyWith(
                      fontSize: 14,
                      fontWeight: FontWeight.bold,
                      color: hasEnough ? AppColors.success : AppColors.error,
                    ),
                  ),
                ],
              ),

              if (errorMessage != null) ...[
                const SizedBox(height: AppSpacing.sm),
                Text(
                  errorMessage!,
                  style: AppTypography.caption.copyWith(color: AppColors.error),
                ),
              ],

              const SizedBox(height: AppSpacing.md),

              PButton(
                text: isValidating ? 'Validating...' : 'Review',
                onPressed: isValidating ? null : onContinue,
                variant: PButtonVariant.primary,
                size: PButtonSize.large,
                loading: isValidating,
              ),
            ],
          ),
        ),
      ],
    );
  }
}

/// Individual output card
class _OutputCard extends StatelessWidget {
  final int index;
  final OutputEntry output;
  final double maxAmount;
  final double spendableForPercent;
  final bool canRemove;
  final VoidCallback onRemove;
  final VoidCallback onChanged;
  final VoidCallback? onScan;
  final VoidCallback? onImport;

  const _OutputCard({
    super.key,
    required this.index,
    required this.output,
    required this.maxAmount,
    required this.spendableForPercent,
    required this.canRemove,
    required this.onRemove,
    required this.onChanged,
    this.onScan,
    this.onImport,
  });

  static String _formatArrrFloor(double amount) {
    if (amount <= 0) return '';
    final arrrtoshis = (amount * 100000000.0).floor();
    if (arrrtoshis <= 0) return '';
    final floored = arrrtoshis / 100000000.0;
    return floored.toStringAsFixed(8);
  }

  @override
  Widget build(BuildContext context) {
    final canMax = maxAmount >= 0.00000001;

    return PCard(
      child: Padding(
        padding: const EdgeInsets.all(AppSpacing.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              children: [
                CircleAvatar(
                  radius: 16,
                  backgroundColor: AppColors.accentPrimary.withValues(
                    alpha: 0.2,
                  ),
                  child: Text(
                    '${index + 1}',
                    style: AppTypography.labelMedium.copyWith(
                      color: AppColors.accentPrimary,
                    ),
                  ),
                ),
                const SizedBox(width: AppSpacing.sm),
                Text(
                  'Recipient ${index + 1}',
                  style: AppTypography.labelMedium,
                ),
                const Spacer(),
                if (canRemove)
                  IconButton(
                    icon: const Icon(Icons.remove_circle_outline),
                    onPressed: onRemove,
                    color: AppColors.error,
                    iconSize: 20,
                    tooltip: 'Remove',
                  ),
              ],
            ),

            if (output.error != null) ...[
              const SizedBox(height: AppSpacing.sm),
              Container(
                padding: const EdgeInsets.all(AppSpacing.sm),
                decoration: BoxDecoration(
                  color: AppColors.error.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Row(
                  children: [
                    Icon(Icons.error_outline, size: 16, color: AppColors.error),
                    const SizedBox(width: AppSpacing.xs),
                    Expanded(
                      child: Text(
                        output.error!,
                        style: AppTypography.caption.copyWith(
                          color: AppColors.error,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],

            const SizedBox(height: AppSpacing.md),

            // Address input
            PInput(
              controller: output.addressController,
              label: 'Recipient address',
              hint: 'Paste address',
              helperText: 'Paste an address.',
              maxLines: 2,
              onChanged: (_) => onChanged(),
              suffixIcon: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    icon: const Icon(Icons.content_paste, size: 20),
                    onPressed: () async {
                      final data = await Clipboard.getData('text/plain');
                      if (data?.text != null) {
                        output.addressController.text = data!.text!;
                        onChanged();
                      }
                    },
                    tooltip: 'Paste',
                  ),
                  if (onImport != null)
                    IconButton(
                      icon: const Icon(Icons.image_search, size: 20),
                      onPressed: onImport,
                      tooltip: 'Import QR image',
                    ),
                  if (onScan != null)
                    IconButton(
                      icon: const Icon(Icons.qr_code_scanner, size: 20),
                      onPressed: onScan,
                      tooltip: 'Scan QR',
                    ),
                ],
              ),
            ),

            const SizedBox(height: AppSpacing.md),

            // Amount input
            PInput(
              controller: output.amountController,
              label: 'Amount (ARRR)',
              hint: '0.00000000',
              keyboardType: const TextInputType.numberWithOptions(
                decimal: true,
              ),
              onChanged: (_) => onChanged(),
              suffixIcon: _AmountMaxButton(
                enabled: canMax,
                onTap: () {
                  if (!canMax) return;
                  output.amountController.text = _formatArrrFloor(maxAmount);
                  onChanged();
                },
              ),
            ),

            const SizedBox(height: AppSpacing.xs),
            _AmountPresetSlider(
              maxAmount: maxAmount,
              spendableForPercent: spendableForPercent,
              controller: output.amountController,
              onCommit: onChanged,
              enabled: canMax,
              formatter: _formatArrrFloor,
            ),

            const SizedBox(height: AppSpacing.md),

            // Memo input
            PInput(
              controller: output.memoController,
              label: 'Memo (optional)',
              hint: 'Add a private note',
              helperText: 'A private note only the receiver can read.',
              maxLines: 2,
              maxLength: kMaxMemoBytes,
              onChanged: (_) => onChanged(),
            ),

            if (output.isMemoNearLimit)
              Padding(
                padding: const EdgeInsets.only(top: AppSpacing.xs),
                child: Text(
                  '${output.memoByteLength}/$kMaxMemoBytes bytes',
                  style: AppTypography.caption.copyWith(
                    color: output.memoByteLength > kMaxMemoBytes
                        ? AppColors.error
                        : AppColors.warning,
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _AmountMaxButton extends StatelessWidget {
  final bool enabled;
  final VoidCallback onTap;

  const _AmountMaxButton({required this.enabled, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final background = enabled
        ? AppColors.selectedBackground
        : AppColors.backgroundSurface.withValues(alpha: 0.6);
    final border = enabled ? AppColors.selectedBorder : AppColors.borderSubtle;
    final textColor = enabled
        ? AppColors.accentPrimary
        : AppColors.textTertiary;

    return Padding(
      padding: const EdgeInsets.only(right: AppSpacing.xs),
      child: InkWell(
        onTap: enabled ? onTap : null,
        borderRadius: BorderRadius.circular(PSpacing.radiusFull),
        child: Container(
          constraints: const BoxConstraints(minHeight: 36),
          padding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.sm,
            vertical: AppSpacing.xs,
          ),
          decoration: BoxDecoration(
            color: background,
            borderRadius: BorderRadius.circular(PSpacing.radiusFull),
            border: Border.all(color: border),
          ),
          child: Text('MAX', style: PTypography.labelSmall(color: textColor)),
        ),
      ),
    );
  }
}

class _AmountPresetSlider extends StatefulWidget {
  final double maxAmount;
  final double spendableForPercent;
  final TextEditingController controller;
  final VoidCallback onCommit;
  final bool enabled;
  final String Function(double) formatter;

  const _AmountPresetSlider({
    required this.maxAmount,
    required this.spendableForPercent,
    required this.controller,
    required this.onCommit,
    required this.enabled,
    required this.formatter,
  });

  @override
  State<_AmountPresetSlider> createState() => _AmountPresetSliderState();
}

class _AmountPresetSliderState extends State<_AmountPresetSlider> {
  double _value = 0.0;
  static const double _presetEpsilon = 0.02;

  @override
  void initState() {
    super.initState();
    _value = _valueFromController();
  }

  @override
  void didUpdateWidget(covariant _AmountPresetSlider oldWidget) {
    super.didUpdateWidget(oldWidget);
    final newValue = _valueFromController();
    if ((_value - newValue).abs() >= 0.001) {
      _value = newValue;
    }
  }

  double _valueFromController() {
    final maxAmount = widget.maxAmount;
    if (maxAmount <= 0) return 0.0;

    final raw = widget.controller.text.trim();
    final amount = raw.isEmpty ? 0.0 : (double.tryParse(raw) ?? 0.0);
    if (amount <= 0) return 0.0;
    if (amount >= maxAmount) return 1.0;

    final ratio = amount / maxAmount;
    if (ratio.isNaN || ratio.isInfinite) return 0.0;
    return ratio.clamp(0.0, 1.0);
  }

  double _currentAmount() {
    final raw = widget.controller.text.trim();
    if (raw.isEmpty) return 0.0;
    final value = double.tryParse(raw);
    if (value == null || value.isNaN || value.isInfinite) return 0.0;
    return value <= 0 ? 0.0 : value;
  }

  String _percentLabel() {
    final total = widget.spendableForPercent;
    if (total <= 0) return '0%';
    final ratio = (_currentAmount() / total).clamp(0.0, 1.0);
    final pct = ratio * 100.0;
    if (pct < 1) return '${pct.toStringAsFixed(2)}%';
    if (pct < 10) return '${pct.toStringAsFixed(1)}%';
    return '${pct.toStringAsFixed(0)}%';
  }

  void _setValue(double rawValue, {required bool commit}) {
    final preset = rawValue.clamp(0.0, 1.0);
    setState(() => _value = preset);

    if (widget.maxAmount <= 0) {
      widget.controller.text = '';
      if (commit) widget.onCommit();
      return;
    }

    if (preset <= 0) {
      widget.controller.text = '';
    } else {
      final amount = (widget.maxAmount * preset).clamp(0.0, widget.maxAmount);
      widget.controller.text = widget.formatter(amount);
    }
    if (commit) widget.onCommit();
  }

  @override
  Widget build(BuildContext context) {
    final enabled = widget.enabled && widget.maxAmount > 0;
    final label = _percentLabel();
    final isAt0 = (_value - 0.0).abs() <= _presetEpsilon;
    final isAtHalf = (_value - 0.5).abs() <= _presetEpsilon;
    final isAtMax = (_value - 1.0).abs() <= _presetEpsilon;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: AppSpacing.xs),
          child: SliderTheme(
            data: SliderTheme.of(context).copyWith(
              trackHeight: 3,
              activeTrackColor: AppColors.accentPrimary,
              inactiveTrackColor: AppColors.borderSubtle,
              thumbColor: AppColors.accentPrimary,
              overlayColor: AppColors.accentPrimary.withValues(alpha: 0.16),
              thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 8),
              overlayShape: const RoundSliderOverlayShape(overlayRadius: 14),
              showValueIndicator: ShowValueIndicator.onlyForContinuous,
              valueIndicatorColor: AppColors.accentPrimary,
              valueIndicatorTextStyle: PTypography.labelSmall(
                color: Colors.white,
              ),
            ),
            child: Slider(
              value: _value,
              min: 0,
              max: 1,
              label: label,
              onChanged: enabled ? (v) => _setValue(v, commit: false) : null,
              onChangeEnd: enabled ? (_) => widget.onCommit() : null,
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: AppSpacing.xs),
          child: LayoutBuilder(
            builder: (context, constraints) {
              // Match Flutter's default track insets (based on overlay radius).
              const trackInset = 14.0;
              final width = constraints.maxWidth;
              final trackWidth = (width - (trackInset * 2)).clamp(0.0, width);

              double xFor(double fraction) {
                return trackInset + (trackWidth * fraction);
              }

              double labelWidth(String label) {
                final painter = TextPainter(
                  text: TextSpan(
                    text: label,
                    style: PTypography.labelSmall(color: Colors.white),
                  ),
                  maxLines: 1,
                  textDirection: Directionality.of(context),
                )..layout();
                // _AmountPresetLabel uses horizontal padding AppSpacing.sm on both sides.
                return painter.width + (AppSpacing.sm * 2);
              }

              double clampLeft(double left, double w) {
                if (!left.isFinite) return 0.0;
                // Keep labels inside the card so they don't get clipped.
                final maxLeft = width - w;
                if (maxLeft <= 0) return 0.0;
                return left.clamp(0.0, maxLeft);
              }

              final w0 = labelWidth('0');
              final wHalf = labelWidth('1/2');
              final wMax = labelWidth('MAX');

              return SizedBox(
                height: 34,
                child: Stack(
                  clipBehavior: Clip.none,
                  children: [
                    Positioned(
                      left: clampLeft(xFor(0.0) - (w0 / 2), w0),
                      child: _AmountPresetLabel(
                        label: '0',
                        isSelected: isAt0,
                        onTap: enabled
                            ? () => _setValue(0.0, commit: true)
                            : null,
                      ),
                    ),
                    Positioned(
                      left: clampLeft(xFor(0.5) - (wHalf / 2), wHalf),
                      child: _AmountPresetLabel(
                        label: '1/2',
                        isSelected: isAtHalf,
                        onTap: enabled
                            ? () => _setValue(0.5, commit: true)
                            : null,
                      ),
                    ),
                    Positioned(
                      left: clampLeft(xFor(1.0) - (wMax / 2), wMax),
                      child: _AmountPresetLabel(
                        label: 'MAX',
                        isSelected: isAtMax,
                        onTap: enabled
                            ? () => _setValue(1.0, commit: true)
                            : null,
                      ),
                    ),
                  ],
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}

class _AmountPresetLabel extends StatelessWidget {
  final String label;
  final bool isSelected;
  final VoidCallback? onTap;

  const _AmountPresetLabel({
    required this.label,
    required this.isSelected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final color = isSelected ? AppColors.accentPrimary : AppColors.textTertiary;

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpacing.sm,
          vertical: AppSpacing.xs,
        ),
        child: Text(label, style: PTypography.labelSmall(color: color)),
      ),
    );
  }
}

class _FeePresetChip extends StatelessWidget {
  final String label;
  final bool isSelected;
  final VoidCallback onTap;

  const _FeePresetChip({
    required this.label,
    required this.isSelected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final background = isSelected
        ? AppColors.selectedBackground
        : AppColors.backgroundSurface;
    final border = isSelected
        ? AppColors.selectedBorder
        : AppColors.borderSubtle;
    final textColor = isSelected
        ? AppColors.textPrimary
        : AppColors.textSecondary;

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
      child: Container(
        constraints: const BoxConstraints(minHeight: 44),
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpacing.md,
          vertical: AppSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(PSpacing.radiusFull),
          border: Border.all(color: border),
        ),
        child: Text(label, style: PTypography.labelSmall(color: textColor)),
      ),
    );
  }
}

class _SendQrScannerScreen extends StatefulWidget {
  const _SendQrScannerScreen();

  @override
  State<_SendQrScannerScreen> createState() => _SendQrScannerScreenState();
}

class _SendQrScannerScreenState extends State<_SendQrScannerScreen> {
  final MobileScannerController _controller = MobileScannerController(
    detectionSpeed: DetectionSpeed.noDuplicates,
    facing: CameraFacing.back,
  );
  bool _hasResult = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _handleDetection(BarcodeCapture capture) {
    if (_hasResult) return;
    for (final barcode in capture.barcodes) {
      final value = barcode.rawValue;
      if (value != null && value.isNotEmpty) {
        _hasResult = true;
        Navigator.of(context).pop(value);
        return;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: SafeArea(
        child: Stack(
          children: [
            Positioned.fill(
              child: MobileScanner(
                controller: _controller,
                onDetect: _handleDetection,
              ),
            ),
            Positioned.fill(
              child: Container(color: Colors.black.withValues(alpha: 0.35)),
            ),
            Center(
              child: Container(
                width: 240,
                height: 240,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(24),
                  border: Border.all(color: AppColors.accentPrimary, width: 2),
                ),
              ),
            ),
            Positioned(
              top: AppSpacing.lg,
              left: AppSpacing.lg,
              right: AppSpacing.lg,
              child: Row(
                children: [
                  IconButton(
                    onPressed: () => Navigator.of(context).pop(),
                    icon: const Icon(Icons.close),
                    color: AppColors.textPrimary,
                    tooltip: 'Close',
                  ),
                  const Spacer(),
                  IconButton(
                    onPressed: _controller.toggleTorch,
                    icon: const Icon(Icons.flashlight_on),
                    color: AppColors.textPrimary,
                    tooltip: 'Flash',
                  ),
                ],
              ),
            ),
            Positioned(
              bottom: AppSpacing.xl,
              left: AppSpacing.lg,
              right: AppSpacing.lg,
              child: Column(
                children: [
                  Text(
                    'Scan QR code',
                    style: AppTypography.h3.copyWith(
                      color: AppColors.textPrimary,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.xs),
                  Text(
                    'Align the QR code inside the frame.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                    textAlign: TextAlign.center,
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

/// Review transaction step
class _ReviewStep extends StatelessWidget {
  final List<OutputEntry> outputs;
  final double fee;
  final double totalAmount;
  final double change;
  final PendingTx? pendingTx;
  final String spendFromLabel;
  final VoidCallback onConfirm;
  final VoidCallback onEdit;

  const _ReviewStep({
    required this.outputs,
    required this.fee,
    required this.totalAmount,
    this.change = 0,
    this.pendingTx,
    required this.spendFromLabel,
    required this.onConfirm,
    required this.onEdit,
  });

  @override
  Widget build(BuildContext context) {
    final grandTotal = totalAmount + fee;
    final gutter = PSpacing.responsiveGutter(MediaQuery.of(context).size.width);

    return SingleChildScrollView(
      padding: EdgeInsets.fromLTRB(
        gutter,
        AppSpacing.lg,
        gutter,
        AppSpacing.lg,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            'Review',
            style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
          ),

          const SizedBox(height: AppSpacing.lg),

          // Output summaries
          ...outputs.asMap().entries.map((entry) {
            final i = entry.key;
            final output = entry.value;
            return Padding(
              padding: const EdgeInsets.only(bottom: AppSpacing.md),
              child: PCard(
                child: Padding(
                  padding: const EdgeInsets.all(AppSpacing.md),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Text(
                            'Recipient ${i + 1}',
                            style: AppTypography.labelMedium,
                          ),
                          const Spacer(),
                          Text(
                            '${(double.tryParse(output.amount) ?? 0).toStringAsFixed(8)} ARRR',
                            style: AppTypography.mono.copyWith(
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: AppSpacing.sm),
                      Text(
                        '${output.address.substring(0, 16)}...${output.address.substring(output.address.length - 16)}',
                        style: AppTypography.mono.copyWith(
                          fontSize: 11,
                          color: AppColors.textSecondary,
                        ),
                      ),
                      if (output.memo.isNotEmpty) ...[
                        const SizedBox(height: AppSpacing.sm),
                        Container(
                          padding: const EdgeInsets.all(AppSpacing.sm),
                          decoration: BoxDecoration(
                            color: AppColors.surface,
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: Row(
                            children: [
                              Icon(
                                Icons.note_outlined,
                                size: 16,
                                color: AppColors.textSecondary,
                              ),
                              const SizedBox(width: AppSpacing.xs),
                              Expanded(
                                child: Text(
                                  output.memo,
                                  style: AppTypography.caption,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            );
          }),

          // Totals
          const Divider(height: AppSpacing.xl),

          _DetailRow(
            label: 'Amount',
            value: '${totalAmount.toStringAsFixed(8)} ARRR',
          ),
          const SizedBox(height: AppSpacing.sm),
          _DetailRow(
            label: 'Network Fee',
            value: '${fee.toStringAsFixed(8)} ARRR',
            valueColor: AppColors.textSecondary,
          ),
          const SizedBox(height: AppSpacing.sm),
          _DetailRow(
            label: 'Total',
            value: '${grandTotal.toStringAsFixed(8)} ARRR',
            valueColor: AppColors.accentPrimary,
            bold: true,
          ),
          const SizedBox(height: AppSpacing.sm),
          _DetailRow(
            label: 'Spend from',
            value: spendFromLabel,
            valueColor: AppColors.textSecondary,
          ),

          if (change > 0) ...[
            const SizedBox(height: AppSpacing.sm),
            _DetailRow(
              label: 'Change (returned)',
              value: '${change.toStringAsFixed(8)} ARRR',
              valueColor: AppColors.success,
            ),
          ],

          // Transaction details
          if (pendingTx != null) ...[
            const SizedBox(height: AppSpacing.lg),
            Container(
              padding: const EdgeInsets.all(AppSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.surfaceElevated,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Column(
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Inputs',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: AppTypography.caption,
                        ),
                      ),
                      Text(
                        '${pendingTx!.numInputs}',
                        style: AppTypography.caption,
                      ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Outputs',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: AppTypography.caption,
                        ),
                      ),
                      Text(
                        '${outputs.length + (change > 0 ? 1 : 0)}',
                        style: AppTypography.caption,
                      ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Expiry',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: AppTypography.caption,
                        ),
                      ),
                      Text('~30 min', style: AppTypography.caption),
                    ],
                  ),
                ],
              ),
            ),
          ],

          const SizedBox(height: AppSpacing.xl),

          // Warning
          Container(
            padding: const EdgeInsets.all(AppSpacing.md),
            decoration: BoxDecoration(
              color: AppColors.error.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppColors.error.withValues(alpha: 0.3)),
            ),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(
                  Icons.warning_amber_rounded,
                  color: AppColors.error,
                  size: 20,
                ),
                const SizedBox(width: AppSpacing.sm),
                Expanded(
                  child: Text(
                    "Transactions can't be undone.",
                    style: AppTypography.caption.copyWith(
                      color: AppColors.textPrimary,
                    ),
                  ),
                ),
              ],
            ),
          ),

          const SizedBox(height: AppSpacing.xl),

          // Buttons
          Row(
            children: [
              Expanded(
                child: PButton(
                  text: 'Edit',
                  onPressed: onEdit,
                  variant: PButtonVariant.secondary,
                  size: PButtonSize.large,
                ),
              ),
              const SizedBox(width: AppSpacing.md),
              Expanded(
                flex: 2,
                child: PButton(
                  text: 'Unlock to send',
                  onPressed: onConfirm,
                  variant: PButtonVariant.primary,
                  size: PButtonSize.large,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Detail row widget
class _DetailRow extends StatelessWidget {
  final String label;
  final String value;
  final Color? valueColor;
  final bool bold;

  const _DetailRow({
    required this.label,
    required this.value,
    this.valueColor,
    this.bold = false,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: AppTypography.body.copyWith(color: AppColors.textSecondary),
          ),
        ),
        const SizedBox(width: AppSpacing.sm),
        Expanded(
          child: Text(
            value,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.right,
            style: AppTypography.mono.copyWith(
              color: valueColor ?? AppColors.textPrimary,
              fontWeight: bold ? FontWeight.bold : FontWeight.normal,
            ),
          ),
        ),
      ],
    );
  }
}

/// Sending step with progress stages
class _SendingStep extends StatelessWidget {
  final String stage;

  const _SendingStep({this.stage = 'Sending...'});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            // Animated progress indicator
            SizedBox(
              width: 80,
              height: 80,
              child: CircularProgressIndicator(
                strokeWidth: 4,
                valueColor: AlwaysStoppedAnimation<Color>(
                  AppColors.accentPrimary,
                ),
              ),
            ),
            const SizedBox(height: AppSpacing.xxl),
            Text(
              stage,
              style: AppTypography.h4.copyWith(color: AppColors.textPrimary),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              'Please keep the app open.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.xl),
          ],
        ),
      ),
    );
  }
}

/// Error step
class _ErrorStep extends StatelessWidget {
  final String error;
  final VoidCallback onRetry;

  const _ErrorStep({required this.error, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(Icons.error_outline, size: 64, color: AppColors.error),
            const SizedBox(height: AppSpacing.lg),
            Text(
              'Send failed',
              style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              error,
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.xxl),
            PButton(
              text: 'Try Again',
              onPressed: onRetry,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
            ),
          ],
        ),
      ),
    );
  }
}
