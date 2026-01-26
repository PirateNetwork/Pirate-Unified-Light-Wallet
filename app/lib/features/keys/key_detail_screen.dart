import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:timeago/timeago.dart' as timeago;

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart'
    show AddressBalanceInfo, KeyGroupInfo, KeyTypeInfo;
import '../../core/security/biometric_auth.dart';
import '../../core/security/decoy_data.dart';
import '../../core/security/screenshot_protection.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../settings/providers/preferences_providers.dart';

class KeyDetailScreen extends ConsumerStatefulWidget {
  const KeyDetailScreen({super.key, required this.keyId});

  final int keyId;

  @override
  ConsumerState<KeyDetailScreen> createState() => _KeyDetailScreenState();
}

class _KeyDetailScreenState extends ConsumerState<KeyDetailScreen> {
  Future<_KeyDetailData>? _loadFuture;
  WalletId? _walletId;
  bool _isGenerating = false;
  String? _error;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final walletId = ref.read(activeWalletProvider);
    _setWallet(walletId);
  }

  void _setWallet(WalletId? walletId) {
    if (_walletId == walletId) return;
    _walletId = walletId;
    if (walletId != null) {
      _loadFuture = _fetchDetail(walletId);
    }
  }

  Future<_KeyDetailData> _fetchDetail(WalletId walletId) async {
    final isDecoy = ref.read(decoyModeProvider);
    if (isDecoy) {
      final key = DecoyData.keyGroups().firstWhere(
        (item) => item.id == widget.keyId,
        orElse: () => DecoyData.keyGroups().first,
      );
      final entry = DecoyData.currentAddress();
      final addresses = [
        AddressBalanceInfo(
          address: entry.address,
          balance: BigInt.zero,
          spendable: BigInt.zero,
          pending: BigInt.zero,
          keyId: key.id,
          addressId: 1,
          createdAt: entry.createdAt.millisecondsSinceEpoch ~/ 1000,
          colorTag: GeneratedAddressBookColorTag.none,
          diversifierIndex: entry.index,
        ),
      ];
      return _KeyDetailData(key: key, addresses: addresses);
    }

    final keys = await FfiBridge.listKeyGroups(walletId);
    final key = keys.firstWhere(
      (k) => k.id == widget.keyId,
      orElse: () => throw StateError('Key group not found'),
    );
    final addresses = await FfiBridge.listAddressBalances(
      walletId,
      keyId: widget.keyId,
    );
    return _KeyDetailData(key: key, addresses: addresses);
  }

  Future<void> _refresh() async {
    final walletId = _walletId;
    if (walletId == null) return;
    setState(() {
      _loadFuture = _fetchDetail(walletId);
    });
  }

  Future<void> _generateAddress({
    required bool useOrchard,
  }) async {
    final walletId = _walletId;
    if (walletId == null) return;
    final isDecoy = ref.read(decoyModeProvider);
    if (isDecoy) {
      DecoyData.generateNextAddress();
      await _refresh();
      return;
    }
    setState(() {
      _isGenerating = true;
      _error = null;
    });
    try {
      await FfiBridge.generateAddressForKey(
        walletId: walletId,
        keyId: widget.keyId,
        useOrchard: useOrchard,
      );
      await _refresh();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) {
        setState(() => _isGenerating = false);
      }
    }
  }

  Future<bool> _authorizeKeyExport() async {
    if (ref.read(decoyModeProvider)) {
      return true;
    }
    final biometricsEnabled = ref.read(biometricsEnabledProvider);
    bool biometricAvailable = false;
    try {
      biometricAvailable = await ref.read(biometricAvailabilityProvider.future);
    } catch (_) {
      biometricAvailable = false;
    }

    if (biometricsEnabled && biometricAvailable) {
      try {
        final authenticated = await BiometricAuth.authenticate(
          reason: 'Verify identity to export keys',
          biometricOnly: true,
        );
        if (authenticated) return true;
      } catch (_) {
        // Fall back to passphrase.
      }
    }

    return _verifyPassphraseDialog();
  }

  Future<bool> _verifyPassphraseDialog() async {
    final controller = TextEditingController();
    bool isVerifying = false;
    String? error;
    final result = await showDialog<bool>(
      context: context,
      barrierDismissible: true,
      barrierColor: AppColors.backgroundOverlay,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setDialogState) {
            Future<void> handleVerify() async {
              final passphrase = controller.text.trim();
              if (passphrase.isEmpty) {
                setDialogState(() => error = 'Enter your passphrase');
                return;
              }
              setDialogState(() {
                isVerifying = true;
                error = null;
              });
              try {
                final ok = await FfiBridge.verifyAppPassphrase(passphrase);
                if (!context.mounted) return;
                if (ok) {
                  controller.clear();
                  Navigator.of(context).pop(true);
                } else {
                  setDialogState(() {
                    error = 'Passphrase is incorrect';
                    isVerifying = false;
                  });
                }
              } catch (_) {
                if (!context.mounted) return;
                setDialogState(() {
                  error = 'Unable to verify passphrase';
                  isVerifying = false;
                });
              }
            }

            return Dialog(
              backgroundColor: AppColors.backgroundElevated,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(PSpacing.radiusXL),
              ),
              child: Container(
                constraints: const BoxConstraints(maxWidth: 500),
                padding: EdgeInsets.all(PSpacing.dialogPadding),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Verify passphrase',
                      style: PTypography.heading4(color: AppColors.textPrimary),
                    ),
                    SizedBox(height: PSpacing.md),
                    PInput(
                      controller: controller,
                      label: 'Passphrase',
                      hint: 'Enter your wallet passphrase',
                      obscureText: true,
                    ),
                    if (error != null) ...[
                      SizedBox(height: PSpacing.sm),
                      Text(
                        error!,
                        style: PTypography.bodySmall(color: AppColors.error),
                      ),
                    ],
                    SizedBox(height: PSpacing.lg),
                    Row(
                      mainAxisAlignment: MainAxisAlignment.end,
                      children: [
                        PButton(
                          onPressed: () => Navigator.of(context).pop(false),
                          variant: PButtonVariant.secondary,
                          child: const Text('Cancel'),
                        ),
                        SizedBox(width: PSpacing.sm),
                        PButton(
                          onPressed: isVerifying ? null : handleVerify,
                          variant: PButtonVariant.primary,
                          child:
                              Text(isVerifying ? 'Verifying...' : 'Verify'),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
    controller.dispose();
    return result ?? false;
  }

  Future<void> _exportKeys(KeyGroupInfo key) async {
    final walletId = _walletId;
    if (walletId == null) return;
    setState(() => _error = null);
    final authorized = await _authorizeKeyExport();
    if (!authorized) return;
    try {
      final isDecoy = ref.read(decoyModeProvider);
      final export = isDecoy
          ? DecoyData.exportKeyGroup(key.id)
          : await FfiBridge.exportKeyGroupKeys(
              walletId: walletId,
              keyId: key.id,
            );
      if (!mounted) return;

      final sections = <Widget?>[
        _buildKeyExportSection('Sapling viewing key', export.saplingViewingKey),
        _buildKeyExportSection('Orchard viewing key', export.orchardViewingKey),
        _buildKeyExportSection('Sapling spending key', export.saplingSpendingKey),
        _buildKeyExportSection('Orchard spending key', export.orchardSpendingKey),
      ].whereType<Widget>().toList();

      if (sections.isEmpty) {
        throw StateError('No exportable keys for this key group.');
      }

      final protection = ScreenshotProtection.protect();
      try {
        await PDialog.show<void>(
          context: context,
          title: 'Export keys',
          content: ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: MediaQuery.of(context).size.height * 0.6,
            ),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: sections,
              ),
            ),
          ),
          actions: const [
            PDialogAction(label: 'Close'),
          ],
        );
      } finally {
        protection.dispose();
      }
    } catch (e) {
      setState(() => _error = 'Failed to export keys: $e');
    }
  }

  Widget? _buildKeyExportSection(String label, String? value) {
    if (value == null || value.isEmpty) return null;
    return Padding(
      padding: EdgeInsets.only(bottom: PSpacing.md),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: PTypography.labelMedium()),
          SizedBox(height: PSpacing.xs),
          Row(
            children: [
              Expanded(
                child: SelectableText(
                  value,
                  style: PTypography.codeSmall(color: AppColors.textSecondary),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                onPressed: () => Clipboard.setData(ClipboardData(text: value)),
                tooltip: 'Copy',
              ),
            ],
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final padding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    final walletId = ref.watch(activeWalletProvider);
    _setWallet(walletId);

    return PScaffold(
      appBar: const PAppBar(
        title: 'Key details',
        subtitle: 'Manage addresses for this key',
        showBackButton: true,
        centerTitle: true,
      ),
      body: walletId == null
          ? _buildEmptyWallet()
          : FutureBuilder<_KeyDetailData>(
              future: _loadFuture,
              builder: (context, snapshot) {
                if (snapshot.connectionState == ConnectionState.waiting) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snapshot.hasError) {
                  return _buildError(
                    snapshot.error?.toString() ?? 'Failed to load key details',
                  );
                }
                final data = snapshot.data;
                if (data == null) {
                  return _buildError('Key details missing');
                }

                final key = data.key;
                return CustomScrollView(
                  slivers: [
                    SliverPadding(
                      padding: padding,
                      sliver: SliverList(
                        delegate: SliverChildListDelegate([
                          _KeySummaryCard(keyInfo: key),
                          SizedBox(height: PSpacing.lg),
                          _buildActions(context, key),
                          if (_error != null) ...[
                            SizedBox(height: PSpacing.md),
                            Text(
                              _error!,
                              style: PTypography.bodySmall(color: AppColors.error),
                            ),
                          ],
                          SizedBox(height: PSpacing.lg),
                          Text(
                            'Addresses',
                            style: PTypography.heading3(),
                          ),
                          SizedBox(height: PSpacing.xs),
                          Text(
                            'All addresses derived from this key.',
                            style: PTypography.bodySmall(
                              color: AppColors.textSecondary,
                            ),
                          ),
                          SizedBox(height: PSpacing.md),
                        ]),
                      ),
                    ),
                    SliverPadding(
                      padding: EdgeInsets.fromLTRB(
                        PSpacing.lg,
                        0,
                        PSpacing.lg,
                        PSpacing.lg,
                      ),
                      sliver: data.addresses.isEmpty
                          ? const SliverToBoxAdapter(
                              child: _AddressEmptyCard(),
                            )
                          : SliverList(
                              delegate: SliverChildBuilderDelegate(
                                (context, index) {
                                  final address = data.addresses[index];
                                  return Padding(
                                    padding: EdgeInsets.only(bottom: PSpacing.sm),
                                    child: _AddressListItem(
                                      address: address,
                                      onCopy: _copyAddress,
                                    ),
                                  );
                                },
                                childCount: data.addresses.length,
                              ),
                            ),
                    ),
                  ],
                );
              },
            ),
    );
  }

  Widget _buildActions(BuildContext context, KeyGroupInfo key) {
    final canGenerateSapling = key.hasSapling;
    final canGenerateOrchard = key.hasOrchard;

    final actions = <_ActionItem>[
      if (canGenerateSapling)
        _ActionItem(
          label: 'New Sapling address',
          variant: PButtonVariant.secondary,
          onPressed:
              _isGenerating ? null : () => _generateAddress(useOrchard: false),
        ),
      if (canGenerateOrchard)
        _ActionItem(
          label: 'New Orchard address',
          variant: PButtonVariant.secondary,
          onPressed:
              _isGenerating ? null : () => _generateAddress(useOrchard: true),
        ),
      if (key.spendable)
        _ActionItem(
          label: 'Consolidate balances',
          variant: PButtonVariant.secondary,
          onPressed: () => context.push(
            '/settings/keys/consolidate?keyId=${key.id}',
          ),
        ),
      if (key.spendable)
        _ActionItem(
          label: 'Sweep balance',
          variant: PButtonVariant.primary,
          onPressed: () => context.push(
            '/settings/keys/sweep?keyId=${key.id}',
          ),
        ),
      _ActionItem(
        label: 'Export keys',
        variant: PButtonVariant.secondary,
        onPressed: () => _exportKeys(key),
      ),
    ];

    return _buildActionGrid(context, actions);
  }

  Widget _buildActionGrid(BuildContext context, List<_ActionItem> actions) {
    if (actions.isEmpty) {
      return const SizedBox.shrink();
    }

    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        final columns = width >= 900
            ? 3
            : width >= 600
                ? 2
                : 1;
        const spacing = PSpacing.sm;
        final rows = <Widget>[];

        for (var i = 0; i < actions.length; i += columns) {
          var end = i + columns;
          if (end > actions.length) {
            end = actions.length;
          }
          final rowItems = actions.sublist(i, end);

          rows.add(
            Row(
              children: [
                for (var columnIndex = 0; columnIndex < columns; columnIndex++)
                  ...[
                    if (columnIndex > 0) SizedBox(width: spacing),
                    Expanded(
                      child: columnIndex < rowItems.length
                          ? PButton(
                              onPressed: rowItems[columnIndex].onPressed,
                              variant: rowItems[columnIndex].variant,
                              fullWidth: true,
                              child: Text(rowItems[columnIndex].label),
                            )
                          : const SizedBox.shrink(),
                    ),
                  ],
              ],
            ),
          );

          if (i + columns < actions.length) {
            rows.add(SizedBox(height: spacing));
          }
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: rows,
        );
      },
    );
  }

  Widget _buildEmptyWallet() {
    return Center(
      child: Padding(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Text(
          'No active wallet.',
          style: PTypography.bodyMedium(),
        ),
      ),
    );
  }

  Widget _buildError(String message) {
    return Center(
      child: Padding(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, color: AppColors.error, size: 40),
            SizedBox(height: PSpacing.sm),
            Text(
              message,
              style: PTypography.bodyMedium(),
              textAlign: TextAlign.center,
            ),
            SizedBox(height: PSpacing.md),
            PButton(
              onPressed: _refresh,
              variant: PButtonVariant.secondary,
              child: const Text('Retry'),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _copyAddress(AddressBalanceInfo address) async {
    await Clipboard.setData(ClipboardData(text: address.address));
  }
}

class _ActionItem {
  const _ActionItem({
    required this.label,
    required this.variant,
    required this.onPressed,
  });

  final String label;
  final PButtonVariant variant;
  final VoidCallback? onPressed;
}

class _KeySummaryCard extends StatelessWidget {
  const _KeySummaryCard({required this.keyInfo});

  final KeyGroupInfo keyInfo;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            _displayLabel(keyInfo),
            style: PTypography.heading3(),
          ),
          SizedBox(height: PSpacing.xs),
          Text(
            _subtitle(keyInfo),
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.md),
          Wrap(
            spacing: PSpacing.xs,
            runSpacing: PSpacing.xs,
            children: [
              if (keyInfo.hasSapling)
                _chip('Sapling', AppColors.infoBackground, AppColors.info),
              if (keyInfo.hasOrchard)
                _chip('Orchard', AppColors.successBackground, AppColors.success),
              if (!keyInfo.spendable)
                _chip('Watch-only', AppColors.warningBackground, AppColors.warning),
            ],
          ),
        ],
      ),
    );
  }

  String _displayLabel(KeyGroupInfo keyInfo) {
    if (keyInfo.keyType == KeyTypeInfo.seed) {
      final label = keyInfo.label?.trim();
      if (label == null || label.isEmpty || label == 'Seed') {
        return 'Default wallet keys';
      }
    }
    return keyInfo.label ?? _defaultLabel(keyInfo);
  }

  String _defaultLabel(KeyGroupInfo keyInfo) {
    return switch (keyInfo.keyType) {
      KeyTypeInfo.seed => 'Default wallet keys',
      KeyTypeInfo.importedSpending => 'Imported spending key',
      KeyTypeInfo.importedViewing => 'Viewing key',
    };
  }

  String _subtitle(KeyGroupInfo keyInfo) {
    final type = switch (keyInfo.keyType) {
      KeyTypeInfo.seed => 'Seed phrase keys',
      KeyTypeInfo.importedSpending => 'Imported spending key',
      KeyTypeInfo.importedViewing => 'Imported viewing key',
    };
    return '$type | Birthday ${keyInfo.birthdayHeight}';
  }

  Widget _chip(String text, Color background, Color foreground) {
    return Container(
      padding: EdgeInsets.symmetric(horizontal: PSpacing.sm, vertical: 4),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(color: foreground.withValues(alpha: 0.3)),
      ),
      child: Text(
        text,
        style: PTypography.labelSmall(color: foreground),
      ),
    );
  }
}

class _AddressEmptyCard extends StatelessWidget {
  const _AddressEmptyCard();

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Padding(
        padding: EdgeInsets.all(PSpacing.lg),
        child: Column(
          children: [
            Icon(Icons.history, color: AppColors.textTertiary, size: 40),
            SizedBox(height: PSpacing.sm),
            Text(
              'No addresses yet.',
              style: PTypography.bodyMedium(),
            ),
            SizedBox(height: PSpacing.xs),
            Text(
              'Generate a new address to get started.',
              style: PTypography.bodySmall(color: AppColors.textSecondary),
            ),
          ],
        ),
      ),
    );
  }
}

class _AddressListItem extends StatelessWidget {
  const _AddressListItem({
    required this.address,
    required this.onCopy,
  });

  final AddressBalanceInfo address;
  final ValueChanged<AddressBalanceInfo> onCopy;

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 36,
            height: 36,
            decoration: BoxDecoration(
              color: _colorTag(address),
              borderRadius: BorderRadius.circular(PSpacing.radiusSM),
            ),
            child: const Icon(Icons.account_balance_wallet_outlined, size: 18),
          ),
          SizedBox(width: PSpacing.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  address.label ?? _truncate(address.address),
                  style: PTypography.bodyMedium(),
                ),
                SizedBox(height: PSpacing.xs),
                Text(
                  'Index ${address.diversifierIndex} | ${_formatTimestamp(address.createdAt)}',
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                ),
                SizedBox(height: PSpacing.xs),
                Text(
                  'Balance ${_formatArrr(address.balance)}',
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                ),
              ],
            ),
          ),
          IconButton(
            icon: const Icon(Icons.copy, size: 18),
            onPressed: () => onCopy(address),
            tooltip: 'Copy address',
          ),
        ],
      ),
    );
  }

  String _truncate(String address) {
    if (address.length <= 20) return address;
    return '${address.substring(0, 10)}...${address.substring(address.length - 10)}';
  }

  String _formatTimestamp(int timestamp) {
    if (timestamp <= 0) return 'Unknown';
    return timeago.format(DateTime.fromMillisecondsSinceEpoch(timestamp * 1000));
  }

  Color _colorTag(AddressBalanceInfo address) {
    final colorTag = AddressBookColorTag.fromValue(address.colorTag.index);
    if (colorTag == AddressBookColorTag.none) {
      return AppColors.backgroundPanel;
    }
    return Color(colorTag.colorValue);
  }

  String _formatArrr(BigInt value) {
    final amount = value.toDouble() / 100000000.0;
    return '${amount.toStringAsFixed(8)} ARRR';
  }
}

class _KeyDetailData {
  const _KeyDetailData({required this.key, required this.addresses});

  final KeyGroupInfo key;
  final List<AddressBalanceInfo> addresses;
}
