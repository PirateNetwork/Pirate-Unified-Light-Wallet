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
import 'key_capabilities.dart';

class KeyManagementScreen extends ConsumerStatefulWidget {
  const KeyManagementScreen({super.key, this.keyLoader});

  final Future<List<KeyGroupInfo>> Function(WalletId walletId)? keyLoader;

  static const Key seedAccountsCardKey = Key('seed-accounts-card');
  static const Key importKeysCardKey = Key('import-keys-card');
  static const Key walletKeysSectionKey = Key('wallet-keys-section');

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
    return widget.keyLoader?.call(walletId) ??
        FfiBridge.listKeyGroups(walletId);
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
    final indices = keys
        .where((key) => key.isRecoveryPhraseAccount)
        .map((key) => key.seedAccountIndex)
        .whereType<int>();
    return indices.fold<int>(
          0,
          (highest, index) => index > highest ? index : highest,
        ) +
        1;
  }

  int _seedBirthdayHeight(List<KeyGroupInfo> keys) {
    final seedKeys = keys.where((key) => key.isRecoveryPhraseAccount);
    return seedKeys.map((key) => key.birthdayHeight).fold<int?>(null, (
          lowest,
          height,
        ) {
          if (height <= 0) return lowest;
          return lowest == null || height < lowest ? height : lowest;
        }) ??
        1;
  }

  Future<void> _showSeedAccountHelp() async {
    final isHandset = PSpacing.isHandset(MediaQuery.sizeOf(context));

    if (isHandset) {
      await showModalBottomSheet<void>(
        context: context,
        useSafeArea: true,
        isScrollControlled: true,
        showDragHandle: true,
        backgroundColor: AppColors.backgroundElevated,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(
            top: Radius.circular(PSpacing.radiusXL),
          ),
        ),
        builder: (sheetContext) =>
            _SeedAccountHelp(onClose: () => Navigator.of(sheetContext).pop()),
      );
      return;
    }

    await showDialog<void>(
      context: context,
      barrierColor: AppColors.backgroundOverlay,
      builder: (dialogContext) => Dialog(
        backgroundColor: AppColors.backgroundElevated,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(PSpacing.radiusXL),
        ),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480),
          child: _SeedAccountHelp(
            onClose: () => Navigator.of(dialogContext).pop(),
          ),
        ),
      ),
    );
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
          builder: (dialogContext) => _SeedAccountConfirmation(
            title: title,
            range: range,
            birthdayHeight: birthdayHeight,
            onCancel: () => Navigator.of(dialogContext).pop(false),
            onConfirm: () => Navigator.of(dialogContext).pop(true),
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
        'Accounts {accounts} added. Scan started.'.trArgs({
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

    Widget action({
      required String label,
      required String tooltip,
      required IconData icon,
      required int count,
    }) {
      final enabled = !isDecoy && !busy;
      final button = Semantics(
        button: true,
        enabled: enabled,
        label: label,
        hint: tooltip,
        child: PButton(
          onPressed: enabled ? () => _addSeedAccounts(keys, count) : null,
          fullWidth: true,
          variant: count == 1
              ? PButtonVariant.primary
              : PButtonVariant.secondary,
          icon: Icon(icon),
          child: Text(label),
        ),
      );
      if (PSpacing.isHandset(MediaQuery.sizeOf(context))) {
        return button;
      }
      return Tooltip(
        message: tooltip,
        constraints: const BoxConstraints(maxWidth: 280),
        waitDuration: const Duration(milliseconds: 350),
        showDuration: const Duration(seconds: 5),
        child: button,
      );
    }

    return Semantics(
      container: true,
      label: 'Seed account management'.tr,
      child: PCard(
        key: KeyManagementScreen.seedAccountsCardKey,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                Container(
                  width: 44,
                  height: 44,
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
                  child: Text(
                    'Seed accounts'.tr,
                    style: PTypography.heading4(),
                  ),
                ),
              ],
            ),
            SizedBox(height: PSpacing.sm),
            Text(
              'Account 0 is the standard account. Add another only if you used a different account number in another wallet.'
                  .tr,
              style: PTypography.bodyMedium(color: AppColors.textSecondary),
            ),
            SizedBox(height: PSpacing.md),
            Wrap(
              spacing: PSpacing.sm,
              runSpacing: PSpacing.xs,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
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
                      border: Border.all(color: AppColors.borderDefault),
                    ),
                    child: Text(
                      'Next account #{index}'.trArgs({'index': nextIndex}),
                      style: PTypography.labelSmall(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ),
                ),
                PTextButton(
                  label: 'How seed accounts work'.tr,
                  leadingIcon: Icons.info_outline,
                  compact: true,
                  onPressed: _showSeedAccountHelp,
                ),
              ],
            ),
            SizedBox(height: PSpacing.md),
            LayoutBuilder(
              builder: (context, constraints) {
                final addOne = action(
                  label: 'Add next account'.tr,
                  tooltip: 'Add account #{index} and scan for its transactions.'
                      .trArgs({'index': nextIndex}),
                  icon: Icons.add_circle_outline,
                  count: 1,
                );
                final addFive = action(
                  label: 'Add 5 accounts'.tr,
                  tooltip: 'Add accounts #{first}–#{last} and scan for their transactions.'
                      .trArgs({'first': nextIndex, 'last': nextIndex + 4}),
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
                        'Adding accounts and starting the scan…'.tr,
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

  Widget _buildImportKeysCard({required bool isDecoy}) {
    return PCard(
      key: KeyManagementScreen.importKeysCardKey,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 44,
                height: 44,
                decoration: BoxDecoration(
                  color: AppColors.accentSecondary.withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                ),
                child: Icon(
                  Icons.key_outlined,
                  color: AppColors.accentSecondary,
                ),
              ),
              SizedBox(width: PSpacing.sm),
              Expanded(
                child: Text('Import keys'.tr, style: PTypography.heading4()),
              ),
            ],
          ),
          SizedBox(height: PSpacing.md),
          LayoutBuilder(
            builder: (context, constraints) {
              final spending = _ImportKeyAction(
                icon: Icons.key,
                title: 'Spending Key'.tr,
                description: 'Add an existing key to this wallet'.tr,
                enabled: !isDecoy,
                onTap: () => context.push('/settings/keys/import'),
              );
              final viewing = _ImportKeyAction(
                icon: Icons.visibility_outlined,
                title: 'Viewing Key'.tr,
                description:
                    'Create a view-only wallet to view incoming activity.'.tr,
                enabled: !isDecoy,
                onTap: _showImportViewingKeyDialog,
              );
              if (constraints.maxWidth < 600) {
                return Column(
                  children: [
                    spending,
                    SizedBox(height: PSpacing.sm),
                    viewing,
                  ],
                );
              }
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(child: spending),
                  SizedBox(width: PSpacing.sm),
                  Expanded(child: viewing),
                ],
              );
            },
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
        subtitle: 'Manage keys & addresses'.tr,
        showBackButton: true,
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
                final hasSeed = keys.supportsSeedAccountDerivation;
                return RefreshIndicator(
                  onRefresh: () async => _refresh(),
                  child: LayoutBuilder(
                    builder: (context, _) {
                      final screenWidth = MediaQuery.sizeOf(context).width;
                      return ListView(
                        physics: const AlwaysScrollableScrollPhysics(),
                        padding: PSpacing.screenPadding(screenWidth),
                        children: [
                          Align(
                            alignment: Alignment.topCenter,
                            child: ConstrainedBox(
                              constraints: const BoxConstraints(maxWidth: 1180),
                              child: LayoutBuilder(
                                builder: (context, content) {
                                  final useColumns =
                                      hasSeed && content.maxWidth >= 900;
                                  final seedAccounts = _buildSeedAccountsCard(
                                    keys: keys,
                                    isDecoy: isDecoy,
                                  );
                                  final imports = _buildImportKeysCard(
                                    isDecoy: isDecoy,
                                  );
                                  final overview = useColumns
                                      ? Row(
                                          crossAxisAlignment:
                                              CrossAxisAlignment.start,
                                          children: [
                                            Expanded(
                                              flex: 3,
                                              child: seedAccounts,
                                            ),
                                            SizedBox(width: PSpacing.lg),
                                            Expanded(flex: 2, child: imports),
                                          ],
                                        )
                                      : Column(
                                          children: [
                                            if (hasSeed) ...[
                                              seedAccounts,
                                              SizedBox(height: PSpacing.lg),
                                            ],
                                            imports,
                                          ],
                                        );

                                  return Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.stretch,
                                    children: [
                                      overview,
                                      SizedBox(height: PSpacing.xl),
                                      if (keys.isEmpty)
                                        _buildNoKeysCard()
                                      else
                                        _buildWalletKeysSection(keys),
                                    ],
                                  );
                                },
                              ),
                            ),
                          ),
                        ],
                      );
                    },
                  ),
                );
              },
            ),
    );
  }

  Widget _buildWalletKeysSection(List<KeyGroupInfo> keys) {
    return Semantics(
      key: KeyManagementScreen.walletKeysSectionKey,
      container: true,
      label: 'Keys'.tr,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Keys'.tr, style: PTypography.heading3()),
          SizedBox(height: PSpacing.md),
          LayoutBuilder(
            builder: (context, constraints) {
              final useGrid = keys.length > 1 && constraints.maxWidth >= 760;
              final itemWidth = useGrid
                  ? (constraints.maxWidth - PSpacing.md) / 2
                  : constraints.maxWidth.clamp(0, 760).toDouble();
              return Align(
                alignment: Alignment.topLeft,
                child: Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: keys
                      .map(
                        (key) => SizedBox(
                          width: itemWidth,
                          child: _KeyCard(
                            keyInfo: key,
                            onTap: () => context.push(
                              '/settings/keys/detail?keyId=${key.id}',
                            ),
                          ),
                        ),
                      )
                      .toList(),
                ),
              );
            },
          ),
        ],
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

class _SeedAccountConfirmation extends StatelessWidget {
  const _SeedAccountConfirmation({
    required this.title,
    required this.range,
    required this.birthdayHeight,
    required this.onCancel,
    required this.onConfirm,
  });

  final String title;
  final String range;
  final int birthdayHeight;
  final VoidCallback onCancel;
  final VoidCallback onConfirm;

  @override
  Widget build(BuildContext context) {
    final isHandset = PSpacing.isHandset(MediaQuery.sizeOf(context));
    final primaryAction = PButton(
      onPressed: onConfirm,
      fullWidth: isHandset,
      child: Text('Add and scan'.tr),
    );
    final cancelAction = PTextButton(
      label: 'Cancel'.tr,
      onPressed: onCancel,
      variant: PTextButtonVariant.neutral,
      fullWidth: isHandset,
    );

    return Dialog(
      insetPadding: EdgeInsets.symmetric(
        horizontal: isHandset ? PSpacing.md : PSpacing.xl,
        vertical: PSpacing.lg,
      ),
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
                    width: 44,
                    height: 44,
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
                  Expanded(child: Text(title, style: PTypography.heading4())),
                ],
              ),
              SizedBox(height: PSpacing.lg),
              _ConfirmationDetail(
                icon: Icons.key_outlined,
                label: range,
                detail: 'Recovery phrase'.tr,
              ),
              SizedBox(height: PSpacing.sm),
              _ConfirmationDetail(
                icon: Icons.manage_search_outlined,
                label: 'Scan starts at block {height}'.trArgs({
                  'height': birthdayHeight,
                }),
                detail: 'Scanning for transactions'.tr,
              ),
              SizedBox(height: PSpacing.xl),
              if (isHandset) ...[
                primaryAction,
                SizedBox(height: PSpacing.xs),
                cancelAction,
              ] else
                Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    cancelAction,
                    SizedBox(width: PSpacing.sm),
                    primaryAction,
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ConfirmationDetail extends StatelessWidget {
  const _ConfirmationDetail({
    required this.icon,
    required this.label,
    required this.detail,
  });

  final IconData icon;
  final String label;
  final String detail;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: EdgeInsets.all(PSpacing.sm),
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: PSpacing.iconMD, color: AppColors.textSecondary),
          SizedBox(width: PSpacing.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: PTypography.labelLarge(color: AppColors.textPrimary),
                ),
                SizedBox(height: PSpacing.xxs),
                Text(
                  detail,
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

class _SeedAccountHelp extends StatelessWidget {
  const _SeedAccountHelp({required this.onClose});

  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: EdgeInsets.fromLTRB(
        PSpacing.dialogPadding,
        PSpacing.sm,
        PSpacing.dialogPadding,
        PSpacing.dialogPadding,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.account_tree_outlined, color: AppColors.accentPrimary),
              SizedBox(width: PSpacing.sm),
              Expanded(
                child: Text(
                  'How seed accounts work'.tr,
                  style: PTypography.heading4(),
                ),
              ),
            ],
          ),
          SizedBox(height: PSpacing.lg),
          _HelpPoint(
            text: "Every account comes from this wallet's recovery phrase.".tr,
          ),
          _HelpPoint(
            text: 'Each account has its own Sapling and Ironwood keys.'.tr,
          ),
          _HelpPoint(
            text: 'After you add an account, the wallet scans for its past transactions.'
                .tr,
          ),
          _HelpPoint(
            text: 'Accounts are added in order and remain even when empty.'.tr,
          ),
          _HelpPoint(
            text:
                'Imported keys can create new addresses, but not new accounts.'
                    .tr,
          ),
          SizedBox(height: PSpacing.md),
          PTextButton(
            label: 'Close'.tr,
            onPressed: onClose,
            variant: PTextButtonVariant.neutral,
            fullWidth: true,
          ),
        ],
      ),
    );
  }
}

class _HelpPoint extends StatelessWidget {
  const _HelpPoint({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(bottom: PSpacing.md),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            margin: const EdgeInsets.only(top: 3),
            width: 24,
            height: 24,
            decoration: BoxDecoration(
              color: AppColors.accentPrimary.withValues(alpha: 0.12),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.check,
              size: PSpacing.iconSM,
              color: AppColors.accentPrimary,
            ),
          ),
          SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              text,
              style: PTypography.bodyMedium(color: AppColors.textSecondary),
            ),
          ),
        ],
      ),
    );
  }
}

class _ImportKeyAction extends StatelessWidget {
  const _ImportKeyAction({
    required this.icon,
    required this.title,
    required this.description,
    required this.enabled,
    required this.onTap,
  });

  final IconData icon;
  final String title;
  final String description;
  final bool enabled;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final foreground = enabled ? AppColors.textPrimary : AppColors.textDisabled;
    return Semantics(
      button: true,
      enabled: enabled,
      label: title,
      hint: description,
      child: Material(
        color: AppColors.backgroundElevated,
        borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        child: InkWell(
          onTap: enabled ? onTap : null,
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
          child: Container(
            constraints: const BoxConstraints(minHeight: 82),
            padding: EdgeInsets.all(PSpacing.sm),
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(PSpacing.radiusMD),
              border: Border.all(color: AppColors.borderSubtle),
            ),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    color: AppColors.accentSecondary.withValues(
                      alpha: enabled ? 0.12 : 0.05,
                    ),
                    borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                  ),
                  child: Icon(
                    icon,
                    size: PSpacing.iconMD,
                    color: enabled
                        ? AppColors.accentSecondary
                        : AppColors.textDisabled,
                  ),
                ),
                SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        style: PTypography.labelLarge(color: foreground),
                      ),
                      SizedBox(height: PSpacing.xxs),
                      Text(
                        description,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: PTypography.bodySmall(
                          color: enabled
                              ? AppColors.textSecondary
                              : AppColors.textDisabled,
                        ),
                      ),
                    ],
                  ),
                ),
                SizedBox(width: PSpacing.xs),
                Icon(
                  Icons.chevron_right,
                  color: enabled
                      ? AppColors.textTertiary
                      : AppColors.textDisabled,
                ),
              ],
            ),
          ),
        ),
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
    final title = _displayKeyLabel(keyInfo);
    final type = _keyTypeLabel(keyInfo);
    return Semantics(
      button: true,
      label: title,
      hint: 'Key details'.tr,
      child: PCard(
        onTap: onTap,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  width: 44,
                  height: 44,
                  decoration: BoxDecoration(
                    color: keyInfo.spendable
                        ? AppColors.accentPrimary.withValues(alpha: 0.12)
                        : AppColors.warningBackground,
                    borderRadius: BorderRadius.circular(PSpacing.radiusMD),
                  ),
                  child: Icon(
                    keyInfo.spendable ? Icons.key : Icons.visibility_outlined,
                    color: keyInfo.spendable
                        ? AppColors.accentPrimary
                        : AppColors.warning,
                  ),
                ),
                SizedBox(width: PSpacing.sm),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(title, style: PTypography.bodyLarge()),
                      SizedBox(height: PSpacing.xxs),
                      Text(
                        type,
                        style: PTypography.bodySmall(
                          color: AppColors.textSecondary,
                        ),
                      ),
                    ],
                  ),
                ),
                SizedBox(width: PSpacing.xs),
                Icon(Icons.chevron_right, color: AppColors.textTertiary),
              ],
            ),
            SizedBox(height: PSpacing.md),
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
            SizedBox(height: PSpacing.sm),
            Row(
              children: [
                Icon(
                  Icons.history_outlined,
                  size: PSpacing.iconSM,
                  color: AppColors.textTertiary,
                ),
                SizedBox(width: PSpacing.xs),
                Expanded(
                  child: Text(
                    'Birthday {height}'.trArgs({
                      'height': keyInfo.birthdayHeight,
                    }),
                    style: PTypography.caption(color: AppColors.textTertiary),
                  ),
                ),
              ],
            ),
          ],
        ),
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
    return switch (key.keyType) {
      KeyTypeInfo.seed => 'Recovery phrase'.tr,
      KeyTypeInfo.importedSpending => 'Imported spending key'.tr,
      KeyTypeInfo.importedViewing => 'Imported viewing key'.tr,
    };
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
