import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/ffi/generated/models.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/security/screenshot_protection.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

class KeyManagementScreen extends ConsumerStatefulWidget {
  const KeyManagementScreen({super.key});

  @override
  ConsumerState<KeyManagementScreen> createState() => _KeyManagementScreenState();
}

class _KeyManagementScreenState extends ConsumerState<KeyManagementScreen> {
  WalletId? _walletId;
  Future<List<KeyGroupInfo>>? _loadFuture;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final walletId = ref.read(activeWalletProvider);
    _setWallet(walletId);
  }

  void _setWallet(WalletId? walletId) {
    if (_walletId == walletId) return;
    _walletId = walletId;
    _loadFuture = walletId == null ? null : _fetchKeys(walletId);
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

  Future<void> _exportViewingKeys() async {
    final walletId = _walletId;
    if (walletId == null) return;
    String? saplingKey;
    String? orchardKey;
    try {
      saplingKey = await FfiBridge.exportIvkSecure(walletId);
    } catch (_) {
      saplingKey = null;
    }
    try {
      orchardKey = await FfiBridge.exportOrchardViewingKey(walletId);
    } catch (_) {
      orchardKey = null;
    }

    final sections = <Widget>[
      if (saplingKey != null && saplingKey.isNotEmpty)
        _buildViewingKeySection('Sapling viewing key', saplingKey),
      if (orchardKey != null && orchardKey.isNotEmpty)
        _buildViewingKeySection('Orchard viewing key', orchardKey),
    ];

    if (sections.isEmpty) {
      _showSnack('No viewing keys available.', color: AppColors.error);
      return;
    }

    final protection = ScreenshotProtection.protect();
    try {
      await PDialog.show<void>(
        context: context,
        title: 'Viewing keys',
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: sections,
        ),
        actions: const [
          PDialogAction(label: 'Close'),
        ],
      );
    } finally {
      protection.dispose();
    }
  }

  Widget _buildViewingKeySection(String label, String value) {
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

  Future<void> _showImportViewingKeyDialog() async {
    final nameController = TextEditingController(text: 'Watch-only wallet');
    final saplingController = TextEditingController();
    final orchardController = TextEditingController();
    final birthdayController = TextEditingController(
      text: '${FfiBridge.defaultBirthdayHeight}',
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
              final orchardKey = orchardController.text.trim();
              final birthdayText = birthdayController.text.trim();
              final birthday = int.tryParse(birthdayText);

              if (name.isEmpty) {
                setDialogState(() => error = 'Enter a wallet name');
                return;
              }
              if (saplingKey.isEmpty && orchardKey.isEmpty) {
                setDialogState(() => error = 'Provide a viewing key');
                return;
              }
              if (birthday == null || birthday <= 0) {
                setDialogState(() => error = 'Enter a valid birthday height');
                return;
              }

              setDialogState(() {
                isLoading = true;
                error = null;
              });

              try {
                await FfiBridge.importIvk(
                  name: name,
                  saplingIvk: saplingKey.isEmpty ? null : saplingKey,
                  orchardIvk: orchardKey.isEmpty ? null : orchardKey,
                  birthday: birthday,
                );
                ref.read(refreshWalletsProvider)();
                if (!context.mounted) return;
                Navigator.of(context).pop(true);
              } catch (e) {
                setDialogState(() => error = 'Failed to import: $e');
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
                constraints: const BoxConstraints(maxWidth: 520),
                padding: EdgeInsets.all(PSpacing.dialogPadding),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Import viewing key',
                      style: PTypography.heading4(color: AppColors.textPrimary),
                    ),
                    SizedBox(height: PSpacing.md),
                    PInput(
                      controller: nameController,
                      label: 'Wallet name',
                      hint: 'e.g. Watch-only wallet',
                    ),
                    SizedBox(height: PSpacing.md),
                    PInput(
                      controller: saplingController,
                      label: 'Sapling viewing key (optional)',
                      hint: 'Paste your Sapling viewing key',
                      maxLines: 3,
                    ),
                    SizedBox(height: PSpacing.md),
                    PInput(
                      controller: orchardController,
                      label: 'Orchard viewing key (optional)',
                      hint: 'Paste your Orchard viewing key',
                      maxLines: 3,
                    ),
                    SizedBox(height: PSpacing.md),
                    PInput(
                      controller: birthdayController,
                      label: 'Birthday height',
                      hint: 'Block height to start scanning',
                      keyboardType: TextInputType.number,
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
                          onPressed: isLoading ? null : handleImport,
                          variant: PButtonVariant.primary,
                          child: Text(isLoading ? 'Importing...' : 'Import'),
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
    orchardController.dispose();
    birthdayController.dispose();

    if (imported == true) {
      _showSnack('Watch-only wallet imported.');
    }
  }

  Widget _buildViewingKeyCard() {
    return PCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Viewing keys', style: PTypography.heading4()),
          SizedBox(height: PSpacing.xs),
          Text(
            'Export or import viewing keys for watch-only wallets.',
            style: PTypography.bodySmall(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.md),
          Wrap(
            spacing: PSpacing.sm,
            runSpacing: PSpacing.sm,
            children: [
              PButton(
                onPressed: _exportViewingKeys,
                variant: PButtonVariant.secondary,
                child: const Text('Export viewing key'),
              ),
              PButton(
                onPressed: _showImportViewingKeyDialog,
                variant: PButtonVariant.secondary,
                child: const Text('Import viewing key'),
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
    _setWallet(walletId);

    return PScaffold(
      appBar: PAppBar(
        title: 'Keys & Addresses',
        subtitle: 'Manage imported keys and addresses',
        showBackButton: true,
        centerTitle: true,
        actions: [
          IconButton(
            tooltip: 'Import spending key',
            icon: const Icon(Icons.add),
            onPressed: walletId == null
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
                    snapshot.error?.toString() ?? 'Failed to load keys',
                  );
                }
                final keys = snapshot.data ?? [];
                return RefreshIndicator(
                  onRefresh: () async => _refresh(),
                  child: ListView(
                    padding: PSpacing.screenPadding(
                      MediaQuery.of(context).size.width,
                    ),
                    children: [
                      _buildViewingKeyCard(),
                      if (keys.isEmpty) ...[
                        SizedBox(height: PSpacing.lg),
                        _buildNoKeysCard(),
                      ] else ...[
                        SizedBox(height: PSpacing.lg),
                        ...keys.map((key) => Padding(
                              padding: EdgeInsets.only(bottom: PSpacing.md),
                              child: _KeyCard(
                                keyInfo: key,
                                onTap: () => context.push(
                                  '/settings/keys/detail?keyId=${key.id}',
                                ),
                              ),
                            )),
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

  Widget _buildNoKeysCard() {
    return PCard(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.vpn_key_outlined, color: AppColors.textTertiary, size: 44),
          SizedBox(height: PSpacing.sm),
          Text(
            'No keys yet',
            style: PTypography.heading3(),
          ),
          SizedBox(height: PSpacing.xs),
          Text(
            'Import a spending key to manage legacy addresses.',
            style: PTypography.bodySmall(color: AppColors.textSecondary),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PSpacing.md),
          PTextButton(
            label: 'Import spending key',
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
                color: keyInfo.spendable ? AppColors.accentPrimary : AppColors.textSecondary,
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

  String _keyTypeLabel(KeyGroupInfo key) {
    final type = switch (key.keyType) {
      KeyTypeInfo.seed => 'Seed phrase keys',
      KeyTypeInfo.importedSpending => 'Imported spending key',
      KeyTypeInfo.importedViewing => 'Imported viewing key',
    };
    return '$type | Birthday ${key.birthdayHeight}';
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
