import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/security/decoy_data.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';

class KeyManagementScreen extends ConsumerStatefulWidget {
  const KeyManagementScreen({super.key});

  @override
  ConsumerState<KeyManagementScreen> createState() =>
      _KeyManagementScreenState();
}

class _KeyManagementScreenState extends ConsumerState<KeyManagementScreen> {
  WalletId? _walletId;
  Future<List<KeyGroupInfo>>? _loadFuture;
  bool _isDecoy = false;
  bool _isAddingSeedAccounts = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final walletId = ref.read(activeWalletProvider);
    final isDecoy = ref.read(decoyModeProvider);
    _setWallet(walletId, isDecoy);
  }

  void _setWallet(WalletId? walletId, bool isDecoy) {
    if (_walletId == walletId && _isDecoy == isDecoy) return;
    _walletId = walletId;
    _isDecoy = isDecoy;
    if (walletId == null) {
      _loadFuture = null;
      return;
    }
    _loadFuture = isDecoy
        ? Future.value(DecoyData.keyGroups())
        : _fetchKeys(walletId);
  }

  Future<List<KeyGroupInfo>> _fetchKeys(WalletId walletId) {
    return FfiBridge.listKeyGroups(walletId);
  }

  void _refresh() {
    final walletId = _walletId;
    if (walletId == null) return;
    setState(() {
      _loadFuture = _fetchKeys(walletId);
    });
  }

  void _showSnack(String message, {Color? color}) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: color ?? AppColors.success,
      ),
    );
  }

  Future<int?> _getDefaultBirthdayHeight() async {
    try {
      return await FfiBridge.getDefaultBirthdayHeight();
    } catch (_) {
      return null;
    }
  }

  Future<void> _showImportViewingKeyDialog() async {
    final defaultBirthday = await _getDefaultBirthdayHeight();
    if (!mounted) return;
    final nameController = TextEditingController(text: 'View only wallet'.tr);
    final saplingController = TextEditingController();
    final ironwoodController = TextEditingController();
    final birthdayController = TextEditingController(
      text: defaultBirthday?.toString() ?? '',
    );
    bool isLoading = false;
    String? error;

    final imported = await showDialog<bool>(
      context: context,
      barrierDismissible: true,
      barrierColor: AppColors.backgroundOverlay,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setDialogState) {
            Future<void> handleImport() async {
              final name = nameController.text.trim();
              final saplingKey = saplingController.text.trim();
              final ironwoodKey = ironwoodController.text.trim();
              final birthdayText = birthdayController.text.trim();
              final birthday = int.tryParse(birthdayText);

              if (name.isEmpty) {
                setDialogState(() => error = 'Enter a wallet name'.tr);
                return;
              }
              if (saplingKey.isEmpty && ironwoodKey.isEmpty) {
                setDialogState(() => error = 'Provide a viewing key'.tr);
                return;
              }
              if (birthday == null || birthday <= 0) {
                setDialogState(
                  () => error = 'Enter a valid birthday height'.tr,
                );
                return;
              }

              setDialogState(() {
                isLoading = true;
                error = null;
              });

              try {
                await ref.read(importViewingWalletProvider)(
                  name: name,
                  saplingViewingKey: saplingKey.isEmpty ? null : saplingKey,
                  ironwoodViewingKey: ironwoodKey.isEmpty ? null : ironwoodKey,
                  birthday: birthday,
                );
                if (!context.mounted) return;
                Navigator.of(context).pop(true);
              } catch (e) {
                setDialogState(
                  () =>
                      error = 'Failed to import: {error}'.trArgs({'error': e}),
                );
              } finally {
                if (context.mounted) {
                  setDialogState(() => isLoading = false);
                }
              }
            }

            return Dialog(
              backgroundColor: AppColors.backgroundElevated,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(PSpacing.radiusXL),
              ),
              child: Container(
                constraints: BoxConstraints(
                  maxWidth: 520,
                  maxHeight: MediaQuery.of(context).size.height * 0.88,
                ),
                padding: EdgeInsets.all(PSpacing.dialogPadding),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Import viewing key'.tr,
                      style: PTypography.heading4(color: AppColors.textPrimary),
                    ),
                    SizedBox(height: PSpacing.md),
                    Flexible(
                      child: SingleChildScrollView(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            PInput(
                              controller: nameController,
                              label: 'Wallet name'.tr,
                              hint: 'e.g. View only wallet'.tr,
                            ),
                            SizedBox(height: PSpacing.md),
                            PInput(
                              controller: saplingController,
                              label: 'Sapling viewing key (optional)'.tr,
                              hint: 'Paste your Sapling viewing key'.tr,
                              maxLines: 4,
                            ),
                            SizedBox(height: PSpacing.md),
                            PInput(
                              controller: ironwoodController,
                              label: 'Ironwood viewing key (optional)'.tr,
                              hint: 'Paste your Ironwood viewing key'.tr,
                              maxLines: 4,
                            ),
                            SizedBox(height: PSpacing.md),
                            PInput(
                              controller: birthdayController,
                              label: 'Birthday height'.tr,
                              hint: 'Block height to start scanning'.tr,
                              keyboardType: TextInputType.number,
                            ),
                            if (error != null) ...[
                              SizedBox(height: PSpacing.sm),
                              Text(
                                error!,
                                style: PTypography.bodySmall(
                                  color: AppColors.error,
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                    SizedBox(height: PSpacing.lg),
                    Wrap(
                      alignment: WrapAlignment.end,
                      spacing: PSpacing.sm,
                      runSpacing: PSpacing.sm,
                      children: [
                        PButton(
                          onPressed: () => Navigator.of(context).pop(false),
                          variant: PButtonVariant.secondary,
                          child: Text('Cancel'.tr),
                        ),
                        PButton(
                          onPressed: isLoading ? null : handleImport,
                          variant: PButtonVariant.primary,
                          child: Text(
                            isLoading ? 'Importing...'.tr : 'Import'.tr,
                          ),
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

    nameController.dispose();
    saplingController.dispose();
    ironwoodController.dispose();
    birthdayController.dispose();

    if (imported ?? false) {
      _showSnack('View only wallet imported.'.tr);
    }
  }

  int _nextSeedAccountIndex(List<KeyGroupInfo> keys) {
    final indices = keys.map((key) => key.seedAccountIndex).whereType<int>();
    return indices.fold<int>(
          0,
          (highest, index) => index > highest ? index : highest,
        ) +
        1;
  }

  int _seedBirthdayHeight(List<KeyGroupInfo> keys) {
    final seedKeys = keys.where((key) => key.seedAccountIndex != null);
    return seedKeys.map((key) => key.birthdayHeight).fold<int?>(null, (
          lowest,
          height,
        ) {
          if (height <= 0) return lowest;
          return lowest == null || height < lowest ? height : lowest;
        }) ??
        1;
  }

  Future<bool> _confirmSeedAccountAddition({
    required int count,
    required int firstIndex,
    required int birthdayHeight,
  }) async {
    final lastIndex = firstIndex + count - 1;
    final title = count == 1
        ? 'Add seed account #{index}?'.trArgs({'index': firstIndex})
        : 'Add 5 seed accounts?'.tr;
    final range = count == 1
        ? 'Account #{index}'.trArgs({'index': firstIndex})
        : 'Accounts #{first}–#{last}'.trArgs({
            'first': firstIndex,
            'last': lastIndex,
          });

    return await showDialog<bool>(
          context: context,
          barrierColor: AppColors.backgroundOverlay,
          builder: (dialogContext) => Dialog(
            backgroundColor: AppColors.backgroundElevated,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(PSpacing.radiusXL),
            ),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 480),
              child: Padding(
                padding: EdgeInsets.all(PSpacing.dialogPadding),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Container(
                          width: 40,
                          height: 40,
                          decoration: BoxDecoration(
                            color: AppColors.accentPrimary.withValues(
                              alpha: 0.12,
                            ),
                            borderRadius: BorderRadius.circular(
                              PSpacing.radiusMD,
                            ),
                          ),
                          child: Icon(
                            Icons.account_tree_outlined,
                            color: AppColors.accentPrimary,
                          ),
                        ),
                        SizedBox(width: PSpacing.sm),
                        Expanded(
                          child: Text(title, style: PTypography.heading4()),
                        ),
                      ],
                    ),
                    SizedBox(height: PSpacing.md),
                    Text(
                      "{range} will be derived from this wallet's existing seed phrase. This does not create a new seed."
                          .trArgs({'range': range}),
                      style: PTypography.bodyMedium(
                        color: AppColors.textSecondary,
                      ),
                    ),
                    SizedBox(height: PSpacing.sm),
                    Text(
                      'The wallet will then rescan from birthday block {height} using verified cached blocks when available.'
                          .trArgs({'height': birthdayHeight}),
                      style: PTypography.bodySmall(
                        color: AppColors.textTertiary,
                      ),
                    ),
                    SizedBox(height: PSpacing.lg),
                    Wrap(
                      alignment: WrapAlignment.end,
                      spacing: PSpacing.sm,
                      runSpacing: PSpacing.sm,
                      children: [
                        PButton(
                          onPressed: () =>
                              Navigator.of(dialogContext).pop(false),
                          variant: PButtonVariant.ghost,
                          child: Text('Cancel'.tr),
                        ),
                        PButton(
                          onPressed: () =>
                              Navigator.of(dialogContext).pop(true),
                          child: Text('Add and rescan'.tr),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          ),
        ) ??
        false;
  }

  Future<void> _addSeedAccounts(List<KeyGroupInfo> keys, int count) async {
    final walletId = _walletId;
    if (walletId == null || _isDecoy || _isAddingSeedAccounts) return;
    final firstIndex = _nextSeedAccountIndex(keys);
    final birthdayHeight = _seedBirthdayHeight(keys);
    final confirmed = await _confirmSeedAccountAddition(
      count: count,
      firstIndex: firstIndex,
      birthdayHeight: birthdayHeight,
    );
    if (!confirmed || !mounted) return;

    setState(() => _isAddingSeedAccounts = true);
    List<int>? added;
    try {
      added = await FfiBridge.addNextSeedAccounts(
        walletId: walletId,
        count: count,
      );
      _refresh();
      await ref.read(rescanProvider)(birthdayHeight);
      if (!mounted) return;
      final addedLabel = added.length == 1
          ? '#${added.first}'
          : '#${added.first}–#${added.last}';
      _showSnack(
        'Seed account(s) {accounts} added. Historical rescan started.'.trArgs({
          'accounts': addedLabel,
        }),
      );
    } catch (error) {
      if (!mounted) return;
      if (added != null) {
        _showSnack(
          'The accounts were added, but the rescan could not start: {error}'
              .trArgs({'error': error}),
          color: AppColors.warning,
        );
      } else {
        _showSnack(
          'Could not add seed accounts: {error}'.trArgs({'error': error}),
          color: AppColors.error,
        );
      }
    } finally {
      if (mounted) setState(() => _isAddingSeedAccounts = false);
    }
  }

  Widget _buildSeedAccountsCard({
    required List<KeyGroupInfo> keys,
    required bool isDecoy,
  }) {
    final nextIndex = _nextSeedAccountIndex(keys);
    final busy = _isAddingSeedAccounts;
    const addOneTooltip =
        'Adds the next numbered account from your current seed, then rescans for its funds. The account stays even if it is empty.';
    const addFiveTooltip =
        'Adds five numbered accounts in order. It does not stop at empty accounts, so you can repeat this to reach a higher account number.';

    Widget action({
      required String label,
      required String tooltip,
      required IconData icon,
      required int count,
    }) {
      final enabled = !isDecoy && !busy;
      return Tooltip(
        message: tooltip.tr,
        waitDuration: const Duration(milliseconds: 350),
        showDuration: const Duration(seconds: 8),
        child: Semantics(
          button: true,
          enabled: enabled,
          label: label.tr,
          hint: tooltip.tr,
          child: PButton(
            onPressed: enabled ? () => _addSeedAccounts(keys, count) : null,
            fullWidth: true,
            variant: count == 1
                ? PButtonVariant.primary
                : PButtonVariant.secondary,
            icon: Icon(icon),
            child: Text(label.tr),
          ),
        ),
      );
    }

    return Semantics(
      container: true,
      label: 'Seed account management'.tr,
      child: PCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    color: AppColors.accentPrimary.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                  ),
                  child: Icon(
                    Icons.account_tree_outlined,
                    color: AppColors.accentPrimary,
                  ),
                ),
                SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('Seed accounts'.tr, style: PTypography.heading4()),
                      SizedBox(height: PSpacing.xxs),
                      Text(
                        'A seed can hold funds in separate numbered accounts. Add them in order if another wallet used more than the default account.'
                            .tr,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ),
                ),
                SizedBox(width: PSpacing.sm),
                Semantics(
                  label: 'Next seed account is {index}'.trArgs({
                    'index': nextIndex,
                  }),
                  child: Container(
                    padding: EdgeInsets.symmetric(
                      horizontal: PSpacing.sm,
                      vertical: PSpacing.xs,
                    ),
                    decoration: BoxDecoration(
                      color: AppColors.backgroundElevated,
                      borderRadius: BorderRadius.circular(PSpacing.radiusFull),
                      border: Border.all(color: AppColors.borderSubtle),
                    ),
                    child: Text(
                      'Next #{index}'.trArgs({'index': nextIndex}),
                      style: PTypography.labelSmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ),
                ),
              ],
            ),
            SizedBox(height: PSpacing.md),
            LayoutBuilder(
              builder: (context, constraints) {
                final addOne = action(
                  label: 'Add next account',
                  tooltip: addOneTooltip,
                  icon: Icons.add_circle_outline,
                  count: 1,
                );
                final addFive = action(
                  label: 'Add 5 accounts',
                  tooltip: addFiveTooltip,
                  icon: Icons.playlist_add,
                  count: 5,
                );
                if (constraints.maxWidth < 520) {
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      addOne,
                      SizedBox(height: PSpacing.sm),
                      addFive,
                    ],
                  );
                }
                return Row(
                  children: [
                    Expanded(child: addOne),
                    SizedBox(width: PSpacing.sm),
                    Expanded(child: addFive),
                  ],
                );
              },
            ),
            if (busy) ...[
              SizedBox(height: PSpacing.md),
              Semantics(
                liveRegion: true,
                label: 'Adding seed accounts and preparing rescan'.tr,
                child: Row(
                  children: [
                    const SizedBox.square(
                      dimension: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                    SizedBox(width: PSpacing.sm),
                    Expanded(
                      child: Text(
                        'Adding accounts and preparing a historical rescan…'.tr,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildViewingKeyCard({required bool isDecoy}) {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Key imports'.tr, style: PTypography.heading4()),
          SizedBox(height: PSpacing.xs),
          Text(
            'Import viewing keys for view only wallets or add a private key.'
                .tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.md),
          Wrap(
            spacing: PSpacing.sm,
            runSpacing: PSpacing.sm,
            children: [
              PButton(
                onPressed: isDecoy
                    ? null
                    : () => context.push('/settings/keys/import'),
                variant: PButtonVariant.secondary,
                child: Text('Import private key'.tr),
              ),
              PButton(
                onPressed: isDecoy ? null : _showImportViewingKeyDialog,
                variant: PButtonVariant.secondary,
                child: Text('Import viewing key'.tr),
              ),
            ],
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final walletId = ref.watch(activeWalletProvider);
    final isDecoy = ref.watch(decoyModeProvider);
    _setWallet(walletId, isDecoy);

    return PScaffold(
      appBar: PAppBar(
        title: 'Keys & Addresses'.tr,
        subtitle: 'Manage imported keys and addresses'.tr,
        showBackButton: true,
        actions: [
          IconButton(
            tooltip: 'Import spending key'.tr,
            icon: const Icon(Icons.add),
            onPressed: walletId == null || isDecoy
                ? null
                : () => context.push('/settings/keys/import'),
          ),
        ],
      ),
      body: walletId == null
          ? _buildEmptyWallet()
          : FutureBuilder<List<KeyGroupInfo>>(
              future: _loadFuture,
              builder: (context, snapshot) {
                if (snapshot.connectionState == ConnectionState.waiting) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snapshot.hasError) {
                  return _buildError(
                    snapshot.error?.toString() ?? 'Failed to load keys'.tr,
                  );
                }
                final keys = snapshot.data ?? [];
                final hasSeed = keys.any((key) => key.seedAccountIndex == 0);
                return RefreshIndicator(
                  onRefresh: () async => _refresh(),
                  child: ListView(
                    padding: PSpacing.screenPadding(
                      MediaQuery.of(context).size.width,
                    ),
                    children: [
                      if (hasSeed) ...[
                        _buildSeedAccountsCard(keys: keys, isDecoy: isDecoy),
                        SizedBox(height: PSpacing.lg),
                      ],
                      _buildViewingKeyCard(isDecoy: isDecoy),
                      if (keys.isEmpty) ...[
                        SizedBox(height: PSpacing.lg),
                        _buildNoKeysCard(),
                      ] else ...[
                        SizedBox(height: PSpacing.lg),
                        ...keys.map(
                          (key) => Padding(
                            padding: EdgeInsets.only(bottom: PSpacing.md),
                            child: _KeyCard(
                              keyInfo: key,
                              onTap: () => context.push(
                                '/settings/keys/detail?keyId=${key.id}',
                              ),
                            ),
                          ),
                        ),
                      ],
                    ],
                  ),
                );
              },
            ),
    );
  }

  Widget _buildEmptyWallet() {
    return Center(
      child: Padding(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Text('No active wallet.'.tr, style: PTypography.bodyMedium()),
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
              child: Text('Retry'.tr),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildNoKeysCard() {
    return PCard(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.vpn_key_outlined, color: AppColors.textTertiary, size: 44),
          SizedBox(height: PSpacing.sm),
          Text('No keys yet'.tr, style: PTypography.heading3()),
          SizedBox(height: PSpacing.xs),
          Text(
            'Import a spending key to manage legacy addresses.'.tr,
            style: PTypography.bodySmall(color: AppColors.textSecondary),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PSpacing.md),
          PTextButton(
            label: 'Import spending key'.tr,
            leadingIcon: Icons.add,
            onPressed: () => context.push('/settings/keys/import'),
          ),
        ],
      ),
    );
  }
}

class _KeyCard extends StatelessWidget {
  const _KeyCard({required this.keyInfo, required this.onTap});

  final KeyGroupInfo keyInfo;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return PCard(
      onTap: onTap,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                keyInfo.spendable ? Icons.key : Icons.visibility_outlined,
                color: keyInfo.spendable
                    ? AppColors.accentPrimary
                    : AppColors.textSecondary,
              ),
              SizedBox(width: PSpacing.sm),
              Expanded(
                child: Text(
                  _displayKeyLabel(keyInfo),
                  style: PTypography.bodyLarge(),
                ),
              ),
              Icon(Icons.chevron_right, color: AppColors.textTertiary),
            ],
          ),
          SizedBox(height: PSpacing.sm),
          Text(
            _keyTypeLabel(keyInfo),
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.sm),
          Wrap(
            spacing: PSpacing.xs,
            runSpacing: PSpacing.xs,
            children: [
              if (keyInfo.seedAccountIndex case final accountIndex?)
                _chip(
                  'Account #{index}'.trArgs({'index': accountIndex}),
                  AppColors.accentPrimary.withValues(alpha: 0.12),
                  AppColors.accentPrimary,
                ),
              if (keyInfo.hasSapling)
                _chip('Sapling', AppColors.infoBackground, AppColors.info),
              if (keyInfo.hasIronwood)
                _chip(
                  'Ironwood',
                  AppColors.successBackground,
                  AppColors.success,
                ),
              if (!keyInfo.spendable)
                _chip(
                  'View only'.tr,
                  AppColors.warningBackground,
                  AppColors.warning,
                ),
            ],
          ),
        ],
      ),
    );
  }

  String _displayKeyLabel(KeyGroupInfo key) {
    if (key.keyType == KeyTypeInfo.seed) {
      final accountIndex = key.seedAccountIndex;
      if (accountIndex != null && accountIndex > 0) {
        return 'Seed account {index}'.trArgs({'index': accountIndex});
      }
      final label = key.label?.trim();
      if (label == null || label.isEmpty || label == 'Seed') {
        return 'Default wallet keys'.tr;
      }
    }
    return key.label ?? _defaultKeyLabel(key);
  }

  String _defaultKeyLabel(KeyGroupInfo key) {
    switch (key.keyType) {
      case KeyTypeInfo.seed:
        return 'Default wallet keys'.tr;
      case KeyTypeInfo.importedSpending:
        return 'Imported spending key'.tr;
      case KeyTypeInfo.importedViewing:
        return 'Viewing key'.tr;
    }
  }

  String _keyTypeLabel(KeyGroupInfo key) {
    final accountIndex = key.seedAccountIndex;
    if (key.keyType == KeyTypeInfo.seed && accountIndex != null) {
      return '{type} | Birthday {height}'.trArgs({
        'type': 'Seed phrase account #{index}'.trArgs({'index': accountIndex}),
        'height': key.birthdayHeight,
      });
    }
    final type = switch (key.keyType) {
      KeyTypeInfo.seed => 'Seed phrase keys'.tr,
      KeyTypeInfo.importedSpending => 'Imported spending key'.tr,
      KeyTypeInfo.importedViewing => 'Imported viewing key'.tr,
    };
    return '{type} | Birthday {height}'.trArgs({
      'type': type,
      'height': key.birthdayHeight,
    });
  }

  Widget _chip(String text, Color background, Color foreground) {
    return Container(
      padding: EdgeInsets.symmetric(horizontal: PSpacing.sm, vertical: 4),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(color: foreground.withValues(alpha: 0.3)),
      ),
      child: Text(text, style: PTypography.labelSmall(color: foreground)),
    );
  }
}
