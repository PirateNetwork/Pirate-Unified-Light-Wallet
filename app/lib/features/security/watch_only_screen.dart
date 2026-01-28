// Watch-only wallet management screen
// 
// Handles viewing key export/import for creating view-only wallets

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/security/screenshot_protection.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Watch-only screen tabs
enum WatchOnlyTab { export, import }

/// Watch-only screen
class WatchOnlyScreen extends ConsumerStatefulWidget {
  const WatchOnlyScreen({super.key});

  @override
  ConsumerState<WatchOnlyScreen> createState() => _WatchOnlyScreenState();
}

class _WatchOnlyScreenState extends ConsumerState<WatchOnlyScreen>
    with SingleTickerProviderStateMixin {
  late TabController _tabController;
  
  // Export state
  bool _isExporting = false;
  String? _exportedIvk;
  int? _clipboardTimer;
  ScreenProtection? _screenProtection;
  
  // Import state
  final _nameController = TextEditingController();
  final _ivkController = TextEditingController();
  final _birthdayController = TextEditingController(text: '2000000');
  bool _isImporting = false;
  String? _importError;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 2, vsync: this);
  }

  @override
  void dispose() {
    _tabController.dispose();
    _nameController.dispose();
    _ivkController.dispose();
    _birthdayController.dispose();
    _enableScreenshots();
    super.dispose();
  }

  void _disableScreenshots() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _enableScreenshots() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  Future<void> _exportIvk() async {
    setState(() {
      _isExporting = true;
      _exportedIvk = null;
    });
    _enableScreenshots();

    try {
      final walletId = ref.read(activeWalletProvider);
      if (walletId == null) {
        throw Exception('No active wallet');
      }

      final ivk = await FfiBridge.exportIvkSecure(walletId);
      
      setState(() {
        _exportedIvk = ivk;
      });
      _disableScreenshots();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to export: $e'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    } finally {
      setState(() => _isExporting = false);
    }
  }

  Future<void> _copyIvkToClipboard() async {
    if (_exportedIvk == null) return;

    await Clipboard.setData(ClipboardData(text: _exportedIvk!));
    
    setState(() => _clipboardTimer = 30);
    _startClipboardTimer();
    
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: const Text('Viewing key copied. Auto-clearing in 30 seconds.'),
          backgroundColor: AppColors.warning,
        ),
      );
    }
  }

  void _startClipboardTimer() {
    Future.doWhile(() async {
      await Future<void>.delayed(const Duration(seconds: 1));
      if (_clipboardTimer == null || _clipboardTimer! <= 0) {
        await Clipboard.setData(const ClipboardData(text: ''));
        setState(() => _clipboardTimer = null);
        return false;
      }
      setState(() => _clipboardTimer = _clipboardTimer! - 1);
      return true;
    });
  }

  Future<void> _importIvk() async {
    setState(() {
      _isImporting = true;
      _importError = null;
    });

    try {
      // Validate inputs
      if (_nameController.text.isEmpty) {
        throw Exception('Please enter a wallet name');
      }
      if (_ivkController.text.isEmpty) {
        throw Exception('Please enter the viewing key');
      }

      final birthday = int.tryParse(_birthdayController.text);
      if (birthday == null || birthday <= 0) {
        throw Exception('Invalid birthday height');
      }

      await FfiBridge.importIvkAsWatchOnly(
        name: _nameController.text,
        ivk: _ivkController.text,
        birthdayHeight: birthday,
      );

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Watch-only wallet created'),
            backgroundColor: AppColors.success,
          ),
        );
        context.go('/home');
      }
    } catch (e) {
      setState(() => _importError = e.toString());
    } finally {
      setState(() => _isImporting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    // Listen for wallet changes
    ref.listen<WalletId?>(
      activeWalletProvider,
      (previous, next) {
        if (previous != next) {
          _resetExportState();
        }
      },
    );
    
    final walletMeta = ref.watch(activeWalletMetaProvider);
    final isWatchOnly = walletMeta?.watchOnly ?? false;

    return PScaffold(
      title: 'Watch-Only Wallets',
      appBar: const PAppBar(
        title: 'Watch-Only Wallets',
        subtitle: 'Export or import viewing keys',
      ),
      body: Column(
        children: [
          Material(
            color: Colors.transparent,
            child: TabBar(
              controller: _tabController,
              indicatorColor: AppColors.accentPrimary,
              labelColor: AppColors.textPrimary,
              tabs: const [
                Tab(text: 'Export Viewing Key'),
                Tab(text: 'Import Viewing Key'),
              ],
            ),
          ),
          Expanded(
            child: TabBarView(
              controller: _tabController,
              children: [
                _buildExportTab(isWatchOnly),
                _buildImportTab(),
              ],
            ),
          ),
        ],
      ),
    );
  }

  void _resetExportState() {
    if (!mounted) return;
    _enableScreenshots();
    setState(() {
      _isExporting = false;
      _exportedIvk = null;
      _clipboardTimer = null;
      _isImporting = false;
      _importError = null;
    });
  }

  Widget _buildExportTab(bool isWatchOnly) {
    final basePadding =
        AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    final padding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return SingleChildScrollView(
      padding: padding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Info card
          PCard(
            backgroundColor: AppColors.info.withValues(alpha: 0.1),
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(Icons.info_outline, color: AppColors.info),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: Text(
                      'Export your viewing key to create a watch-only '
                      'wallet on another device. Watch-only wallets can see incoming '
                      'transactions but cannot spend funds.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          
          const SizedBox(height: AppSpacing.lg),
          
          // Warning card
          PCard(
            backgroundColor: AppColors.warning.withValues(alpha: 0.1),
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(Icons.warning_amber, color: AppColors.warning),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: Text(
                      'Anyone with this viewing key can see your incoming transactions '
                      'and balance. Only share it with services or devices you trust.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.warning,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          
          const SizedBox(height: AppSpacing.xl),
          
          if (isWatchOnly)
            // Cannot export from watch-only
            Container(
              padding: const EdgeInsets.all(AppSpacing.lg),
              decoration: BoxDecoration(
                color: AppColors.error.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Column(
                children: [
                  Icon(Icons.block, size: 48, color: AppColors.error),
                  const SizedBox(height: AppSpacing.md),
                  Text(
                    'Cannot Export',
                    style: AppTypography.h4.copyWith(color: AppColors.error),
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  Text(
                    'This is a watch-only wallet. Viewing keys can only be exported '
                    'from a full wallet.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                    textAlign: TextAlign.center,
                  ),
                ],
              ),
            )
          else if (_exportedIvk == null)
            // Export button
            PButton(
                  text: 'Export Viewing Key',
                  onPressed: _isExporting ? null : _exportIvk,
                  isLoading: _isExporting,
                  variant: PButtonVariant.primary,
                  size: PButtonSize.large,
                  icon: const Icon(Icons.key),
                )
          else
            // Exported viewing key display
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  'Your viewing key',
                  style: AppTypography.h4.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
                
                const SizedBox(height: AppSpacing.md),
                
                PCard(
                  child: Padding(
                    padding: const EdgeInsets.all(AppSpacing.md),
                    child: SelectableText(
                      _exportedIvk!,
                      style: AppTypography.body.copyWith(
                        fontFamily: 'monospace',
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ),
                
                const SizedBox(height: AppSpacing.md),
                
                PButton(
                  text: _clipboardTimer != null
                      ? 'Copied (clearing in ${_clipboardTimer}s)'
                      : 'Copy to Clipboard',
                  onPressed: _clipboardTimer != null ? null : _copyIvkToClipboard,
                  variant: PButtonVariant.secondary,
                  size: PButtonSize.large,
                  icon: const Icon(Icons.copy),
                ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _buildImportTab() {
    final basePadding =
        AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    final padding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return SingleChildScrollView(
      padding: padding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Info card
          PCard(
            backgroundColor: AppColors.info.withValues(alpha: 0.1),
            child: Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Row(
                children: [
                  Icon(Icons.visibility, color: AppColors.info),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: Text(
                      'Import a viewing key to create a watch-only wallet. '
                      'You will be able to see your balance and incoming transactions, '
                      'but you cannot spend funds.',
                      style: AppTypography.body.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          
          const SizedBox(height: AppSpacing.xl),
          
          // Name input
          PInput(
            controller: _nameController,
            label: 'Wallet Name',
            hint: 'e.g., Watch Wallet',
          ),
          
          const SizedBox(height: AppSpacing.md),
          
          // Viewing key input
          PInput(
            controller: _ivkController,
            label: 'Viewing key',
            hint: 'Paste your viewing key here',
            maxLines: 4,
          ),
          
          const SizedBox(height: AppSpacing.md),
          
          // Birthday height input
          PInput(
            controller: _birthdayController,
            label: 'Birthday Height',
            hint: 'Block height when wallet was created',
            keyboardType: TextInputType.number,
          ),
          
          const SizedBox(height: AppSpacing.sm),
          
          Text(
            'The birthday height is the block when this wallet was first created. '
            'Scanning will start from this height to find your transactions.',
            style: AppTypography.caption.copyWith(
              color: AppColors.textTertiary,
            ),
          ),
          
          if (_importError != null)
            Padding(
              padding: const EdgeInsets.only(top: AppSpacing.md),
              child: Text(
                _importError!,
                style: AppTypography.caption.copyWith(color: AppColors.error),
              ),
            ),
          
          const SizedBox(height: AppSpacing.xl),
          
          PButton(
            text: 'Create Watch-Only Wallet',
            onPressed: _isImporting ? null : _importIvk,
            isLoading: _isImporting,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
          ),
        ],
      ),
    );
  }
}

/// Incoming-only banner widget for watch-only wallets
class WatchOnlyBannerWidget extends StatelessWidget {
  const WatchOnlyBannerWidget({super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpacing.lg,
        vertical: AppSpacing.md,
      ),
      decoration: BoxDecoration(
        color: AppColors.info.withValues(alpha: 0.1),
        border: Border(
          bottom: BorderSide(
            color: AppColors.info.withValues(alpha: 0.3),
          ),
        ),
      ),
      child: Row(
        children: [
          Icon(
            Icons.visibility,
            color: AppColors.info,
            size: 20,
          ),
          const SizedBox(width: AppSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Incoming Only',
                  style: AppTypography.bodyBold.copyWith(
                    color: AppColors.info,
                  ),
                ),
                Text(
                  'This wallet can only view incoming transactions',
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
